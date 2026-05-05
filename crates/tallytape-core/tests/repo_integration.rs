//! Integration tests: cross-repository FK behaviour, ordering, uniqueness,
//! `v_receipt_totals` view, and the `updated_at` trigger.
//!
//! Every test uses a fresh `Database::open(":memory:")` and is synchronous.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tallytape_core::{
    Database, ItemRepository, NewItem, NewItemDraft, NewSession, ReceiptRepository,
    SessionRepository, merge_item,
};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------
mod helpers {
    use super::*;

    pub fn open_db() -> Database {
        Database::open(":memory:").expect("in-memory db should open")
    }

    static COUNTER: AtomicI64 = AtomicI64::new(1);

    pub fn next_id() -> i64 {
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Insert a session via `SessionRepository::upsert` and return it.
    pub fn make_session(db: &Database, source: &str) -> tallytape_core::Session {
        let repo = SessionRepository::new(db.clone());
        let n = next_id();
        repo.upsert(NewSession {
            source: source.to_string(),
            external_id: format!("ext-{n}"),
            cwd: Some(format!("/cwd-{n}")),
            started_at: 1_714_867_200,
            ended_at: None,
            metadata: None,
        })
        .expect("make_session should succeed")
    }

    /// Insert a session with a specific cwd via `SessionRepository::upsert` and return it.
    pub fn make_session_with_cwd(
        db: &Database,
        source: &str,
        cwd: &str,
    ) -> tallytape_core::Session {
        let repo = SessionRepository::new(db.clone());
        let n = next_id();
        repo.upsert(NewSession {
            source: source.to_string(),
            external_id: format!("ext-{n}"),
            cwd: Some(cwd.to_string()),
            started_at: 1_714_867_200,
            ended_at: None,
            metadata: None,
        })
        .expect("make_session_with_cwd should succeed")
    }

    /// Build a `NewItemDraft` with customisable token/cost values.
    /// `occurred_at` is a unix timestamp (i64).
    pub fn make_draft(
        request_id: &str,
        occurred_at: i64,
        input_tokens: i64,
        output_tokens: i64,
        cost: f64,
    ) -> NewItemDraft {
        // cwd is derived from the session inside merge_item; draft only needs the
        // fields that go directly into the items row.
        NewItemDraft {
            source: "claude".to_string(),
            request_id: request_id.to_string(),
            message_id: None,
            parent_uuid: None,
            is_sidechain: false,
            occurred_at,
            model: "claude-opus-4-5".to_string(),
            service_tier: None,
            input_tokens,
            output_tokens,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost,
            metadata: None,
        }
    }

    /// Query `v_receipt_totals` and return `(total_input_tokens, total_output_tokens, total_cost)`.
    pub fn read_view_totals(db: &Database, receipt_id: i64) -> (i64, i64, f64) {
        let conn = db.lock();
        conn.query_row(
            "SELECT total_input_tokens, total_output_tokens, total_cost \
             FROM v_receipt_totals WHERE receipt_id = ?1",
            rusqlite::params![receipt_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, f64>(2)?)),
        )
        .expect("v_receipt_totals row should exist for receipt_id")
    }
}

use helpers::*;

// ---------------------------------------------------------------------------
// mod fk_behavior
// ---------------------------------------------------------------------------
mod fk_behavior {
    use super::*;

    /// C1 — Deleting a session NULLs the receipt's `session_id` (ON DELETE SET NULL).
    #[test]
    fn session_delete_nulls_receipt() {
        let db = open_db();

        let session = make_session(&db, "claude");
        let receipt_repo = ReceiptRepository::new(db.clone());
        // Use a timestamp that resolves to 2026-05-05 in UTC
        let ts = 1_746_403_200i64; // 2026-05-05 00:00:00 UTC
        let receipt = receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/a", ts)
            .expect("upsert receipt should succeed");

        // Delete the session directly via raw SQL
        {
            let conn = db.lock();
            conn.execute(
                "DELETE FROM sessions WHERE id = ?1",
                rusqlite::params![session.id],
            )
            .expect("DELETE session should succeed");
        }

        // Receipt still exists; session_id is now NULL
        let found = receipt_repo
            .find_by_id(receipt.id)
            .expect("find_by_id should not error")
            .expect("receipt should still exist");

        assert_eq!(found.session_id, None, "session_id should be NULL after session delete");
    }

