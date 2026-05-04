use anyhow::Context;
use rusqlite::{OptionalExtension, Row, TransactionBehavior};

use crate::Database;

/// A receipt row from the `receipts` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub id: i64,
    pub session_id: i64,
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
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
        })
    }

    /// Insert a receipt for `session_id` if one does not already exist, then
    /// return the current receipt row.
    ///
    /// # Atomicity
    ///
    /// Uses `BEGIN IMMEDIATE` so the read that follows the insert is part of
    /// the same transaction. The `ON CONFLICT DO NOTHING` strategy ensures the
    /// `AFTER UPDATE` trigger in migration 0002 is never fired, preserving
    /// `updated_at` idempotency.
    ///
    /// # Errors
    ///
    /// Returns `Err` when `session_id` does not reference a row in `sessions`
    /// (foreign-key violation). The transaction is rolled back and no receipt
    /// row is left behind.
    pub fn upsert_by_session_id(&self, session_id: i64) -> anyhow::Result<Receipt> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("upsert_by_session_id: begin transaction failed")?;

        tx.execute(
            "INSERT INTO receipts(session_id) VALUES (?1) ON CONFLICT(session_id) DO NOTHING",
            rusqlite::params![session_id],
        )
        .context("upsert_by_session_id: insert failed")?;

        let receipt = tx
            .query_row(
                "SELECT id, session_id, created_at, updated_at FROM receipts WHERE session_id = ?1",
                rusqlite::params![session_id],
                Self::map_receipt_row,
            )
            .context("upsert_by_session_id: select failed")?;

        tx.commit().context("upsert_by_session_id: commit failed")?;

        Ok(receipt)
    }

    /// Look up a receipt by its primary key.
    ///
    /// Returns `Ok(None)` when no row with that `id` exists.
    pub fn find_by_id(&self, id: i64) -> anyhow::Result<Option<Receipt>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, session_id, created_at, updated_at FROM receipts WHERE id = ?1",
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
            .prepare("SELECT id, session_id, created_at, updated_at FROM receipts ORDER BY id DESC")
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

    /// Return receipts whose session started on a day within `[start_date, end_date]` (UTC, inclusive).
    ///
    /// `start_date` and `end_date` must be ISO `YYYY-MM-DD` strings. Malformed
    /// strings are passed straight to SQLite; `date()` returns `NULL` for them,
    /// which causes the `BETWEEN` predicate to be false — the result will be
    /// empty rather than an error.
    pub fn list_by_date_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<Vec<Receipt>> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT r.id, r.session_id, r.created_at, r.updated_at \
                 FROM receipts r \
                 INNER JOIN sessions s ON s.id = r.session_id \
                 WHERE date(s.started_at, 'unixepoch') BETWEEN ?1 AND ?2 \
                 ORDER BY r.id DESC",
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
    fn insert_session(db: &Database, started_at: i64) -> i64 {
        let conn = db.lock();
        let external_id = format!("ext-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        conn.execute(
            "INSERT INTO sessions (source, external_id, started_at) VALUES ('test', ?1, ?2)",
            rusqlite::params![external_id, started_at],
        )
        .expect("insert session should succeed");
        conn.last_insert_rowid()
    }

    // -------------------------------------------------------------------------
    // 1. upsert_inserts_new_receipt
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_inserts_new_receipt() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let session_id = insert_session(&db, 0);
        let receipt = repo
            .upsert_by_session_id(session_id)
            .expect("upsert should succeed");

        assert_eq!(receipt.session_id, session_id);
        assert!(receipt.id >= 1);
        assert_eq!(receipt.created_at, receipt.updated_at);
    }

    // -------------------------------------------------------------------------
    // 2. upsert_is_idempotent
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_is_idempotent() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let session_id = insert_session(&db, 0);
        let r1 = repo
            .upsert_by_session_id(session_id)
            .expect("first upsert should succeed");
        let r2 = repo
            .upsert_by_session_id(session_id)
            .expect("second upsert should succeed");

        assert_eq!(r1.id, r2.id, "both calls must return the same receipt id");
        assert_eq!(
            r1.updated_at, r2.updated_at,
            "updated_at must not change on second call"
        );

        let conn = db.lock();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM receipts WHERE session_id = ?1",
                rusqlite::params![session_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "exactly one receipt row must exist");
    }

    // -------------------------------------------------------------------------
    // 3. upsert_fk_violation_returns_error_and_no_row
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_fk_violation_returns_error_and_no_row() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        // sessions table is empty; session 999 does not exist
        let result = repo.upsert_by_session_id(999);
        assert!(result.is_err(), "FK violation must return Err");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "no receipt row should exist after FK error");
    }

    // -------------------------------------------------------------------------
    // 4. upsert_recovers_after_fk_failure
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_recovers_after_fk_failure() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        // First: FK violation
        assert!(repo.upsert_by_session_id(999).is_err());

        // Then: valid session + upsert
        let session_id = insert_session(&db, 0);
        let receipt = repo
            .upsert_by_session_id(session_id)
            .expect("upsert after rollback should succeed");
        assert_eq!(receipt.session_id, session_id);
    }

    // -------------------------------------------------------------------------
    // 5. find_by_id_returns_some_then_none
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_id_returns_some_then_none() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let session_id = insert_session(&db, 0);
        let expected = repo
            .upsert_by_session_id(session_id)
            .expect("upsert should succeed");

        let found = repo
            .find_by_id(expected.id)
            .expect("find_by_id should not error");
        assert_eq!(found, Some(expected));

        let missing = repo
            .find_by_id(9999)
            .expect("find_by_id for missing id should not error");
        assert_eq!(missing, None);
    }

    // -------------------------------------------------------------------------
    // 6. list_returns_empty_when_no_rows
    // -------------------------------------------------------------------------
    #[test]
    fn list_returns_empty_when_no_rows() {
        let db = open_db();
        let repo = ReceiptRepository::new(db);

        let receipts = repo.list().expect("list should succeed");
        assert!(receipts.is_empty());
    }

    // -------------------------------------------------------------------------
    // 7. list_orders_by_id_desc
    // -------------------------------------------------------------------------
    #[test]
    fn list_orders_by_id_desc() {
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let s1 = insert_session(&db, 100);
        let s2 = insert_session(&db, 200);
        let s3 = insert_session(&db, 300);

        repo.upsert_by_session_id(s1).unwrap();
        repo.upsert_by_session_id(s2).unwrap();
        repo.upsert_by_session_id(s3).unwrap();

        let receipts = repo.list().expect("list should succeed");
        assert_eq!(receipts.len(), 3);

        let session_ids: Vec<i64> = receipts.iter().map(|r| r.session_id).collect();
        assert_eq!(session_ids, vec![s3, s2, s1]);
    }

    // -------------------------------------------------------------------------
    // 8. list_by_date_range_inclusive_boundaries
    // -------------------------------------------------------------------------
    #[test]
    fn list_by_date_range_inclusive_boundaries() {
        // 2026-05-01T00:00:00Z = 1777593600  → date = "2026-05-01"
        // 2026-05-03T23:59:59Z = 1777852799  → date = "2026-05-03"
        // 2026-05-04T00:00:00Z = 1777852800  → date = "2026-05-04"
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let sa = insert_session(&db, 1_777_593_600); // 2026-05-01
        let sb = insert_session(&db, 1_777_852_799); // 2026-05-03 23:59:59Z → "2026-05-03"
        let sc = insert_session(&db, 1_777_852_800); // 2026-05-04

        let ra = repo.upsert_by_session_id(sa).unwrap();
        let rb = repo.upsert_by_session_id(sb).unwrap();
        repo.upsert_by_session_id(sc).unwrap();

        let results = repo
            .list_by_date_range("2026-05-01", "2026-05-03")
            .expect("list_by_date_range should succeed");

        assert_eq!(results.len(), 2, "only sA and sB should be included");

        // id DESC: rb has larger id than ra
        assert_eq!(results[0].session_id, sb);
        assert_eq!(results[1].session_id, sa);
        let _ = (ra, rb); // suppress unused warnings
    }

    // -------------------------------------------------------------------------
    // 9. list_by_date_range_excludes_outside_days
    // -------------------------------------------------------------------------
    #[test]
    fn list_by_date_range_excludes_outside_days() {
        // 2026-04-30T23:59:59Z = 1777593599 → date = "2026-04-30"
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let s = insert_session(&db, 1_777_593_599); // 2026-04-30
        repo.upsert_by_session_id(s).unwrap();

        let results = repo
            .list_by_date_range("2026-05-01", "2026-05-03")
            .expect("list_by_date_range should succeed");

        assert!(
            results.is_empty(),
            "2026-04-30 must be excluded from [05-01, 05-03]"
        );
    }

    // -------------------------------------------------------------------------
    // 10. list_by_date_range_filters_multi_session
    // -------------------------------------------------------------------------
    #[test]
    fn list_by_date_range_filters_multi_session() {
        // 5 sessions on 5 consecutive days: 2026-04-29 through 2026-05-03
        // Epochs (UTC 00:00:00 each day):
        //   2026-04-29T00:00:00Z = 1777420800
        //   2026-04-30T00:00:00Z = 1777507200
        //   2026-05-01T00:00:00Z = 1777593600
        //   2026-05-02T00:00:00Z = 1777680000
        //   2026-05-03T00:00:00Z = 1777766400
        let db = open_db();
        let repo = ReceiptRepository::new(db.clone());

        let epochs = [
            1_777_420_800i64, // 2026-04-29
            1_777_507_200,    // 2026-04-30
            1_777_593_600,    // 2026-05-01
            1_777_680_000,    // 2026-05-02
            1_777_766_400,    // 2026-05-03
        ];

        let mut session_ids = Vec::new();
        for &epoch in &epochs {
            let sid = insert_session(&db, epoch);
            repo.upsert_by_session_id(sid).unwrap();
            session_ids.push(sid);
        }

        // Query 2026-04-30..2026-05-02 → 3 sessions (indices 1,2,3)
        let results = repo
            .list_by_date_range("2026-04-30", "2026-05-02")
            .expect("list_by_date_range should succeed");

        assert_eq!(results.len(), 3, "expected exactly 3 receipts in range");

        // id DESC means last inserted first
        let result_sids: Vec<i64> = results.iter().map(|r| r.session_id).collect();
        assert_eq!(
            result_sids,
            vec![session_ids[3], session_ids[2], session_ids[1]],
            "results should be id DESC for the 3 in-range sessions"
        );
    }
}
