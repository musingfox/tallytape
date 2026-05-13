use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tallytape_core::{db_path, Database, Item, ItemRepository, Receipt, ReceiptRepository};
use tauri::{AppHandle, Emitter, Manager, State};

const RECEIPT_ADDED_EVENT: &str = "receipt-added";
const RECEIPT_UPDATED_EVENT: &str = "receipt-updated";
const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);

type AppResult<T> = Result<T, String>;

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

pub struct AppState {
    backend: Arc<AppBackend>,
    _watcher: RecommendedWatcher,
}

#[tauri::command]
fn list_receipts(
    state: State<'_, AppState>,
    date_range: Option<DateRange>,
) -> AppResult<Vec<ReceiptDto>> {
    state
        .backend
        .list_receipts(date_range)
        .map_err(error_string)
}

#[tauri::command]
fn get_receipt(state: State<'_, AppState>, id: i64) -> AppResult<Option<ReceiptDto>> {
    state.backend.get_receipt(id).map_err(error_string)
}

#[tauri::command]
fn list_items_by_receipt(state: State<'_, AppState>, receipt_id: i64) -> AppResult<Vec<ItemDto>> {
    state
        .backend
        .list_items_by_receipt(receipt_id)
        .map_err(error_string)
}

fn error_string(error: anyhow::Error) -> String {
    error.to_string()
}

fn start_receipt_watcher(
    app: AppHandle,
    backend: Arc<AppBackend>,
    database_path: PathBuf,
) -> anyhow::Result<RecommendedWatcher> {
    let watched_parent = database_path
        .parent()
        .context("database path has no parent directory")?
        .to_path_buf();
    let watched_paths = watched_database_paths(&database_path);
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(event) = result {
            if event.paths.iter().any(|path| watched_paths.contains(path)) {
                let _ = tx.send(());
            }
        }
    })
    .context("failed to create receipt database watcher")?;

    watcher
        .watch(&watched_parent, RecursiveMode::NonRecursive)
        .with_context(|| format!("failed to watch {}", watched_parent.display()))?;

    thread::spawn(move || run_debounced_receipt_scanner(app, backend, rx));

    Ok(watcher)
}

fn watched_database_paths(database_path: &Path) -> Vec<PathBuf> {
    let db = database_path.to_path_buf();
    vec![
        db.clone(),
        PathBuf::from(format!("{}-wal", db.display())),
        PathBuf::from(format!("{}-shm", db.display())),
    ]
}

fn run_debounced_receipt_scanner(app: AppHandle, backend: Arc<AppBackend>, rx: mpsc::Receiver<()>) {
    while rx.recv().is_ok() {
        while rx.recv_timeout(WATCH_DEBOUNCE).is_ok() {}

        match backend.scan_receipt_changes() {
            Ok(changes) => emit_receipt_changes(&app, changes),
            Err(error) => eprintln!("failed to scan receipt changes: {error}"),
        }
    }
}

fn emit_receipt_changes(app: &AppHandle, changes: Vec<ReceiptChange>) {
    for change in changes {
        match change {
            ReceiptChange::Added(receipt) => {
                let _ = app.emit(RECEIPT_ADDED_EVENT, receipt);
            }
            ReceiptChange::Updated(receipt) => {
                let _ = app.emit(RECEIPT_UPDATED_EVENT, receipt);
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
            let watcher = start_receipt_watcher(app.handle().clone(), Arc::clone(&backend), path)?;
            app.manage(AppState {
                backend,
                _watcher: watcher,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_receipts,
            get_receipt,
            list_items_by_receipt
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
}