    /// C2 — Deleting a receipt cascades to its items.
    #[test]
    fn receipt_delete_cascades_items() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        let item1 = merge_item(
            &db,
            session.id,
            make_draft("req-c2a", ts, 10, 20, 0.01),
        )
        .expect("first merge_item should succeed");

        merge_item(
            &db,
            session.id,
            make_draft("req-c2b", ts, 5, 7, 0.005),
        )
        .expect("second merge_item should succeed");

        let receipt_id = item1.receipt_id;

        // Both items exist before deletion
        let item_repo = ItemRepository::new(db.clone());
        assert_eq!(
            item_repo.list_by_receipt(receipt_id).unwrap().len(),
            2,
            "two items should exist before receipt deletion"
        );

        // Delete the receipt
        {
            let conn = db.lock();
            conn.execute(
                "DELETE FROM receipts WHERE id = ?1",
                rusqlite::params![receipt_id],
            )
            .expect("DELETE receipt should succeed");
        }

        // Items should be gone
        assert!(
            item_repo.list_by_receipt(receipt_id).unwrap().is_empty(),
            "items should be deleted when receipt is deleted"
        );
    }

    /// C3 — Deleting a session that still has items referencing it is rejected (FK violation).
    #[test]
    fn session_delete_restrict_when_items_reference() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        merge_item(
            &db,
            session.id,
            make_draft("req-c3", ts, 10, 20, 0.01),
        )
        .expect("merge_item should succeed");

        // Attempt to delete the session — items have FK items.session_id → sessions.id
        // which should restrict the delete.
        let result = {
            let conn = db.lock();
            conn.execute(
                "DELETE FROM sessions WHERE id = ?1",
                rusqlite::params![session.id],
            )
        };

        assert!(result.is_err(), "DELETE session with referencing items must fail");

        // Session should still exist
        let found = SessionRepository::new(db.clone())
            .find_by_id(session.id)
            .expect("find_by_id should not error");
        assert!(found.is_some(), "session should still exist after failed delete");
    }
}

// ---------------------------------------------------------------------------
// mod ordering
// ---------------------------------------------------------------------------
mod ordering {
    use super::*;

