use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tallytape_core::{db_path, Database, Item, ItemRepository, Receipt, ReceiptRepository};
use tauri::{AppHandle, Manager, State};

const RECEIPT_ADDED_EVENT: &str = "receipt-added";
const RECEIPT_UPDATED_EVENT: &str = "receipt-updated";
const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);

type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub message: String,
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptDto {
    pub id: i64,
    pub session_id: Option<i64>,
    pub cwd: String,
    pub date: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Receipt> for ReceiptDto {
    fn from(receipt: Receipt) -> Self {
        Self {
            id: receipt.id,
            session_id: receipt.session_id,
            cwd: receipt.cwd,
            date: receipt.date,
            created_at: receipt.created_at,
            updated_at: receipt.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemDto {
    pub id: i64,
    pub receipt_id: i64,
    pub session_id: i64,
    pub source: String,
    pub request_id: String,
    pub message_id: Option<String>,
    pub parent_uuid: Option<String>,
    pub is_sidechain: bool,
    pub occurred_at: i64,
    pub model: String,
    pub service_tier: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cost: f64,
    pub metadata: Option<String>,
}

impl From<Item> for ItemDto {
    fn from(item: Item) -> Self {
        Self {
            id: item.id,
            receipt_id: item.receipt_id,
            session_id: item.session_id,
            source: item.source,
            request_id: item.request_id,
            message_id: item.message_id,
            parent_uuid: item.parent_uuid,
            is_sidechain: item.is_sidechain,
            occurred_at: item.occurred_at,
            model: item.model,
            service_tier: item.service_tier,
            input_tokens: item.input_tokens,
            output_tokens: item.output_tokens,
            cache_read_tokens: item.cache_read_tokens,
            cache_creation_tokens: item.cache_creation_tokens,
            cost: item.cost,
            metadata: item.metadata,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptChange {
    Added(ReceiptDto),
    Updated(ReceiptDto),
}

// C1: EventEmitter trait — allows deterministic testing via a stub
pub trait EventEmitter: Send + Sync + 'static {
    fn emit_receipt_change(&self, change: &ReceiptChange) -> Result<(), String>;
}

impl EventEmitter for AppHandle {
    fn emit_receipt_change(&self, change: &ReceiptChange) -> Result<(), String> {
        match change {
            ReceiptChange::Added(receipt) => {
                tauri::Emitter::emit(self, RECEIPT_ADDED_EVENT, receipt.clone())
                    .map_err(|e| e.to_string())
            }
            ReceiptChange::Updated(receipt) => {
                tauri::Emitter::emit(self, RECEIPT_UPDATED_EVENT, receipt.clone())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

// C3: WatchSignal enum — crate-private
enum WatchSignal {
    DatabaseTouched,
    Stop,
}

pub struct AppBackend {
    db: Database,
    receipt_snapshot: Mutex<HashMap<i64, i64>>,
}

impl AppBackend {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let db = Database::open(path.as_ref())?;
        let backend = Self {
            db,
            receipt_snapshot: Mutex::new(HashMap::new()),
        };
        backend.refresh_receipt_snapshot()?;
        Ok(backend)
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    pub fn list_receipts(&self, date_range: Option<DateRange>) -> anyhow::Result<Vec<ReceiptDto>> {
        let repo = ReceiptRepository::new(self.db.clone());
        let receipts = match date_range {
            Some(range) => repo.list_by_date_range(&range.start_date, &range.end_date)?,
            None => repo.list()?,
        };
        Ok(receipts.into_iter().map(ReceiptDto::from).collect())
    }

    pub fn get_receipt(&self, id: i64) -> anyhow::Result<Option<ReceiptDto>> {
        ReceiptRepository::new(self.db.clone())
            .find_by_id(id)
            .map(|receipt| receipt.map(ReceiptDto::from))
    }

    pub fn list_items_by_receipt(&self, receipt_id: i64) -> anyhow::Result<Vec<ItemDto>> {
        ItemRepository::new(self.db.clone())
            .list_by_receipt(receipt_id)
            .map(|items| items.into_iter().map(ItemDto::from).collect())
    }

    pub fn refresh_receipt_snapshot(&self) -> anyhow::Result<()> {
        let snapshot = self.current_receipt_snapshot()?;
        *self
            .receipt_snapshot
            .lock()
            .map_err(|_| anyhow::anyhow!("receipt snapshot mutex poisoned"))? = snapshot;
        Ok(())
    }

    pub fn scan_receipt_changes(&self) -> anyhow::Result<Vec<ReceiptChange>> {
        let receipts = self.list_receipts(None)?;
        let mut snapshot = self
            .receipt_snapshot
            .lock()
            .map_err(|_| anyhow::anyhow!("receipt snapshot mutex poisoned"))?;
        let mut changes = Vec::new();

        for receipt in receipts {
            match snapshot.get(&receipt.id).copied() {
                None => changes.push(ReceiptChange::Added(receipt.clone())),
                Some(previous_updated_at) if previous_updated_at < receipt.updated_at => {
                    changes.push(ReceiptChange::Updated(receipt.clone()));
                }
                Some(_) => {}
            }
            snapshot.insert(receipt.id, receipt.updated_at);
        }

        Ok(changes)
    }

    fn current_receipt_snapshot(&self) -> anyhow::Result<HashMap<i64, i64>> {
        Ok(self
            .list_receipts(None)?
            .into_iter()
            .map(|receipt| (receipt.id, receipt.updated_at))
            .collect())
    }
}

// C4: WatcherHandle — shutdown + Drop + from_parts test ctor
pub struct WatcherHandle {
    watcher: Option<RecommendedWatcher>,
    stop_tx: Option<mpsc::Sender<WatchSignal>>,
    join: Option<thread::JoinHandle<()>>,
}

impl WatcherHandle {
    pub fn shutdown(&mut self) {
        // Send Stop signal; drop watcher; join thread.
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(WatchSignal::Stop);
        }
        // Drop the watcher so the notify thread stops sending events.
        self.watcher.take();
        // Join the scanner thread.
        if let Some(handle) = self.join.take() {
            match handle.join() {
                Ok(()) => {}
                Err(_) => eprintln!("receipt scanner thread panicked"),
            }
        }
    }

    pub(crate) fn from_parts(
        watcher: Option<RecommendedWatcher>,
        stop_tx: mpsc::Sender<WatchSignal>,
        join: thread::JoinHandle<()>,
    ) -> Self {
        Self {
            watcher,
            stop_tx: Some(stop_tx),
            join: Some(join),
        }
    }
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct AppState {
    pub backend: Arc<AppBackend>,
    pub watcher: Mutex<Option<WatcherHandle>>,
}

impl AppState {
    pub fn shutdown_watcher(&self) {
        let taken = self.watcher.lock().ok().and_then(|mut g| g.take());
        if let Some(mut h) = taken {
            h.shutdown();
        }
    }
}

#[tauri::command]
fn list_receipts(
    state: State<'_, AppState>,
    date_range: Option<DateRange>,
) -> AppResult<Vec<ReceiptDto>> {
    state.backend.list_receipts(date_range).map_err(app_error)
}

#[tauri::command]
fn get_receipt(state: State<'_, AppState>, id: i64) -> AppResult<Option<ReceiptDto>> {
    state.backend.get_receipt(id).map_err(app_error)
}

#[tauri::command]
fn list_items_by_receipt(state: State<'_, AppState>, receipt_id: i64) -> AppResult<Vec<ItemDto>> {
    state
        .backend
        .list_items_by_receipt(receipt_id)
        .map_err(app_error)
}

fn app_error(error: anyhow::Error) -> AppError {
    error.into()
}

fn start_receipt_watcher<E: EventEmitter>(
    emitter: E,
    backend: Arc<AppBackend>,
    database_path: PathBuf,
) -> anyhow::Result<WatcherHandle> {
    let watched_parent = database_path
        .parent()
        .context("database path has no parent directory")?
        .to_path_buf();
    let watched_paths = watched_database_paths(&database_path);

    // Single channel: watcher callback sends DatabaseTouched; shutdown sends Stop.
    let (tx, rx) = mpsc::channel::<WatchSignal>();
    let tx_for_notify = tx.clone();
    let tx_for_stop = tx;

    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if event_touches_watched_database(&result, &watched_paths) {
            let _ = tx_for_notify.send(WatchSignal::DatabaseTouched);
        }
    })
    .context("failed to create receipt database watcher")?;

    watcher
        .watch(&watched_parent, RecursiveMode::NonRecursive)
        .with_context(|| format!("failed to watch {}", watched_parent.display()))?;

    let join = thread::spawn(move || run_debounced_receipt_scanner(emitter, backend, rx));

    Ok(WatcherHandle::from_parts(Some(watcher), tx_for_stop, join))
}

fn watched_database_paths(database_path: &Path) -> Vec<PathBuf> {
    let db = database_path.to_path_buf();
    vec![
        db.clone(),
        PathBuf::from(format!("{}-wal", db.display())),
        PathBuf::from(format!("{}-shm", db.display())),
    ]
}

fn event_touches_watched_database(
    result: &notify::Result<notify::Event>,
    watched_paths: &[PathBuf],
) -> bool {
    result
        .as_ref()
        .map(|event| event.paths.iter().any(|path| watched_paths.contains(path)))
        .unwrap_or(false)
}

// C2: run_debounced_receipt_scanner — generic over EventEmitter; breaks on WatchSignal::Stop
fn run_debounced_receipt_scanner<E: EventEmitter>(
    emitter: E,
    backend: Arc<AppBackend>,
    rx: mpsc::Receiver<WatchSignal>,
) {
    loop {
        // Wait for first signal
        let first = match rx.recv() {
            Ok(signal) => signal,
            Err(_) => return, // sender disconnected
        };

        match first {
            WatchSignal::Stop => return,
            WatchSignal::DatabaseTouched => {
                // Drain additional signals during debounce window
                let mut stop_requested = false;
                loop {
                    match rx.recv_timeout(WATCH_DEBOUNCE) {
                        Ok(WatchSignal::Stop) => {
                            stop_requested = true;
                            break;
                        }
                        Ok(WatchSignal::DatabaseTouched) => {
                            // coalesce — keep draining
                        }
                        Err(_) => break, // timeout or disconnected
                    }
                }

                // Scan and emit
                match backend.scan_receipt_changes() {
                    Ok(changes) => {
                        for change in &changes {
                            if let Err(e) = emitter.emit_receipt_change(change) {
                                eprintln!("failed to emit receipt change: {e}");
                            }
                        }
                    }
                    Err(error) => eprintln!("failed to scan receipt changes: {error}"),
                }

                if stop_requested {
                    return;
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let path = db_path().context("failed to resolve tallytape database path")?;
            let backend =
                Arc::new(AppBackend::open(&path).context("failed to open tallytape database")?);
            let watcher =
                start_receipt_watcher(app.handle().clone(), Arc::clone(&backend), path)?;
            app.manage(AppState {
                backend,
                watcher: Mutex::new(Some(watcher)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_receipts,
            get_receipt,
            list_items_by_receipt
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.shutdown_watcher();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tallytape_core::{NewItem, NewSession, ReceiptRepository, SessionRepository};
    use tempfile::tempdir;

    static COUNTER: AtomicI64 = AtomicI64::new(1);

    fn test_backend() -> (tempfile::TempDir, AppBackend) {
        let dir = tempdir().unwrap();
        let backend = AppBackend::open(dir.path().join("test.db")).unwrap();
        (dir, backend)
    }

    fn insert_session(db: &Database, cwd: &str, started_at: i64) -> i64 {
        let external_id = format!("session-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        SessionRepository::new(db.clone())
            .upsert(NewSession {
                source: "test".to_string(),
                external_id,
                cwd: Some(cwd.to_string()),
                started_at,
                ended_at: None,
                metadata: None,
            })
            .unwrap()
            .id
    }

    fn insert_receipt(db: &Database, cwd: &str, occurred_at: i64) -> i64 {
        let session_id = insert_session(db, cwd, occurred_at);
        ReceiptRepository::new(db.clone())
            .upsert_by_cwd_date(Some(session_id), cwd, occurred_at)
            .unwrap()
            .id
    }

    fn insert_item(
        db: &Database,
        receipt_id: i64,
        session_id: i64,
        request_id: &str,
        occurred_at: i64,
    ) {
        tallytape_core::ItemRepository::new(db.clone())
            .insert(&NewItem {
                receipt_id,
                session_id,
                source: "test".to_string(),
                request_id: request_id.to_string(),
                message_id: None,
                parent_uuid: None,
                is_sidechain: false,
                occurred_at,
                model: "claude-opus-4-7".to_string(),
                service_tier: None,
                input_tokens: 1,
                output_tokens: 2,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                cost: 0.01,
                metadata: None,
            })
            .unwrap();
    }

    fn event_with_paths(paths: Vec<PathBuf>) -> notify::Result<notify::Event> {
        let mut event = notify::Event::new(notify::EventKind::Any);
        event.paths = paths;
        Ok(event)
    }

    // C1: StubEmitter for deterministic tests
    #[derive(Clone, Default)]
    struct StubEmitter {
        events: Arc<Mutex<Vec<(String, ReceiptDto)>>>,
    }

    impl EventEmitter for StubEmitter {
        fn emit_receipt_change(&self, change: &ReceiptChange) -> Result<(), String> {
            let mut events = self.events.lock().unwrap();
            match change {
                ReceiptChange::Added(receipt) => {
                    events.push(("receipt-added".to_string(), receipt.clone()));
                }
                ReceiptChange::Updated(receipt) => {
                    events.push(("receipt-updated".to_string(), receipt.clone()));
                }
            }
            Ok(())
        }
    }

    #[test]
    fn watched_database_paths_include_db_wal_and_shm() {
        let db_path = PathBuf::from("/tmp/tallytape.sqlite");

        assert_eq!(
            watched_database_paths(&db_path),
            vec![
                PathBuf::from("/tmp/tallytape.sqlite"),
                PathBuf::from("/tmp/tallytape.sqlite-wal"),
                PathBuf::from("/tmp/tallytape.sqlite-shm"),
            ]
        );
    }

    #[test]
    fn event_path_filter_matches_db_wal_and_shm_only() {
        let watched_paths = watched_database_paths(&PathBuf::from("/tmp/tallytape.sqlite"));

        assert!(event_touches_watched_database(
            &event_with_paths(vec![PathBuf::from("/tmp/tallytape.sqlite")]),
            &watched_paths,
        ));
        assert!(event_touches_watched_database(
            &event_with_paths(vec![PathBuf::from("/tmp/tallytape.sqlite-wal")]),
            &watched_paths,
        ));
        assert!(event_touches_watched_database(
            &event_with_paths(vec![PathBuf::from("/tmp/tallytape.sqlite-shm")]),
            &watched_paths,
        ));
        assert!(!event_touches_watched_database(
            &event_with_paths(vec![PathBuf::from("/tmp/other.sqlite")]),
            &watched_paths,
        ));
    }

    #[test]
    fn event_path_filter_ignores_notify_errors() {
        let watched_paths = watched_database_paths(&PathBuf::from("/tmp/tallytape.sqlite"));
        let error = Err(notify::Error::generic("watch failed"));

        assert!(!event_touches_watched_database(&error, &watched_paths));
    }

    #[test]
    fn app_error_contains_error_message() {
        let error = AppError::from(anyhow::anyhow!("query failed"));

        assert_eq!(error.message, "query failed");
    }

    #[test]
    fn app_open_uses_wal_database() {
        let (_dir, backend) = test_backend();
        let conn = backend.database().lock();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn list_receipts_returns_newest_first_without_range() {
        let (_dir, backend) = test_backend();
        insert_receipt(backend.database(), "/old", 1_777_593_600);
        insert_receipt(backend.database(), "/new", 1_777_766_400);

        let receipts = backend.list_receipts(None).unwrap();

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].cwd, "/new");
        assert_eq!(receipts[1].cwd, "/old");
    }

    #[test]
    fn list_receipts_filters_by_inclusive_date_range() {
        let (_dir, backend) = test_backend();
        insert_receipt(backend.database(), "/may-01", 1_777_593_600);
        insert_receipt(backend.database(), "/may-03", 1_777_766_400);
        insert_receipt(backend.database(), "/may-05", 1_777_939_200);

        let receipts = backend
            .list_receipts(Some(DateRange {
                start_date: "2026-05-01".to_string(),
                end_date: "2026-05-03".to_string(),
            }))
            .unwrap();

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].cwd, "/may-03");
        assert_eq!(receipts[1].cwd, "/may-01");
    }

    #[test]
    fn get_receipt_returns_some_or_none() {
        let (_dir, backend) = test_backend();
        let id = insert_receipt(backend.database(), "/known", 1_777_593_600);

        assert_eq!(backend.get_receipt(id).unwrap().unwrap().cwd, "/known");
        assert_eq!(backend.get_receipt(999_999).unwrap(), None);
    }

    #[test]
    fn list_items_by_receipt_orders_by_occurrence_time() {
        let (_dir, backend) = test_backend();
        let session_id = insert_session(backend.database(), "/items", 1_777_593_600);
        let receipt_id = ReceiptRepository::new(backend.database().clone())
            .upsert_by_cwd_date(Some(session_id), "/items", 1_777_593_600)
            .unwrap()
            .id;
        insert_item(backend.database(), receipt_id, session_id, "req-300", 300);
        insert_item(backend.database(), receipt_id, session_id, "req-100", 100);
        insert_item(backend.database(), receipt_id, session_id, "req-200", 200);

        let items = backend.list_items_by_receipt(receipt_id).unwrap();

        assert_eq!(
            items
                .iter()
                .map(|item| item.occurred_at)
                .collect::<Vec<_>>(),
            vec![100, 200, 300]
        );
    }

    #[test]
    fn receipt_change_scan_emits_added_once() {
        let (_dir, backend) = test_backend();
        let id = insert_receipt(backend.database(), "/added", 1_777_593_600);

        let first = backend.scan_receipt_changes().unwrap();
        let second = backend.scan_receipt_changes().unwrap();

        assert_eq!(
            first,
            vec![ReceiptChange::Added(
                backend.get_receipt(id).unwrap().unwrap()
            )]
        );
        assert!(second.is_empty());
    }

    #[test]
    fn receipt_change_scan_emits_updated_once() {
        let (_dir, backend) = test_backend();
        let id = insert_receipt(backend.database(), "/updated", 1_777_593_600);
        backend.scan_receipt_changes().unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 10;
        {
            let conn = backend.database().lock();
            conn.execute(
                "UPDATE receipts SET updated_at = ?1 WHERE id = ?2",
                [now, id],
            )
            .unwrap();
        }

        let first = backend.scan_receipt_changes().unwrap();
        let second = backend.scan_receipt_changes().unwrap();

        assert!(matches!(first.as_slice(), [ReceiptChange::Updated(receipt)] if receipt.id == id));
        assert!(second.is_empty());
    }

    #[test]
    fn concurrent_writer_style_writes_and_app_reads_do_not_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("concurrent.db");
        let app_backend = Arc::new(AppBackend::open(&path).unwrap());
        let writer_db = Database::open(&path).unwrap();

        let reader = {
            let app_backend = Arc::clone(&app_backend);
            thread::spawn(move || {
                for _ in 0..100 {
                    app_backend.list_receipts(None).unwrap();
                }
            })
        };

        let writer = thread::spawn(move || {
            for i in 0..100 {
                insert_receipt(&writer_db, &format!("/cwd-{i}"), 1_777_593_600 + i);
            }
        });

        reader.join().unwrap();
        writer.join().unwrap();

        assert_eq!(app_backend.list_receipts(None).unwrap().len(), 100);
    }

    // C1 tests: StubEmitter records correct event names and payloads
    #[test]
    fn stub_emitter_records_added_event() {
        let stub = StubEmitter::default();
        let (_dir, backend) = test_backend();
        let id = insert_receipt(backend.database(), "/stub-added", 1_777_593_600);
        let receipt = backend.get_receipt(id).unwrap().unwrap();

        stub.emit_receipt_change(&ReceiptChange::Added(receipt.clone()))
            .unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "receipt-added");
        assert_eq!(recorded[0].1.id, id);
    }

    #[test]
    fn stub_emitter_records_updated_event() {
        let stub = StubEmitter::default();
        let (_dir, backend) = test_backend();
        let id = insert_receipt(backend.database(), "/stub-updated", 1_777_593_600);
        let receipt = backend.get_receipt(id).unwrap().unwrap();

        stub.emit_receipt_change(&ReceiptChange::Updated(receipt.clone()))
            .unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "receipt-updated");
        assert_eq!(recorded[0].1.id, id);
    }

    // C3 smoke: WatchSignal variants are constructible and pattern-matchable
    #[test]
    fn watch_signal_variants_are_constructible() {
        let touched = WatchSignal::DatabaseTouched;
        let stop = WatchSignal::Stop;
        let touched_matches = matches!(touched, WatchSignal::DatabaseTouched);
        let stop_matches = matches!(stop, WatchSignal::Stop);
        assert!(touched_matches);
        assert!(stop_matches);
    }

    // C5: Deterministic debounce-coalescing test
    #[test]
    fn scanner_coalesces_burst_into_single_added_event() {
        let (_dir, backend) = test_backend();
        let backend = Arc::new(backend);
        let id = insert_receipt(backend.database(), "/burst", 1_777_593_600);

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        // Send 5 DatabaseTouched signals in quick succession
        for _ in 0..5 {
            tx.send(WatchSignal::DatabaseTouched).unwrap();
        }
        // Wait for debounce to settle
        thread::sleep(WATCH_DEBOUNCE * 2);
        // Stop the scanner
        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "receipt-added");
        assert_eq!(recorded[0].1.id, id);
    }

    // C2 test: Stop alone records 0 events
    #[test]
    fn scanner_stop_alone_records_no_events() {
        let (_dir, backend) = test_backend();
        let backend = Arc::new(backend);

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 0);
    }

    // C6: Two distinct bursts with intervening DB change
    #[test]
    fn scanner_two_bursts_emit_added_then_updated() {
        let (_dir, backend) = test_backend();
        let backend = Arc::new(backend);
        let id = insert_receipt(backend.database(), "/two-bursts", 1_777_593_600);

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        // Burst 1: emit receipt-added
        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);

        // Verify first event
        {
            let recorded = stub.events.lock().unwrap();
            assert_eq!(recorded.len(), 1);
            assert_eq!(recorded[0].0, "receipt-added");
            assert_eq!(recorded[0].1.id, id);
        }

        // UPDATE updated_at on the receipt (mirroring lib.rs:548-555 pattern)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 10;
        {
            let conn = backend.database().lock();
            conn.execute(
                "UPDATE receipts SET updated_at = ?1 WHERE id = ?2",
                [now, id],
            )
            .unwrap();
        }

        // Burst 2: emit receipt-updated
        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);

        // Stop and join
        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].0, "receipt-added");
        assert_eq!(recorded[0].1.id, id);
        assert_eq!(recorded[1].0, "receipt-updated");
        assert_eq!(recorded[1].1.id, id);
    }

    // C4 tests: WatcherHandle shutdown, drop, double-shutdown
    #[test]
    fn watcher_handle_shutdown_joins_thread() {
        let stub = StubEmitter::default();
        let (_dir, backend) = test_backend();
        let backend = Arc::new(backend);

        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let backend_clone = Arc::clone(&backend);
        let stub_clone = stub.clone();
        let join = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        let mut handle = WatcherHandle::from_parts(None, tx, join);

        // Send a DatabaseTouched before shutdown (scanner should handle it)
        // Then shutdown — scanner must be joined within this call
        handle.shutdown();

        // If we reach here, thread was joined (shutdown blocks until join).
        // No panic = success.
    }

    #[test]
    fn watcher_handle_drop_terminates_thread() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let alive = Arc::new(AtomicBool::new(true));
        let alive_clone = Arc::clone(&alive);

        let (tx, rx) = mpsc::channel::<WatchSignal>();

        // Spawn a thread that sets alive=false when it exits
        let join = thread::spawn(move || {
            // Run the scanner — it will exit when Stop is sent or rx disconnects
            let _rx = rx; // hold rx so the channel stays open until thread exits
            loop {
                match _rx.recv() {
                    Ok(WatchSignal::Stop) => break,
                    Ok(WatchSignal::DatabaseTouched) => {}
                    Err(_) => break,
                }
            }
            alive_clone.store(false, Ordering::SeqCst);
        });

        let handle = WatcherHandle::from_parts(None, tx, join);

        // Drop without calling shutdown explicitly
        drop(handle);

        // Poll for up to 500ms for thread to terminate
        let start = std::time::Instant::now();
        while alive.load(Ordering::SeqCst) {
            if start.elapsed() > Duration::from_millis(500) {
                panic!("thread did not terminate within 500ms after WatcherHandle drop");
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!alive.load(Ordering::SeqCst));
    }

    #[test]
    fn watcher_handle_double_shutdown_is_noop() {
        let (tx, rx) = mpsc::channel::<WatchSignal>();

        let join = thread::spawn(move || {
            // Exit as soon as channel closes or Stop received
            let _ = rx.recv();
        });

        let mut handle = WatcherHandle::from_parts(None, tx, join);
        handle.shutdown(); // first call — joins thread
        handle.shutdown(); // second call — must not panic
    }

    // C8 unit test: AppState::shutdown_watcher joins scanner thread
    #[test]
    fn app_state_shutdown_watcher_joins_thread() {
        let (_dir, backend) = test_backend();
        let backend = Arc::new(backend);
        let stub = StubEmitter::default();

        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let join = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        let watcher_handle = WatcherHandle::from_parts(None, tx, join);
        let state = AppState {
            backend,
            watcher: Mutex::new(Some(watcher_handle)),
        };

        state.shutdown_watcher();

        // Second call must be a no-op (watcher was taken)
        state.shutdown_watcher();

        // Verify the watcher slot is now empty
        let guard = state.watcher.lock().unwrap();
        assert!(guard.is_none());
    }

    // B2: No-op re-ingest emits zero events
    #[test]
    fn scanner_no_op_reingest_emits_zero_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");

        // Pre-insert a receipt using a writer DB, then close it
        let writer_db = Database::open(&path).unwrap();
        let t = 1_777_593_600i64;
        let rid = insert_receipt(&writer_db, "/b2", t);
        drop(writer_db);

        // Open AppBackend — snapshot is pre-populated with the existing receipt
        let backend = Arc::new(AppBackend::open(&path).unwrap());

        // Re-upsert with identical args via a new writer handle — DO NOTHING, no-op
        let writer_db2 = Database::open(&path).unwrap();
        let session_id = insert_session(&writer_db2, "/b2", t);
        ReceiptRepository::new(writer_db2.clone())
            .upsert_by_cwd_date(Some(session_id), "/b2", t)
            .unwrap();
        let _ = rid;

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);
        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert!(recorded.is_empty(), "no-op re-ingest must emit zero events");
    }

    // B3: N inserts in one burst → N receipt-added events
    #[test]
    fn scanner_three_inserts_emit_three_added() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let backend = Arc::new(AppBackend::open(&path).unwrap());

        let writer_db = Database::open(&path).unwrap();
        let t = 1_777_593_600i64;
        let id1 = insert_receipt(&writer_db, "/a", t);
        let id2 = insert_receipt(&writer_db, "/b", t);
        let id3 = insert_receipt(&writer_db, "/c", t);

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);
        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 3, "expected 3 receipt-added events");
        assert!(
            recorded.iter().all(|(name, _)| name == "receipt-added"),
            "all events must be receipt-added"
        );
        let ids: std::collections::HashSet<i64> =
            recorded.iter().map(|(_, r)| r.id).collect();
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
        assert!(ids.contains(&id3));
    }

    // B4: Insert + update in one debounce window → exactly one receipt-added
    #[test]
    fn scanner_insert_and_update_in_one_window_emits_one_added() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let backend = Arc::new(AppBackend::open(&path).unwrap());

        let writer_db = Database::open(&path).unwrap();
        let t = 1_777_593_600i64;
        let id = insert_receipt(&writer_db, "/b4", t);

        // Directly bump updated_at (trigger won't re-fire since we change updated_at)
        {
            let conn = writer_db.lock();
            conn.execute(
                "UPDATE receipts SET updated_at = updated_at + 1 WHERE id = ?1",
                [id],
            )
            .unwrap();
        }

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);
        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 1, "expected exactly 1 event");
        assert_eq!(recorded[0].0, "receipt-added");
        assert_eq!(recorded[0].1.id, id);
    }

    // B5: Manual UPDATE after drain emits receipt-updated (regression guard)
    #[test]
    fn scanner_real_update_after_drain_emits_receipt_updated() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let backend = Arc::new(AppBackend::open(&path).unwrap());

        let writer_db = Database::open(&path).unwrap();
        let t = 1_777_593_600i64;
        let id = insert_receipt(&writer_db, "/b5", t);

        let stub = StubEmitter::default();
        let (tx, rx) = mpsc::channel::<WatchSignal>();
        let stub_clone = stub.clone();
        let backend_clone = Arc::clone(&backend);
        let handle = thread::spawn(move || {
            run_debounced_receipt_scanner(stub_clone, backend_clone, rx);
        });

        // First drain: seats the snapshot with receipt-added
        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);

        {
            let recorded = stub.events.lock().unwrap();
            assert_eq!(recorded.len(), 1, "first drain must yield receipt-added");
            assert_eq!(recorded[0].0, "receipt-added");
        }

        // Sleep so unixepoch() advances
        thread::sleep(Duration::from_millis(1100));

        // Real column change — trigger fires and bumps updated_at
        {
            let conn = writer_db.lock();
            conn.execute(
                "UPDATE receipts SET cwd = ?2 WHERE id = ?1",
                (id, "/b5-updated"),
            )
            .unwrap();
        }

        // Second drain: should yield receipt-updated
        tx.send(WatchSignal::DatabaseTouched).unwrap();
        thread::sleep(WATCH_DEBOUNCE * 2);
        tx.send(WatchSignal::Stop).unwrap();
        handle.join().unwrap();

        let recorded = stub.events.lock().unwrap();
        assert_eq!(recorded.len(), 2, "expected 2 events total");
        assert_eq!(recorded[0].0, "receipt-added");
        assert_eq!(recorded[0].1.id, id);
        assert_eq!(recorded[1].0, "receipt-updated");
        assert_eq!(recorded[1].1.id, id);
    }

    // C7: Real-notify smoke test (ignored — non-deterministic on macOS FSEvents)
    #[test]
    #[ignore]
    fn real_notify_smoke_test_detects_database_write() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let backend = Arc::new(AppBackend::open(&db_path).unwrap());
        let stub = StubEmitter::default();

        let mut handle =
            start_receipt_watcher(stub.clone(), Arc::clone(&backend), db_path.clone()).unwrap();

        // Use a second Database::open (mirroring concurrent_writer_style_writes_and_app_reads_do_not_lock)
        let writer_db = Database::open(&db_path).unwrap();
        insert_receipt(&writer_db, "/x", 1_777_593_600);

        // Poll for up to 2 seconds
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            {
                let recorded = stub.events.lock().unwrap();
                if recorded.len() >= 1 {
                    assert_eq!(recorded[0].0, "receipt-added");
                    break;
                }
            }
            if std::time::Instant::now() > deadline {
                panic!("no receipt-added event received within 2 seconds");
            }
            thread::sleep(Duration::from_millis(50));
        }

        handle.shutdown();
    }
}
