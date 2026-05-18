//! BDD step bindings for `tests/features/p6-1-drawer.feature` (API layer).
//!
//! Runs the `@dual` scenarios against `AppBackend` directly — the same code
//! path used by the Tauri IPC handlers and the file watcher. The feature file
//! deliberately scopes itself to happy-path + critical flows; presentation
//! details live in `src/components/__tests__/ReceiptDrawer.test.tsx`.

use cucumber::{gherkin::Step, given, then, when, World};
use tallytape_app_lib::{AppBackend, ReceiptChange};
use tempfile::TempDir;

struct BackendCell(AppBackend);

impl std::fmt::Debug for BackendCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppBackend{..}")
    }
}

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct DrawerWorld {
    tmp: Option<TempDir>,
    backend: Option<BackendCell>,
    last_arrival_cwd: Option<String>,
    detected_changes: Vec<ReceiptChange>,
}

impl DrawerWorld {
    async fn new() -> Self {
        Self {
            tmp: None,
            backend: None,
            last_arrival_cwd: None,
            detected_changes: Vec::new(),
        }
    }

    fn backend(&self) -> &AppBackend {
        &self.backend.as_ref().expect("backend not initialised").0
    }
}

fn seed_receipt(backend: &AppBackend, cwd: &str, date: &str) {
    let conn = backend.database().lock();
    conn.execute(
        "INSERT INTO receipts (cwd, date) VALUES (?1, ?2)",
        rusqlite::params![cwd, date],
    )
    .expect("insert receipt");
}

#[given("a fresh tallytape data directory")]
async fn fresh_data_dir(world: &mut DrawerWorld) {
    let tmp = TempDir::new().expect("tempdir");
    let backend =
        AppBackend::open(tmp.path().join("tallytape.sqlite")).expect("open backend");
    world.tmp = Some(tmp);
    world.backend = Some(BackendCell(backend));
}

#[given("my last few days of work produced these sessions:")]
async fn seed_from_table(world: &mut DrawerWorld, step: &Step) {
    let table = step.table.as_ref().expect("expected data table");
    // First row is the header (cwd | date).
    for row in table.rows.iter().skip(1) {
        let cwd = row.first().expect("missing cwd column");
        let date = row.get(1).expect("missing date column");
        seed_receipt(world.backend(), cwd, date);
    }
}

#[given(regex = r"^the dashboard is showing my (\d+) most recent receipts$")]
async fn dashboard_showing_n(world: &mut DrawerWorld, n: usize) {
    for i in 0..n {
        let cwd = format!("/Users/dev/project-{i}");
        // Pick dates that are clearly older than the "new arrival" date below.
        let date = format!("2026-05-{:02}", 10 + i);
        seed_receipt(world.backend(), &cwd, &date);
    }
    // "Showing" implies the snapshot has been seeded — this matches the watcher
    // boot sequence that calls scan_receipt_changes once at startup.
    world
        .backend()
        .scan_receipt_changes()
        .expect("seed snapshot");
}

#[when("I open the dashboard")]
async fn open_dashboard(world: &mut DrawerWorld) {
    // Seed the snapshot so a subsequent `scan_receipt_changes` reports only the
    // *new* arrivals — mirroring the watcher's startup behaviour.
    world
        .backend()
        .scan_receipt_changes()
        .expect("seed snapshot");
}

#[when("a new Claude Code session finishes")]
async fn new_session(world: &mut DrawerWorld) {
    let cwd = "/Users/dev/just-arrived";
    let date = "2026-05-20";
    seed_receipt(world.backend(), cwd, date);
    world.last_arrival_cwd = Some(cwd.to_string());
    world.detected_changes = world
        .backend()
        .scan_receipt_changes()
        .expect("scan changes");
}

#[then(regex = r"^I see (\d+) receipts$")]
async fn see_n_receipts(world: &mut DrawerWorld, n: usize) {
    let got = world
        .backend()
        .list_receipts(None)
        .expect("list receipts")
        .len();
    assert_eq!(got, n, "expected {n} receipts, got {got}");
}

#[then(regex = r"^I now see (\d+) receipts in total$")]
async fn see_n_total(world: &mut DrawerWorld, n: usize) {
    see_n_receipts(world, n).await;
}

#[then("the most recent receipt appears first")]
async fn most_recent_first(world: &mut DrawerWorld) {
    let receipts = world.backend().list_receipts(None).expect("list receipts");
    assert!(
        receipts.len() >= 2,
        "need at least 2 receipts to assert ordering"
    );
    for pair in receipts.windows(2) {
        assert!(
            pair[0].date >= pair[1].date,
            "expected newest-first ordering by date: got {} before {}",
            pair[0].date,
            pair[1].date,
        );
    }
}

#[then("the new receipt appears highlighted as new")]
async fn appears_highlighted(world: &mut DrawerWorld) {
    let cwd = world
        .last_arrival_cwd
        .as_ref()
        .expect("no arrival recorded");
    let found = world.detected_changes.iter().any(|c| match c {
        ReceiptChange::Added(receipt) => receipt.cwd == *cwd,
        ReceiptChange::Updated(_) => false,
    });
    assert!(
        found,
        "expected scan_receipt_changes to report Added({cwd}); got {:?}",
        world.detected_changes,
    );
}

#[tokio::main]
async fn main() {
    DrawerWorld::cucumber()
        .filter_run("../tests/features", |feature, _rule, scenario| {
            feature
                .tags
                .iter()
                .chain(scenario.tags.iter())
                .any(|t| t == "dual")
        })
        .await;
}