    /// C4 — `list_by_receipt` returns items ordered by `occurred_at ASC`.
    #[test]
    fn items_ordered_by_occurred_at_asc() {
        let db = open_db();
        let session = make_session(&db, "claude");

        // Three unix timestamps for 2026-05-05 at 03:00, 01:00, 02:00 UTC
        let t03 = 1_746_414_000i64; // 2026-05-05T03:00:00Z
        let t01 = 1_746_406_800i64; // 2026-05-05T01:00:00Z
        let t02 = 1_746_410_400i64; // 2026-05-05T02:00:00Z

        // Insert in the order 03, 01, 02 to prove sorting is by occurred_at not insert order
        let item_a = merge_item(&db, session.id, make_draft("req-c4a", t03, 1, 1, 0.001))
            .expect("insert 03:00 item");
        merge_item(&db, session.id, make_draft("req-c4b", t01, 1, 1, 0.001))
            .expect("insert 01:00 item");
        merge_item(&db, session.id, make_draft("req-c4c", t02, 1, 1, 0.001))
            .expect("insert 02:00 item");

        let receipt_id = item_a.receipt_id;
        let item_repo = ItemRepository::new(db.clone());
        let items = item_repo.list_by_receipt(receipt_id).expect("list_by_receipt");

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].occurred_at, t01, "first item should be 01:00");
        assert_eq!(items[1].occurred_at, t02, "second item should be 02:00");
        assert_eq!(items[2].occurred_at, t03, "third item should be 03:00");
    }

    /// C5 — `list()` returns receipts ordered by `id DESC`.
    #[test]
    fn receipts_list_orders_by_id_desc() {
        let db = open_db();
        let receipt_repo = ReceiptRepository::new(db.clone());

        // Insert session first (needed for FK when using raw upsert)
        let session = make_session(&db, "claude");

        // Three UTC midnight timestamps on consecutive dates
        // 2026-05-01 UTC = 1777593600, 2026-05-02 = 1777680000, 2026-05-03 = 1777766400
        let t1 = 1_777_593_600i64; // 2026-05-01T00:00:00Z
        let t2 = 1_777_680_000i64; // 2026-05-02T00:00:00Z
        let t3 = 1_777_766_400i64; // 2026-05-03T00:00:00Z

        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t1).expect("upsert t1");
        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t2).expect("upsert t2");
        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t3).expect("upsert t3");

        let receipts = receipt_repo.list().expect("list should succeed");

        // Because UNIQUE(cwd, date), same cwd with different dates creates 3 rows
        assert_eq!(receipts.len(), 3, "should have 3 receipts");

        // Primary assertion: id ordering proves the list is ordered by id DESC
        assert!(
            receipts[0].id > receipts[1].id && receipts[1].id > receipts[2].id,
            "receipts should be ordered by id DESC: {:?}",
            receipts.iter().map(|r| r.id).collect::<Vec<_>>()
        );

        // Secondary assertion: confirm the right rows came back (dates are distinct)
        assert!(
            receipts[0].date >= receipts[1].date && receipts[1].date >= receipts[2].date,
            "dates should also be non-ascending (t3 > t2 > t1): {:?}",
            receipts.iter().map(|r| &r.date).collect::<Vec<_>>()
        );
    }

    /// C6 — `list_by_date_range` returns only receipts in the inclusive range, newest first.
    #[test]
    fn list_by_date_range_inclusive_desc() {
        let db = open_db();
        let receipt_repo = ReceiptRepository::new(db.clone());
        let session = make_session(&db, "claude");

        // 2026-04-30, 2026-05-01, 2026-05-03, 2026-05-04 UTC midnight
        let t_apr30 = 1_777_507_200i64; // 2026-04-30T00:00:00Z
        let t_may01 = 1_777_593_600i64; // 2026-05-01T00:00:00Z
        let t_may03 = 1_777_766_400i64; // 2026-05-03T00:00:00Z
        let t_may04 = 1_777_852_800i64; // 2026-05-04T00:00:00Z

        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t_apr30).expect("upsert apr30");
        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t_may01).expect("upsert may01");
        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t_may03).expect("upsert may03");
        receipt_repo.upsert_by_cwd_date(Some(session.id), "/proj", t_may04).expect("upsert may04");

        // Use SQLite to get the actual date strings for the bounds, to avoid timezone issues
        let (d_may01, d_may03): (String, String) = {
            let conn = db.lock();
            let d1: String = conn
                .query_row("SELECT date(?1,'unixepoch','localtime')", rusqlite::params![t_may01], |r| r.get(0))
                .unwrap();
            let d3: String = conn
                .query_row("SELECT date(?1,'unixepoch','localtime')", rusqlite::params![t_may03], |r| r.get(0))
                .unwrap();
            (d1, d3)
        };

        let results = receipt_repo
            .list_by_date_range(&d_may01, &d_may03)
            .expect("list_by_date_range should succeed");

        assert_eq!(results.len(), 2, "should return 2 receipts in range [{d_may01}, {d_may03}]");
        // id DESC means may03 comes before may01
        assert_eq!(results[0].date, d_may03, "first result should be may03");
        assert_eq!(results[1].date, d_may01, "second result should be may01");
    }
}

// ---------------------------------------------------------------------------
// mod uniqueness
// ---------------------------------------------------------------------------
mod uniqueness {
    use super::*;

    /// C7 — Two `upsert_by_cwd_date` calls with same (cwd, date) return the same row.
    #[test]
    fn receipt_unique_cwd_date_returns_existing() {
        let db = open_db();
        let receipt_repo = ReceiptRepository::new(db.clone());
        let ts = 1_746_403_200i64; // 2026-05-05T00:00:00Z

        let r1 = receipt_repo
            .upsert_by_cwd_date(None, "/proj", ts)
            .expect("first upsert");
        let r2 = receipt_repo
            .upsert_by_cwd_date(None, "/proj", ts)
            .expect("second upsert");

        assert_eq!(r1.id, r2.id, "both upserts should return the same receipt id");

        let all = receipt_repo.list().expect("list");
        assert_eq!(all.len(), 1, "only one receipt should exist");
    }

    /// C8 — Inserting a duplicate (source, request_id) returns Err; row count stays at 1.
    #[test]
    fn item_unique_source_request_id() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        // Create a receipt manually for direct ItemRepository::insert usage
        let receipt_repo = ReceiptRepository::new(db.clone());
        let receipt = receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", ts)
            .expect("upsert receipt");

