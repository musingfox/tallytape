use anyhow::Context;
use rusqlite::{OptionalExtension, Row, TransactionBehavior};

use crate::Database;

/// A receipt row from the `receipts` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: i64,
    pub session_id: Option<i64>,
    pub cwd: String,
    pub date: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Data-access object for the `receipts` table.
pub struct ReceiptRepository {
    db: Database,
}

impl ReceiptRepository {
    /// Create a new repository holding `db` by value.
    ///
    /// `Database` is `Clone` over an `Arc<Mutex<Connection>>`, so holding it
    /// by value is cheap and gives the repository shared access to the same
    /// underlying connection.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Map a `receipts` result row to a [`Receipt`].
    fn map_receipt_row(row: &Row<'_>) -> rusqlite::Result<Receipt> {
        Ok(Receipt {
            id: row.get(0)?,
            session_id: row.get(1)?,
            cwd: row.get(2)?,
            date: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    }

    /// Find or create a receipt for `(cwd, date(occurred_at))`.
    ///
    /// First-write-wins on `session_id`: if a receipt already exists for the
    /// `(cwd, date)` pair the existing row is returned unchanged.
    ///
    /// # Atomicity
    ///
    /// Uses `BEGIN IMMEDIATE` so the SELECT that follows the insert is part of
    /// the same transaction and no concurrent writer can interleave.
    pub fn upsert_by_cwd_date(
        &self,
        seed_session_id: Option<i64>,
        cwd: &str,
        occurred_at: i64,
    ) -> anyhow::Result<Receipt> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("upsert_by_cwd_date: begin transaction failed")?;

        tx.execute(
            "INSERT INTO receipts (session_id, cwd, date) \
             VALUES (?1, ?2, date(?3, 'unixepoch', 'localtime')) \
             ON CONFLICT(cwd, date) DO UPDATE SET cwd = excluded.cwd",
            rusqlite::params![seed_session_id, cwd, occurred_at],
        )
        .context("upsert_by_cwd_date: insert failed")?;

        let receipt = tx
            .query_row(
                "SELECT id, session_id, cwd, date, created_at, updated_at \
                 FROM receipts \
                 WHERE cwd = ?1 AND date = date(?2, 'unixepoch', 'localtime')",
                rusqlite::params![cwd, occurred_at],
                Self::map_receipt_row,
            )
            .context("upsert_by_cwd_date: select failed")?;

        tx.commit().context("upsert_by_cwd_date: commit failed")?;

        Ok(receipt)
    }

    /// Look up a receipt by its primary key.
    ///
    /// Returns `Ok(None)` when no row with that `id` exists.
    pub fn find_by_id(&self, id: i64) -> anyhow::Result<Option<Receipt>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, session_id, cwd, date, created_at, updated_at FROM receipts WHERE id = ?1",
            rusqlite::params![id],
            Self::map_receipt_row,
        )
        .optional()
        .context("find_by_id: query failed")
    }

    /// Return all receipts ordered newest-first (by `id DESC`).
    pub fn list(&self) -> anyhow::Result<Vec<Receipt>> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, cwd, date, created_at, updated_at \
                 FROM receipts ORDER BY id DESC",
            )
            .context("list: query failed")?;

        let rows = stmt
            .query_map([], Self::map_receipt_row)
            .context("list: query failed")?;

        let mut receipts = Vec::new();
        for row in rows {
            receipts.push(row.context("list: query failed")?);
        }
        Ok(receipts)
    }

    /// Return receipts whose `date` column is within `[start_date, end_date]` (inclusive).
    ///
    /// `start_date` and `end_date` must be ISO `YYYY-MM-DD` strings. Malformed
    /// strings are passed straight to SQLite; the `BETWEEN` predicate will be
    /// false for `NULL` dates — the result will be empty rather than an error.
    pub fn list_by_date_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<Vec<Receipt>> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, cwd, date, created_at, updated_at \
                 FROM receipts \
                 WHERE date BETWEEN ?1 AND ?2 \
                 ORDER BY id DESC",
            )
            .context("list_by_date_range: query failed")?;

        let rows = stmt
            .query_map(
                rusqlite::params![start_date, end_date],
                Self::map_receipt_row,
            )
            .context("list_by_date_range: query failed")?;

        let mut receipts = Vec::new();
        for row in rows {
            receipts.push(row.context("list_by_date_range: query failed")?);
        }
        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    /// Open a fresh in-memory database for each test.
    fn open_db() -> Database {
        Database::open(":memory:").expect("in-memory db should open")
    }

    /// Counter used to generate unique `external_id` values.
    static COUNTER: AtomicI64 = AtomicI64::new(1);

    /// Insert a session and return its `rowid`.
    fn insert_session(db: &Database, cwd: Option<&str>, started_at: i64) -> i64 {
        let conn = db.lock();
        let external_id = format!("ext-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        conn.execute(
            "INSERT INTO sessions (source, external_id, cwd, started_at) VALUES ('test', ?1, ?2, ?3)",
            rusqlite::params![external_id, cwd, started_at],
        )
        .expect("insert session should succeed");
        conn.last_insert_rowid()
    }

    // -------------------------------------------------------------------------
    // 1. fresh_insert_creates_receipt
    // -------------------------------------------------------------------------
    #[test]
    fn fresh_insert_creates_receipt() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        // 2024-05-05 00:00:00 UTC = 1714867200
        let occurred_at = 1_714_867_200i64;
        let sid = insert_session(&db, Some("/proj/a"), occurred_at);

        let receipt = repo
            .upsert_by_cwd_date(Some(sid), "/proj/a", occurred_at)
            .expect("upsert should succeed");

        assert!(receipt.id >= 1);
        assert_eq!(receipt.session_id, Some(sid));
        assert_eq!(receipt.cwd, "/proj/a");

        // Verify date matches SQLite's computation
        let conn = db.lock();
        let expected_date: String = conn
            .query_row(
                "SELECT date(?1, 'unixepoch', 'localtime')",
                rusqlite::params![occurred_at],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipt.date, expected_date);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // -------------------------------------------------------------------------
    // 2. upsert_is_idempotent
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_is_idempotent() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let occurred_at = 1_714_867_200i64;
        let sid = insert_session(&db, Some("/proj/a"), occurred_at);

        let r1 = repo
            .upsert_by_cwd_date(Some(sid), "/proj/a", occurred_at)
            .expect("first upsert should succeed");
        let r2 = repo
            .upsert_by_cwd_date(Some(sid), "/proj/a", occurred_at)
            .expect("second upsert should succeed");

        assert_eq!(r1.id, r2.id, "both calls must return the same receipt id");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "exactly one receipt row must exist");
    }

    // -------------------------------------------------------------------------
    // 3. first_write_wins_on_session_id
    // -------------------------------------------------------------------------
    #[test]
    fn first_write_wins_on_session_id() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let t = 1_714_867_200i64;
        let sid1 = insert_session(&db, Some("/proj/a"), t);
        let sid2 = insert_session(&db, Some("/proj/a"), t + 60);

        // First call creates the receipt with session_id = sid1
        let r1 = repo
            .upsert_by_cwd_date(Some(sid1), "/proj/a", t)
            .expect("first upsert should succeed");

        // Second call same (cwd, date) — different session_id, same date window
        let r2 = repo
            .upsert_by_cwd_date(Some(sid2), "/proj/a", t + 60)
            .expect("second upsert should succeed");

        assert_eq!(r1.id, r2.id, "same receipt row should be returned");
        assert_eq!(
            r2.session_id,
            Some(sid1),
            "session_id must not be overwritten (first-write-wins)"
        );

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // -------------------------------------------------------------------------
    // 4. different_cwd_same_time_creates_new_row
    // -------------------------------------------------------------------------
    #[test]
    fn different_cwd_same_time_creates_new_row() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let t = 1_714_867_200i64;
        let sid = insert_session(&db, Some("/proj/a"), t);

        let ra = repo
            .upsert_by_cwd_date(Some(sid), "/proj/a", t)
            .expect("first upsert should succeed");
        let rb = repo
            .upsert_by_cwd_date(Some(sid), "/proj/b", t)
            .expect("second upsert should succeed");

        assert_ne!(ra.id, rb.id, "different cwd must produce different receipts");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    // -------------------------------------------------------------------------
    // 5. empty_cwd_works
    // -------------------------------------------------------------------------
    #[test]
    fn empty_cwd_works() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let t = 1_714_867_200i64;
        let sid1 = insert_session(&db, None, t);
        let sid2 = insert_session(&db, None, t + 60);

        let r1 = repo
            .upsert_by_cwd_date(Some(sid1), "", t)
            .expect("first empty-cwd upsert should succeed");
        let r2 = repo
            .upsert_by_cwd_date(Some(sid2), "", t + 60)
            .expect("second empty-cwd upsert should succeed");

        assert_eq!(r1.id, r2.id, "same date → same receipt for empty cwd");
        assert_eq!(r1.cwd, "");
        assert_eq!(r1.session_id, Some(sid1), "first-write-wins on session_id");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // -------------------------------------------------------------------------
    // 6. find_by_id_returns_new_fields
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_id_returns_new_fields() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let t = 1_714_867_200i64;
        let sid = insert_session(&db, Some("/proj/a"), t);
        let created = repo
            .upsert_by_cwd_date(Some(sid), "/proj/a", t)
            .expect("upsert should succeed");

        let found = repo
            .find_by_id(created.id)
            .expect("find_by_id should not error");

        assert_eq!(found, Some(created.clone()));
        assert!(found.unwrap().date.len() == 10, "date should be YYYY-MM-DD");
    }

    // -------------------------------------------------------------------------
    // 7. list_and_list_by_date_range_work
    // -------------------------------------------------------------------------
    #[test]
    fn list_returns_all_receipts_desc() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        // 2026-05-01T00:00:00 UTC
        let t1 = 1_777_593_600i64;
        // 2026-05-03T00:00:00 UTC
        let t3 = 1_777_766_400i64;

        let sid1 = insert_session(&db, Some("/a"), t1);
        let sid2 = insert_session(&db, Some("/b"), t3);

        repo.upsert_by_cwd_date(Some(sid1), "/a", t1).unwrap();
        repo.upsert_by_cwd_date(Some(sid2), "/b", t3).unwrap();

        let receipts = repo.list().expect("list should succeed");
        assert_eq!(receipts.len(), 2);
        // id DESC: sid2 was inserted last
        assert_eq!(receipts[0].cwd, "/b");
        assert_eq!(receipts[1].cwd, "/a");
    }

    #[test]
    fn list_by_date_range_filters_by_date_column() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        // 2026-05-01T00:00:00 UTC
        let t1 = 1_777_593_600i64;
        // 2026-05-03T00:00:00 UTC
        let t3 = 1_777_766_400i64;
        // 2026-05-05T00:00:00 UTC
        let t5 = 1_777_939_200i64;

        let sid1 = insert_session(&db, Some("/a"), t1);
        let sid2 = insert_session(&db, Some("/b"), t3);
        let sid3 = insert_session(&db, Some("/c"), t5);

        // Get actual date strings from SQLite for the epochs
        let (d1, d3, d5) = {
            let conn = db.lock();
            let d1: String = conn.query_row("SELECT date(?1,'unixepoch','localtime')", rusqlite::params![t1], |r| r.get(0)).unwrap();
            let d3: String = conn.query_row("SELECT date(?1,'unixepoch','localtime')", rusqlite::params![t3], |r| r.get(0)).unwrap();
            let d5: String = conn.query_row("SELECT date(?1,'unixepoch','localtime')", rusqlite::params![t5], |r| r.get(0)).unwrap();
            (d1, d3, d5)
        };

        repo.upsert_by_cwd_date(Some(sid1), "/a", t1).unwrap();
        repo.upsert_by_cwd_date(Some(sid2), "/b", t3).unwrap();
        repo.upsert_by_cwd_date(Some(sid3), "/c", t5).unwrap();

        let results = repo
            .list_by_date_range(&d1, &d3)
            .expect("list_by_date_range should succeed");

        assert_eq!(results.len(), 2, "only /a and /b should be in range");
        assert_eq!(results[0].cwd, "/b"); // id DESC
        assert_eq!(results[1].cwd, "/a");

        // Verify /c is excluded
        let _ = d5; // suppress unused warning
        let all = repo.list_by_date_range(&d1, &d5).unwrap();
        assert_eq!(all.len(), 3);
    }
}
