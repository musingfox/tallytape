use anyhow::Context;
use rusqlite::{Row, TransactionBehavior};

use crate::Database;

/// An item row from the `items` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
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

/// Data needed to insert a new item row.
#[derive(Debug, Clone)]
pub struct NewItem {
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

/// Data-access object for the `items` table.
pub struct ItemRepository {
    db: Database,
}

impl ItemRepository {
    /// Create a new repository holding `db` by value.
    ///
    /// `Database` is `Clone` over an `Arc<Mutex<Connection>>`, so holding it
    /// by value is cheap and gives the repository shared access to the same
    /// underlying connection.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Map an `items` result row to an [`Item`].
    ///
    /// Columns are read by index in declaration order (0-based).
    fn map_item_row(row: &Row<'_>) -> rusqlite::Result<Item> {
        let is_sidechain_raw: i64 = row.get(7)?;
        Ok(Item {
            id: row.get(0)?,
            receipt_id: row.get(1)?,
            session_id: row.get(2)?,
            source: row.get(3)?,
            request_id: row.get(4)?,
            message_id: row.get(5)?,
            parent_uuid: row.get(6)?,
            is_sidechain: is_sidechain_raw != 0,
            occurred_at: row.get(8)?,
            model: row.get(9)?,
            service_tier: row.get(10)?,
            input_tokens: row.get(11)?,
            output_tokens: row.get(12)?,
            cache_read_tokens: row.get(13)?,
            cache_creation_tokens: row.get(14)?,
            cost: row.get(15)?,
            metadata: row.get(16)?,
        })
    }

    /// Insert a new item row and return the freshly-loaded [`Item`].
    ///
    /// # Atomicity
    ///
    /// Uses `BEGIN IMMEDIATE` so the SELECT that reads the inserted row back is
    /// part of the same transaction, and no concurrent writer can interleave.
    ///
    /// # Errors
    ///
    /// Returns `Err` when:
    /// - `receipt_id` does not reference a row in `receipts` (FK violation).
    /// - `session_id` does not reference a row in `sessions` (FK violation).
    /// - The `(source, request_id)` pair already exists (UNIQUE violation).
    ///
    /// The transaction is rolled back and no item row is left behind.
    pub fn insert(&self, new: &NewItem) -> anyhow::Result<Item> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("insert: begin transaction failed")?;

        tx.execute(
            "INSERT INTO items \
             (receipt_id, session_id, source, request_id, message_id, parent_uuid, \
              is_sidechain, occurred_at, model, service_tier, input_tokens, output_tokens, \
              cache_read_tokens, cache_creation_tokens, cost, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                new.receipt_id,
                new.session_id,
                new.source,
                new.request_id,
                new.message_id,
                new.parent_uuid,
                new.is_sidechain as i64,
                new.occurred_at,
                new.model,
                new.service_tier,
                new.input_tokens,
                new.output_tokens,
                new.cache_read_tokens,
                new.cache_creation_tokens,
                new.cost,
                new.metadata,
            ],
        )
        .context("insert: insert failed")?;

        let row_id = tx.last_insert_rowid();

        let item = tx
            .query_row(
                "SELECT id, receipt_id, session_id, source, request_id, message_id, parent_uuid, \
                  is_sidechain, occurred_at, model, service_tier, input_tokens, output_tokens, \
                  cache_read_tokens, cache_creation_tokens, cost, metadata \
                 FROM items WHERE id = ?1",
                rusqlite::params![row_id],
                Self::map_item_row,
            )
            .context("insert: select back failed")?;

        tx.commit().context("insert: commit failed")?;