        let item_repo = ItemRepository::new(db.clone());
        let new_item = NewItem {
            receipt_id: receipt.id,
            session_id: session.id,
            source: "claude".to_string(),
            request_id: format!("req-c8-{}", next_id()),
            message_id: None,
            parent_uuid: None,
            is_sidechain: false,
            occurred_at: ts,
            model: "claude-opus-4-5".to_string(),
            service_tier: None,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost: 0.01,
            metadata: None,
        };

        // First insert succeeds
        item_repo.insert(&new_item).expect("first insert should succeed");

        // Second insert with same (source, request_id) but different occurred_at must fail
        let duplicate = NewItem {
            occurred_at: ts + 3600,
            ..new_item.clone()
        };
        let result = item_repo.insert(&duplicate);
        assert!(result.is_err(), "duplicate (source, request_id) must return Err");

        let items = item_repo.list_by_receipt(receipt.id).expect("list_by_receipt");
        assert_eq!(items.len(), 1, "exactly one item row should exist");
    }

    /// C9 — Two `SessionRepository::upsert` calls with same (source, external_id) return same session.
    #[test]
    fn session_unique_source_external_id() {
        let db = open_db();
        let repo = SessionRepository::new(db.clone());
        let n = next_id();
        let ext_id = format!("ext-c9-{n}");

        let s1 = repo
            .upsert(NewSession {
                source: "claude".to_string(),
                external_id: ext_id.clone(),
                cwd: Some("/proj-a".to_string()),
                started_at: 1_700_000_000,
                ended_at: None,
                metadata: None,
            })
            .expect("first upsert");

        let s2 = repo
            .upsert(NewSession {
                source: "claude".to_string(),
                external_id: ext_id.clone(),
                cwd: Some("/proj-b".to_string()), // different cwd — must be ignored
                started_at: 1_800_000_000,
                ended_at: None,
                metadata: None,
            })
            .expect("second upsert");

        assert_eq!(s1.id, s2.id, "both upserts should return the same session id");

        let found = repo
            .find_by_source_and_external_id("claude", &ext_id)
            .expect("find should not error")
            .expect("session should exist");
        assert_eq!(found.id, s1.id, "find_by_source_and_external_id should return same id");
    }
}

// ---------------------------------------------------------------------------
// mod view_totals
// ---------------------------------------------------------------------------
mod view_totals {
    use super::*;

    /// C10 — `v_receipt_totals` correctly sums tokens and cost across items.
    #[test]
    fn view_aggregates_tokens_and_cost() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        let item1 = merge_item(
            &db,
            session.id,
            make_draft("req-c10a", ts, 10, 20, 0.10),
        )
        .expect("merge item1");

        merge_item(
            &db,
            session.id,
            make_draft("req-c10b", ts + 60, 5, 7, 0.05),
        )
        .expect("merge item2");

        let receipt_id = item1.receipt_id;
        let (total_in, total_out, total_cost) = read_view_totals(&db, receipt_id);

        assert_eq!(total_in, 15, "total_input_tokens should be 10+5=15");
        assert_eq!(total_out, 27, "total_output_tokens should be 20+7=27");
        assert!(
            (total_cost - 0.15f64).abs() < 1e-9,
            "total_cost should be 0.10+0.05=0.15, got {total_cost}"
        );
    }

    /// C11 — View still returns correct totals after receipt's `session_id` is NULLed.
    #[test]
    fn view_survives_null_session_id() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        let item1 = merge_item(
            &db,
            session.id,
            make_draft("req-c11a", ts, 10, 20, 0.10),
        )
        .expect("merge item1");

        merge_item(
            &db,
            session.id,
            make_draft("req-c11b", ts + 60, 5, 7, 0.05),
        )
        .expect("merge item2");

        let receipt_id = item1.receipt_id;

        // Simulate post-SET-NULL state
        {
            let conn = db.lock();
            conn.execute(
                "UPDATE receipts SET session_id = NULL WHERE id = ?1",
                rusqlite::params![receipt_id],
            )
            .expect("UPDATE session_id to NULL should succeed");
        }

        let (total_in, total_out, total_cost) = read_view_totals(&db, receipt_id);

        assert_eq!(total_in, 15, "total_input_tokens should still be 15");
        assert_eq!(total_out, 27, "total_output_tokens should still be 27");
        assert!(
            (total_cost - 0.15f64).abs() < 1e-9,
            "total_cost should still be 0.15, got {total_cost}"
        );
    }
}

