use anyhow::Context;
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/0001_initial.sql")),
        M::up(include_str!(
            "../migrations/0002_receipts_updated_at_trigger.sql"
        )),
    ])
}

fn init_connection(path: &Path) -> anyhow::Result<Connection> {
    let mut conn = Connection::open(path)
        .with_context(|| format!("failed to open SQLite database at {}", path.display()))?;

    let journal_mode: String = conn
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .context("failed to set journal_mode=WAL")?;
    let jm_lower = journal_mode.to_ascii_lowercase();
    anyhow::ensure!(
        jm_lower == "wal" || jm_lower == "memory",
        "journal_mode did not switch to WAL (got {journal_mode:?}) — filesystem may not support it"
    );

    conn.pragma_update(None, "foreign_keys", "ON")
        .context("failed to set PRAGMA foreign_keys")?;

    conn.pragma_update(None, "busy_timeout", 5000i64)
        .context("failed to set PRAGMA busy_timeout")?;

    migrations()
        .to_latest(&mut conn)
        .context("failed to run migrations")?;

    Ok(conn)
}

/// A cheap-to-clone, shared handle to a SQLite database connection.
///
/// `Database` wraps a `Connection` in an `Arc<Mutex<...>>` so it can be
/// cloned freely and sent across threads. Cloning produces another handle
/// to the **same** underlying connection (i.e., `Arc::clone` semantics) —
/// it does **not** open a second connection.
///
/// # Pragmas applied on open
///
/// Every `Database` opened via [`Database::open`] has the following pragmas
/// set before any migrations run:
///
/// - `journal_mode=WAL` — enables Write-Ahead Logging for better concurrency.
///   For in-memory databases (`:memory:`), SQLite returns `"memory"` instead
///   of `"wal"`, which is also accepted.
/// - `foreign_keys=ON` — enforces referential integrity.
/// - `busy_timeout=5000` — waits up to 5 s before returning `SQLITE_BUSY`.
///
/// # Migrations
///
/// All pending schema migrations are run automatically during `open`. The
/// schema is the single source of truth; no migration is ever re-run on a
/// database that is already current.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    /// Open (or create) the SQLite database at `path`.
    ///
    /// # What this does
    ///
    /// 1. Opens (or creates) the SQLite file at `path`.
    /// 2. Sets `PRAGMA journal_mode=WAL` (in-memory databases accept `"memory"`).
    /// 3. Sets `PRAGMA foreign_keys=ON`.
    /// 4. Sets `PRAGMA busy_timeout=5000`.
    /// 5. Runs all pending migrations via `rusqlite_migration`.
    ///
    /// # Errors
    ///
    /// - `"failed to open SQLite database at {path}"` — if `Connection::open` fails.
    /// - `ensure!` failure if `journal_mode` returns neither `"wal"` nor `"memory"`.
    /// - `"failed to set PRAGMA foreign_keys"` — if the pragma update fails.
    /// - `"failed to set PRAGMA busy_timeout"` — if the pragma update fails.
    /// - `"failed to run migrations"` — if `Migrations::to_latest` fails.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = init_connection(path.as_ref())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Acquire an exclusive lock on the underlying `Connection`.
    ///
    /// All database access is serialized through this mutex. Only one caller
    /// holds the guard at a time; other callers block until the guard is
    /// dropped.
    ///
    /// # Poison recovery
    ///
    /// If the mutex was poisoned by a panic, `lock()` recovers the guard and
    /// best-effort `ROLLBACK`s any transaction the panicking caller may have
    /// left open, so the next caller starts from a clean transactional state.
    /// SQLite returns an error when no transaction is active; that is silently
    /// ignored.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let guard = poisoned.into_inner();
                // Best-effort: abort any transaction left open by the panicking holder.
                // SQLite returns an error when no transaction is active; that is fine.
                let _ = guard.execute_batch("ROLLBACK");
                guard
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn open_creates_schema() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).expect("open should succeed");
        let conn = db.lock();

        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let tables: HashSet<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for table in &["sessions", "receipts", "items", "pricing"] {
            assert!(tables.contains(*table), "table {table} should exist");
        }
    }

    #[test]
    fn open_sets_wal() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("wal.db")).expect("open should succeed");
        let conn = db.lock();

        let jm: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jm, "wal");
    }

    #[test]
    fn open_sets_foreign_keys() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("fk.db")).expect("open should succeed");
        let conn = db.lock();

        let fk: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn open_sets_busy_timeout_5000() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("bt.db")).expect("open should succeed");
        let conn = db.lock();

        let bt: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(bt, 5000);
    }

    #[test]
    fn open_runs_migrations_to_latest() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("mig.db")).expect("open should succeed");
        let conn = db.lock();

        // sessions table must exist — proof that migration ran
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "sessions table must exist after migration");

        // user_version tracks migration progress in rusqlite_migration
        let uv: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            uv, 2,
            "user_version should match number of migrations applied"
        );
    }

    #[test]
    fn database_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_clone<T: Clone>() {}
        fn assert_debug<T: std::fmt::Debug>() {}
        assert_send_sync::<Database>();
        assert_clone::<Database>();
        assert_debug::<Database>();
    }

    #[test]
    fn lock_sequential_reuse() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("seq.db")).expect("open should succeed");

        {
            let conn = db.lock();
            let val: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
            assert_eq!(val, 1);
        }
        {
            let conn = db.lock();
            let val: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
            assert_eq!(val, 1);
        }
    }

    #[test]
    fn lock_recovers_from_poison() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("poison.db")).expect("open should succeed");
        let db2 = db.clone();

        let handle = thread::spawn(move || {
            let _guard = db2.lock();
            panic!("intentional panic to poison the mutex");
        });

        // The spawned thread panicked — join returns Err
        assert!(handle.join().is_err());

        // lock() must still succeed on the main thread
        let conn = db.lock();
        let val: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(val, 1);
    }

    #[test]
    fn multi_thread_reads_and_writes() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("mt.db")).expect("open should succeed");

        let mut handles = Vec::new();
        for t in 0..8usize {
            let db_clone = db.clone();
            let handle = thread::spawn(move || {
                for i in 0..50usize {
                    let conn = db_clone.lock();
                    let id = format!("t{t}-{i}");
                    let now = 1_700_000_000i64 + (t * 50 + i) as i64;
                    conn.execute(
                        "INSERT INTO sessions (source, external_id, started_at) VALUES (?1, ?2, ?3)",
                        rusqlite::params!["test", id, now],
                    )
                    .expect("insert should succeed");
                    let _count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
                        .unwrap();
                }
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().expect("thread should not panic");
        }

        let conn = db.lock();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 400);
    }

    #[test]
    fn open_in_memory_succeeds() {
        let db = Database::open(":memory:").expect("in-memory open should succeed");
        let conn = db.lock();
        let val: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(val, 1);
    }

    #[test]
    fn lock_rolls_back_after_poison() {
        let dir = tempdir().unwrap();
        let db =
            Database::open(dir.path().join("poison_rollback.db")).expect("open should succeed");
        let db2 = db.clone();

        let handle = thread::spawn(move || {
            let conn = db2.lock();
            conn.execute_batch(
                "BEGIN; INSERT INTO sessions(source, external_id, started_at) VALUES ('test', 'poisoned', 0);",
            )
            .expect("begin+insert should succeed");
            panic!("intentional panic with open transaction");
        });

        // The spawned thread panicked — join returns Err
        assert!(handle.join().is_err());

        // lock() recovers and issues ROLLBACK; the insert must not be visible
        let conn = db.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE external_id = 'poisoned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "poisoned insert should have been rolled back");
    }

    #[test]
    fn receipts_updated_at_refreshed_on_update() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("touch.db")).expect("open should succeed");
        let conn = db.lock();

        // Seed a session and a receipt with a backdated updated_at.
        conn.execute(
            "INSERT INTO sessions (source, external_id, started_at) VALUES ('test', 's1', 0)",
            [],
        )
        .unwrap();
        let session_id: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO receipts (session_id, created_at, updated_at) VALUES (?1, 1000, 1000)",
            rusqlite::params![session_id],
        )
        .unwrap();
        let receipt_id: i64 = conn.last_insert_rowid();

        // Touch a non-updated_at column; trigger should bump updated_at to unixepoch().
        conn.execute(
            "UPDATE receipts SET created_at = 2000 WHERE id = ?1",
            rusqlite::params![receipt_id],
        )
        .unwrap();

        let updated_at: i64 = conn
            .query_row(
                "SELECT updated_at FROM receipts WHERE id = ?1",
                rusqlite::params![receipt_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            updated_at > 1000,
            "updated_at should be refreshed to a current unix epoch (got {updated_at})"
        );

        // Explicit updated_at should be preserved (WHEN guard prevents re-fire).
        conn.execute(
            "UPDATE receipts SET updated_at = 9999 WHERE id = ?1",
            rusqlite::params![receipt_id],
        )
        .unwrap();
        let explicit: i64 = conn
            .query_row(
                "SELECT updated_at FROM receipts WHERE id = ?1",
                rusqlite::params![receipt_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(explicit, 9999, "explicit updated_at must be respected");
    }
}