        Ok(item)
    }

    /// Return all items for `receipt_id`, ordered by `occurred_at ASC`.
    ///
    /// Returns `Ok(vec![])` when no items match — this is not an error.
    pub fn list_by_receipt(&self, receipt_id: i64) -> anyhow::Result<Vec<Item>> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, receipt_id, session_id, source, request_id, message_id, parent_uuid, \
                  is_sidechain, occurred_at, model, service_tier, input_tokens, output_tokens, \
                  cache_read_tokens, cache_creation_tokens, cost, metadata \
                 FROM items WHERE receipt_id = ?1 ORDER BY occurred_at ASC",
            )
            .context("list_by_receipt: prepare failed")?;

        let rows = stmt
            .query_map(rusqlite::params![receipt_id], Self::map_item_row)
            .context("list_by_receipt: query failed")?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row.context("list_by_receipt: row failed")?);
        }
        Ok(items)
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

    /// Counter used to generate unique `external_id` values across tests.
    static COUNTER: AtomicI64 = AtomicI64::new(1);

    /// Insert a session and return its `rowid`.
    fn insert_session(db: &Database, started_at: i64) -> i64 {
        let conn = db.lock();
        let external_id = format!("ext-{}", COUNTER.fetch_add(1, Ordering::Relaxed));
        conn.execute(
            "INSERT INTO sessions (source, external_id, cwd, started_at) VALUES ('test', ?1, '/test', ?2)",
            rusqlite::params![external_id, started_at],
        )
        .expect("insert session should succeed");
        conn.last_insert_rowid()
    }

    /// Insert a receipt for `session_id` and return its `id`.
    fn insert_receipt(db: &Database, session_id: i64, started_at: i64) -> i64 {
        // Get the cwd from the session so upsert_by_cwd_date can use it.
        let cwd: Option<String> = {
            let conn = db.lock();
            conn.query_row(
                "SELECT cwd FROM sessions WHERE id = ?1",
                rusqlite::params![session_id],
                |r| r.get(0),
            )
            .expect("session must exist")
        };
        crate::ReceiptRepository::new(db.clone())
            .upsert_by_cwd_date(Some(session_id), cwd.as_deref().unwrap_or(""), started_at)
            .expect("upsert receipt should succeed")
            .id
    }

    /// Build a `NewItem` with all nullable fields set to `Some`.
    fn sample_new_item(
        receipt_id: i64,
        session_id: i64,
        request_id: &str,
        occurred_at: i64,
    ) -> NewItem {
        NewItem {
            receipt_id,
            session_id,
            source: "test".to_string(),
            request_id: request_id.to_string(),
            message_id: Some("msg_abc".to_string()),
            parent_uuid: Some("uuid-parent".to_string()),
            is_sidechain: true,
            occurred_at,
            model: "claude-opus-4-5".to_string(),
            service_tier: Some("standard".to_string()),
            input_tokens: 100,
            output_tokens: 200,
            cache_read_tokens: Some(50),
            cache_creation_tokens: Some(25),
            cost: 0.042,
            metadata: Some(r#"{"stop_reason":"end_turn"}"#.to_string()),
        }
    }

    // -------------------------------------------------------------------------
    // 1. insert_round_trip_all_fields_some
    // -------------------------------------------------------------------------
    #[test]
    fn insert_round_trip_all_fields_some() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 1_000);
        let rid = insert_receipt(&db, sid, 1_000);
        let new = sample_new_item(rid, sid, "req_some", 1_000);

        let item = repo.insert(&new).expect("insert should succeed");

        assert!(item.id >= 1);
        assert_eq!(item.receipt_id, rid);
        assert_eq!(item.session_id, sid);
        assert_eq!(item.source, "test");
        assert_eq!(item.request_id, "req_some");
        assert_eq!(item.message_id, Some("msg_abc".to_string()));
        assert_eq!(item.parent_uuid, Some("uuid-parent".to_string()));
        assert!(item.is_sidechain);
        assert_eq!(item.occurred_at, 1_000);
        assert_eq!(item.model, "claude-opus-4-5");
        assert_eq!(item.service_tier, Some("standard".to_string()));
        assert_eq!(item.input_tokens, 100);
        assert_eq!(item.output_tokens, 200);
        assert_eq!(item.cache_read_tokens, Some(50));
        assert_eq!(item.cache_creation_tokens, Some(25));
        assert!((item.cost - 0.042).abs() < f64::EPSILON);
        assert_eq!(
            item.metadata,
            Some(r#"{"stop_reason":"end_turn"}"#.to_string())
        );

        let listed = repo
            .list_by_receipt(rid)
            .expect("list_by_receipt should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], item);
    }

    // -------------------------------------------------------------------------
    // 2. insert_round_trip_all_fields_none
    // -------------------------------------------------------------------------
    #[test]
    fn insert_round_trip_all_fields_none() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 2_000);
        let rid = insert_receipt(&db, sid, 2_000);

        let new = NewItem {
            receipt_id: rid,
            session_id: sid,
            source: "test".to_string(),
            request_id: "req_none".to_string(),
            message_id: None,
            parent_uuid: None,
            is_sidechain: false,
            occurred_at: 2_000,
            model: "claude-haiku-3".to_string(),
            service_tier: None,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost: 0.001,
            metadata: None,
        };

        let item = repo.insert(&new).expect("insert should succeed");

        assert_eq!(item.message_id, None);
        assert_eq!(item.parent_uuid, None);
        assert!(!item.is_sidechain);
        assert_eq!(item.service_tier, None);
        assert_eq!(item.cache_read_tokens, None);
        assert_eq!(item.cache_creation_tokens, None);
        assert_eq!(item.metadata, None);

        let listed = repo
            .list_by_receipt(rid)
            .expect("list_by_receipt should succeed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], item);
    }

    // -------------------------------------------------------------------------
    // 3. list_by_receipt_orders_by_occurred_at_asc
    // -------------------------------------------------------------------------
    #[test]
    fn list_by_receipt_orders_by_occurred_at_asc() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 3_000);
        let rid = insert_receipt(&db, sid, 3_000);

        // Insert in order 300, 100, 200
        repo.insert(&sample_new_item(rid, sid, "req_300", 300))
            .unwrap();
        repo.insert(&sample_new_item(rid, sid, "req_100", 100))
            .unwrap();
        repo.insert(&sample_new_item(rid, sid, "req_200", 200))
            .unwrap();

        let items = repo
            .list_by_receipt(rid)
            .expect("list_by_receipt should succeed");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].occurred_at, 100);
        assert_eq!(items[1].occurred_at, 200);
        assert_eq!(items[2].occurred_at, 300);
    }

    // -------------------------------------------------------------------------
    // 4. list_by_receipt_empty_for_unknown_id
    // -------------------------------------------------------------------------
    #[test]
    fn list_by_receipt_empty_for_unknown_id() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let result = repo
            .list_by_receipt(9999)
            .expect("list_by_receipt should not error for unknown id");
        assert!(result.is_empty());
    }

    // -------------------------------------------------------------------------
    // 5. insert_fk_violation_on_missing_receipt_id
    // -------------------------------------------------------------------------
    #[test]
    fn insert_fk_violation_on_missing_receipt_id() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 5_000);
        // receipt_id 9999 does not exist
        let new = sample_new_item(9999, sid, "req_fk_receipt", 5_000);
        let result = repo.insert(&new);
        assert!(result.is_err(), "FK violation must return Err");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "no item row should exist after FK error");
    }

    // -------------------------------------------------------------------------
    // 6. insert_fk_violation_on_missing_session_id
    // -------------------------------------------------------------------------
    #[test]
    fn insert_fk_violation_on_missing_session_id() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 6_000);
        let rid = insert_receipt(&db, sid, 6_000);
        // session_id 9999 does not exist
        let new = sample_new_item(rid, 9999, "req_fk_session", 6_000);
        let result = repo.insert(&new);
        assert!(result.is_err(), "FK violation must return Err");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "no item row should exist after FK error");
    }

    // -------------------------------------------------------------------------
    // 7. insert_unique_violation_on_duplicate_source_request_id
    // -------------------------------------------------------------------------
    #[test]
    fn insert_unique_violation_on_duplicate_source_request_id() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 7_000);
        let rid = insert_receipt(&db, sid, 7_000);
        let new = sample_new_item(rid, sid, "req_dup", 7_000);

        let first = repo.insert(&new);
        assert!(first.is_ok(), "first insert should succeed");

        let second = repo.insert(&new);
        assert!(
            second.is_err(),
            "duplicate (source, request_id) must return Err"
        );

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "exactly one item row must exist");
    }

    // -------------------------------------------------------------------------
    // 8. list_by_receipt_returns_mixed_nullability
    // -------------------------------------------------------------------------
    #[test]
    fn list_by_receipt_returns_mixed_nullability() {
        let db = open_db();
        let repo = ItemRepository::new(db.clone());

        let sid = insert_session(&db, 8_000);
        let rid = insert_receipt(&db, sid, 8_000);

        // All-Some item
        let all_some = repo
            .insert(&sample_new_item(rid, sid, "req_mix_some", 8_001))
            .expect("all-Some insert should succeed");

        // All-None nullable item
        let none_new = NewItem {
            receipt_id: rid,
            session_id: sid,
            source: "test".to_string(),
            request_id: "req_mix_none".to_string(),
            message_id: None,
            parent_uuid: None,
            is_sidechain: false,
            occurred_at: 8_002,
            model: "claude-haiku-3".to_string(),
            service_tier: None,
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost: 0.0,
            metadata: None,
        };
        let all_none = repo
            .insert(&none_new)
            .expect("all-None insert should succeed");

        let items = repo
            .list_by_receipt(rid)
            .expect("list_by_receipt should succeed");
        assert_eq!(items.len(), 2);

        // occurred_at ASC: all_some (8_001) comes first
        assert_eq!(items[0], all_some);
        assert_eq!(items[1], all_none);

        // Verify nullability
        assert!(items[0].message_id.is_some());
        assert!(items[1].message_id.is_none());
        assert!(items[0].cache_read_tokens.is_some());
        assert!(items[1].cache_read_tokens.is_none());
    }
}