// ---------------------------------------------------------------------------
// mod merge_scenarios
// ---------------------------------------------------------------------------
mod merge_scenarios {
    use std::collections::HashMap;

    use super::*;

    /// C13 — Items from multiple sessions × cwds × local-days collapse correctly
    /// on `(cwd, local_date)`. Receipt count, item counts per receipt, and
    /// `v_receipt_totals` aggregates are all verified in a single combined test.
    #[test]
    fn combined_merge_scenarios() {
        let db = open_db();

        // Three sessions: s_a and s_c share /proj-a, s_b is on /proj-b.
        let s_a = make_session_with_cwd(&db, "claude", "/proj-a");
        let s_b = make_session_with_cwd(&db, "claude", "/proj-b");
        let s_c = make_session_with_cwd(&db, "claude", "/proj-a");

        // D1 = 2024-05-05 00:00:00 UTC; D2 is two days later (guarantees distinct local dates).
        let d1: i64 = 1_714_867_200;
        let d2: i64 = d1 + 86_400 * 2;

        // Derive expected date strings from SQLite (TZ-safe).
        let (date_d1, date_d2): (String, String) = {
            let conn = db.lock();
            let s1: String = conn
                .query_row(
                    "SELECT date(?1,'unixepoch','localtime')",
                    rusqlite::params![d1],
                    |r| r.get(0),
                )
                .expect("date_d1 query should succeed");
            let s2: String = conn
                .query_row(
                    "SELECT date(?1,'unixepoch','localtime')",
                    rusqlite::params![d2],
                    |r| r.get(0),
                )
                .expect("date_d2 query should succeed");
            (s1, s2)
        };
        assert_ne!(date_d1, date_d2, "D1 and D2 must map to different local dates");

        // Five merge_item calls covering 3 receipts.
        merge_item(&db, s_a.id, make_draft("req-1", d1, 10, 20, 0.10))
            .expect("merge req-1 should succeed");
        merge_item(&db, s_a.id, make_draft("req-2", d1, 5, 7, 0.05))
            .expect("merge req-2 should succeed");
        // Different session, same cwd and day — should land in the same receipt.
        merge_item(&db, s_c.id, make_draft("req-3", d1, 3, 4, 0.02))
            .expect("merge req-3 should succeed");
        // Same cwd as s_a, but a different day — a new receipt.
        merge_item(&db, s_a.id, make_draft("req-4", d2, 8, 9, 0.08))
            .expect("merge req-4 should succeed");
        // Different cwd, same day as D1 — another new receipt.
        merge_item(&db, s_b.id, make_draft("req-5", d1, 1, 2, 0.01))
            .expect("merge req-5 should succeed");

        // ---- Receipt count ----
        let receipt_repo = ReceiptRepository::new(db.clone());
        let receipts = receipt_repo.list().expect("list should succeed");
        assert_eq!(receipts.len(), 3, "expected exactly 3 receipts");

        // Index receipts by (cwd, date) for keyed assertions.
        let mut receipt_map: HashMap<(String, String), tallytape_core::Receipt> = HashMap::new();
        for r in receipts {
            receipt_map.insert((r.cwd.clone(), r.date.clone()), r);
        }

        // ---- (/proj-a, D1): 3 items, in=18, out=31, cost=0.17 ----
        let key_a_d1 = ("/proj-a".to_string(), date_d1.clone());
        let r_a_d1 = receipt_map
            .get(&key_a_d1)
            .expect("receipt (/proj-a, date_d1) must exist");

        let item_repo = ItemRepository::new(db.clone());
        let items_a_d1 = item_repo
            .list_by_receipt(r_a_d1.id)
            .expect("list_by_receipt (/proj-a, D1)");
        assert_eq!(items_a_d1.len(), 3, "(/proj-a, D1) should have 3 items");

        let (in_a_d1, out_a_d1, cost_a_d1) = read_view_totals(&db, r_a_d1.id);
        assert_eq!(in_a_d1, 18, "(/proj-a, D1) input total should be 10+5+3=18");
        assert_eq!(out_a_d1, 31, "(/proj-a, D1) output total should be 20+7+4=31");
        assert!(
            (cost_a_d1 - 0.17f64).abs() < 1e-9,
            "(/proj-a, D1) cost should be 0.10+0.05+0.02=0.17, got {cost_a_d1}"
        );

        // First-write-wins: the receipt's session_id should be s_a (req-1 was written first).
        assert_eq!(
            r_a_d1.session_id,
            Some(s_a.id),
            "(/proj-a, D1) session_id should be s_a (first-write-wins)"
        );

        // ---- (/proj-a, D2): 1 item, in=8, out=9, cost=0.08 ----
        let key_a_d2 = ("/proj-a".to_string(), date_d2.clone());
        let r_a_d2 = receipt_map
            .get(&key_a_d2)
            .expect("receipt (/proj-a, date_d2) must exist");

        let items_a_d2 = item_repo
            .list_by_receipt(r_a_d2.id)
            .expect("list_by_receipt (/proj-a, D2)");
        assert_eq!(items_a_d2.len(), 1, "(/proj-a, D2) should have 1 item");

        let (in_a_d2, out_a_d2, cost_a_d2) = read_view_totals(&db, r_a_d2.id);
        assert_eq!(in_a_d2, 8, "(/proj-a, D2) input total should be 8");
        assert_eq!(out_a_d2, 9, "(/proj-a, D2) output total should be 9");
        assert!(
            (cost_a_d2 - 0.08f64).abs() < 1e-9,
            "(/proj-a, D2) cost should be 0.08, got {cost_a_d2}"
        );

        // ---- (/proj-b, D1): 1 item, in=1, out=2, cost=0.01 ----
        let key_b_d1 = ("/proj-b".to_string(), date_d1.clone());
        let r_b_d1 = receipt_map
            .get(&key_b_d1)
            .expect("receipt (/proj-b, date_d1) must exist");

        let items_b_d1 = item_repo
            .list_by_receipt(r_b_d1.id)
            .expect("list_by_receipt (/proj-b, D1)");
        assert_eq!(items_b_d1.len(), 1, "(/proj-b, D1) should have 1 item");

        let (in_b_d1, out_b_d1, cost_b_d1) = read_view_totals(&db, r_b_d1.id);
        assert_eq!(in_b_d1, 1, "(/proj-b, D1) input total should be 1");
        assert_eq!(out_b_d1, 2, "(/proj-b, D1) output total should be 2");
        assert!(
            (cost_b_d1 - 0.01f64).abs() < 1e-9,
            "(/proj-b, D1) cost should be 0.01, got {cost_b_d1}"
        );
    }
}

// ---------------------------------------------------------------------------
// mod triggers
// ---------------------------------------------------------------------------
mod triggers {
    use super::*;

    /// C12 — `trg_receipts_updated_at` fires when a second item is merged into
    /// the same receipt, bumping `updated_at` past the value from the first merge.
    #[test]
    fn updated_at_trigger_fires_on_remerge() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        // First merge
        let item1 = merge_item(
            &db,
            session.id,
            make_draft("req-c12a", ts, 10, 20, 0.01),
        )
        .expect("first merge_item");

        let receipt_id = item1.receipt_id;
        let receipt_repo = ReceiptRepository::new(db.clone());
        let r0 = receipt_repo
            .find_by_id(receipt_id)
            .expect("find_by_id should not error")
            .expect("receipt should exist");

        // Sleep 1.1s to ensure unixepoch() advances before the second merge
        std::thread::sleep(Duration::from_millis(1100));

        // Second merge into the same (session, cwd, date) with a different request_id.
        // upsert_by_cwd_date now uses DO UPDATE SET cwd = excluded.cwd, which fires the
        // AFTER UPDATE trigger and bumps updated_at to CURRENT_TIMESTAMP.
        merge_item(
            &db,
            session.id,
            make_draft("req-c12b", ts + 60, 5, 7, 0.005),
        )
        .expect("second merge_item");

        let r1 = receipt_repo
            .find_by_id(receipt_id)
            .expect("find_by_id should not error")
            .expect("receipt should still exist");

        assert!(
            r1.updated_at > r0.updated_at,
            "updated_at ({}) should be greater than original ({}) after trigger fires on remerge",
            r1.updated_at,
            r0.updated_at
        );
    }
}
