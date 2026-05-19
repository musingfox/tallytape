//! Integration tests: cross-repository FK behaviour, ordering, uniqueness,
//! `v_receipt_totals` view, and the `updated_at` trigger.
//!
//! Every test uses a fresh `Database::open(":memory:")` and is synchronous.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tallytape_core::{
    merge_item, AppSettingsRepository, Database, ItemRepository, NewItem, NewItemDraft, NewSession,
    ReceiptRepository, SessionRepository, SummaryRepository, LAST_SEEN_MAX_UPDATED_AT,
    LAST_SEEN_RECEIPT_ID,
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
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
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

        assert_eq!(
            found.session_id, None,
            "session_id should be NULL after session delete"
        );
    }

    /// C2 — Deleting a receipt cascades to its items.
    #[test]
    fn receipt_delete_cascades_items() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        let item1 = merge_item(&db, session.id, make_draft("req-c2a", ts, 10, 20, 0.01))
            .expect("first merge_item should succeed");

        merge_item(&db, session.id, make_draft("req-c2b", ts, 5, 7, 0.005))
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

        merge_item(&db, session.id, make_draft("req-c3", ts, 10, 20, 0.01))
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

        assert!(
            result.is_err(),
            "DELETE session with referencing items must fail"
        );

        // Session should still exist
        let found = SessionRepository::new(db.clone())
            .find_by_id(session.id)
            .expect("find_by_id should not error");
        assert!(
            found.is_some(),
            "session should still exist after failed delete"
        );
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
        let items = item_repo
            .list_by_receipt(receipt_id)
            .expect("list_by_receipt");

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

        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t1)
            .expect("upsert t1");
        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t2)
            .expect("upsert t2");
        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t3)
            .expect("upsert t3");

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

        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t_apr30)
            .expect("upsert apr30");
        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t_may01)
            .expect("upsert may01");
        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t_may03)
            .expect("upsert may03");
        receipt_repo
            .upsert_by_cwd_date(Some(session.id), "/proj", t_may04)
            .expect("upsert may04");

        // Use SQLite to get the actual date strings for the bounds, to avoid timezone issues
        let (d_may01, d_may03): (String, String) = {
            let conn = db.lock();
            let d1: String = conn
                .query_row(
                    "SELECT date(?1,'unixepoch','localtime')",
                    rusqlite::params![t_may01],
                    |r| r.get(0),
                )
                .unwrap();
            let d3: String = conn
                .query_row(
                    "SELECT date(?1,'unixepoch','localtime')",
                    rusqlite::params![t_may03],
                    |r| r.get(0),
                )
                .unwrap();
            (d1, d3)
        };

        let results = receipt_repo
            .list_by_date_range(&d_may01, &d_may03)
            .expect("list_by_date_range should succeed");

        assert_eq!(
            results.len(),
            2,
            "should return 2 receipts in range [{d_may01}, {d_may03}]"
        );
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

        assert_eq!(
            r1.id, r2.id,
            "both upserts should return the same receipt id"
        );

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
        item_repo
            .insert(&new_item)
            .expect("first insert should succeed");

        // Second insert with same (source, request_id) but different occurred_at must fail
        let duplicate = NewItem {
            occurred_at: ts + 3600,
            ..new_item.clone()
        };
        let result = item_repo.insert(&duplicate);
        assert!(
            result.is_err(),
            "duplicate (source, request_id) must return Err"
        );

        let items = item_repo
            .list_by_receipt(receipt.id)
            .expect("list_by_receipt");
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

        assert_eq!(
            s1.id, s2.id,
            "both upserts should return the same session id"
        );

        let found = repo
            .find_by_source_and_external_id("claude", &ext_id)
            .expect("find should not error")
            .expect("session should exist");
        assert_eq!(
            found.id, s1.id,
            "find_by_source_and_external_id should return same id"
        );
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

        let item1 = merge_item(&db, session.id, make_draft("req-c10a", ts, 10, 20, 0.10))
            .expect("merge item1");

        merge_item(&db, session.id, make_draft("req-c10b", ts + 60, 5, 7, 0.05))
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

        let item1 = merge_item(&db, session.id, make_draft("req-c11a", ts, 10, 20, 0.10))
            .expect("merge item1");

        merge_item(&db, session.id, make_draft("req-c11b", ts + 60, 5, 7, 0.05))
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
        assert_ne!(
            date_d1, date_d2,
            "D1 and D2 must map to different local dates"
        );

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
        assert_eq!(
            out_a_d1, 31,
            "(/proj-a, D1) output total should be 20+7+4=31"
        );
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

    /// Contract B1 — Re-merging an item into the same `(cwd, date)` receipt is
    /// a no-op: `upsert_by_cwd_date` now uses `DO NOTHING`, so the
    /// `trg_receipts_updated_at` trigger never fires and `updated_at` is
    /// unchanged even after a >1 s sleep.
    #[test]
    fn merge_item_no_op_does_not_bump_updated_at() {
        let db = open_db();
        let session = make_session(&db, "claude");
        let ts = 1_746_403_200i64;

        // First merge — creates the receipt
        let item1 = merge_item(&db, session.id, make_draft("req-c12a", ts, 10, 20, 0.01))
            .expect("first merge_item");

        let receipt_id = item1.receipt_id;
        let receipt_repo = ReceiptRepository::new(db.clone());
        let r0 = receipt_repo
            .find_by_id(receipt_id)
            .expect("find_by_id should not error")
            .expect("receipt should exist");

        // Sleep 1.1s so unixepoch() would tick if the trigger fired
        std::thread::sleep(Duration::from_millis(1100));

        // Second merge into the same (session, cwd, date) with a different request_id.
        // upsert_by_cwd_date now uses DO NOTHING, so no UPDATE is issued and
        // trg_receipts_updated_at does not fire.
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

        assert_eq!(
            r1.updated_at, r0.updated_at,
            "updated_at must not change on no-op re-merge (DO NOTHING)"
        );
    }
}

// ---------------------------------------------------------------------------
// mod aggregation
// ---------------------------------------------------------------------------
mod aggregation {
    use super::*;
    use tallytape_core::{AggregationBucket, AggregationRepository, ModelBreakdown};

    fn date_for(db: &Database, ts: i64) -> String {
        let conn = db.lock();
        conn.query_row(
            "SELECT date(?1,'unixepoch','localtime')",
            rusqlite::params![ts],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn bucket_for(db: &Database, fmt: &str, date: &str) -> String {
        let conn = db.lock();
        conn.query_row(
            "SELECT strftime(?1, ?2)",
            rusqlite::params![fmt, date],
            |r| r.get(0),
        )
        .unwrap()
    }

    struct CustomItem<'a> {
        session_id: i64,
        cwd: &'a str,
        ts: i64,
        model: &'a str,
        input_tokens: i64,
        output_tokens: i64,
        cost: f64,
        cache_read_tokens: Option<i64>,
        cache_creation_tokens: Option<i64>,
    }

    fn custom_item(db: &Database, args: CustomItem<'_>) {
        let receipt = ReceiptRepository::new(db.clone())
            .upsert_by_cwd_date(Some(args.session_id), args.cwd, args.ts)
            .expect("upsert receipt");
        ItemRepository::new(db.clone())
            .insert(&NewItem {
                receipt_id: receipt.id,
                session_id: args.session_id,
                source: "claude".to_string(),
                request_id: format!("req-agg-{}", next_id()),
                message_id: None,
                parent_uuid: None,
                is_sidechain: false,
                occurred_at: args.ts,
                model: args.model.to_string(),
                service_tier: None,
                input_tokens: args.input_tokens,
                output_tokens: args.output_tokens,
                cache_read_tokens: args.cache_read_tokens,
                cache_creation_tokens: args.cache_creation_tokens,
                cost: args.cost,
                metadata: None,
            })
            .expect("insert item");
    }

    fn draft_with_id(
        request_id: &str,
        ts: i64,
        input: i64,
        output: i64,
        cost: f64,
    ) -> NewItemDraft {
        make_draft(request_id, ts, input, output, cost)
    }

    #[test]
    fn daily_multi_model_single_bucket() {
        let db = open_db();
        let ts = 1_779_019_200i64;
        let d = date_for(&db, ts);
        let session = make_session_with_cwd(&db, "claude", "/agg-t1");
        custom_item(
            &db,
            CustomItem {
                session_id: session.id,
                cwd: "/agg-t1",
                ts,
                model: "opus",
                input_tokens: 10,
                output_tokens: 20,
                cost: 0.10,
                cache_read_tokens: None,
                cache_creation_tokens: None,
            },
        );
        custom_item(
            &db,
            CustomItem {
                session_id: session.id,
                cwd: "/agg-t1",
                ts: ts + 60,
                model: "haiku",
                input_tokens: 5,
                output_tokens: 7,
                cost: 0.01,
                cache_read_tokens: None,
                cache_creation_tokens: None,
            },
        );

        let actual = AggregationRepository::new(db)
            .aggregate_daily(&d, &d)
            .unwrap();
        assert_eq!(
            actual,
            vec![AggregationBucket {
                bucket: d,
                receipt_count: 1,
                total_cost: 0.11,
                total_tokens: 42,
                model_breakdown: vec![
                    ModelBreakdown {
                        model: "haiku".to_string(),
                        count: 1,
                        cost: 0.01,
                        tokens: 12
                    },
                    ModelBreakdown {
                        model: "opus".to_string(),
                        count: 1,
                        cost: 0.10,
                        tokens: 30
                    },
                ],
            }]
        );
    }

    #[test]
    fn daily_excludes_cache_tokens() {
        let db = open_db();
        let ts = 1_779_019_200i64;
        let d = date_for(&db, ts);
        let session = make_session_with_cwd(&db, "claude", "/agg-t2");
        custom_item(
            &db,
            CustomItem {
                session_id: session.id,
                cwd: "/agg-t2",
                ts,
                model: "opus",
                input_tokens: 10,
                output_tokens: 20,
                cost: 0.10,
                cache_read_tokens: Some(1000),
                cache_creation_tokens: Some(500),
            },
        );

        let actual = AggregationRepository::new(db)
            .aggregate_daily(&d, &d)
            .unwrap();
        assert_eq!(actual[0].total_tokens, 30);
        assert_eq!(actual[0].model_breakdown[0].tokens, 30);
    }

    #[test]
    fn daily_empty_range() {
        let db = open_db();
        let actual = AggregationRepository::new(db)
            .aggregate_daily("2026-05-01", "2026-05-31")
            .unwrap();
        assert!(actual.is_empty());
    }

    #[test]
    fn daily_zero_item_receipt_excluded() {
        let db = open_db();
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO receipts (session_id, cwd, date) VALUES (NULL, '/empty', '2026-05-17')",
                [],
            )
            .unwrap();
        }
        let session = make_session_with_cwd(&db, "claude", "/agg-t4");
        let ts = 1_779_105_600i64; // 2026-05-18 12:00 UTC
        let d = date_for(&db, ts);
        merge_item(&db, session.id, draft_with_id("req-t4", ts, 1, 2, 0.01)).unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_daily("2026-05-17", &d)
            .unwrap();
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].bucket, d);
    }

    #[test]
    fn daily_sparse_multi_day_range() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/agg-t5");
        let t1 = 1_777_636_800i64; // 2026-05-01 12:00 UTC
        let t5 = 1_777_982_400i64; // 2026-05-05 12:00 UTC
        let d1 = date_for(&db, t1);
        let d5 = date_for(&db, t5);
        merge_item(&db, session.id, draft_with_id("req-t5a", t1, 1, 1, 0.01)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t5b", t5, 1, 1, 0.01)).unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_daily(&d1, "2026-05-10")
            .unwrap();
        assert_eq!(
            actual.iter().map(|b| b.bucket.clone()).collect::<Vec<_>>(),
            vec![d1, d5]
        );
    }

    #[test]
    fn daily_boundary_inclusive() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/agg-t6");
        let t1 = 1_777_636_800i64;
        let t2 = t1 + 86_400;
        let t3 = t2 + 86_400;
        let d1 = date_for(&db, t1);
        let d2 = date_for(&db, t2);
        merge_item(&db, session.id, draft_with_id("req-t6a", t1, 1, 1, 0.01)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t6b", t2, 1, 1, 0.01)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t6c", t3, 1, 1, 0.01)).unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_daily(&d1, &d2)
            .unwrap();
        assert_eq!(actual.len(), 2);
        assert_eq!(
            actual.iter().map(|b| b.bucket.clone()).collect::<Vec<_>>(),
            vec![d1, d2]
        );
    }

    #[test]
    fn weekly_year_crossover_iso_week() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/agg-t7");
        let t1 = 1_798_459_200i64;
        let t2 = 1_798_977_600i64;
        let d1 = date_for(&db, t1);
        let d2 = date_for(&db, t2);
        let w1 = bucket_for(&db, "%G-W%V", &d1);
        let w2 = bucket_for(&db, "%G-W%V", &d2);
        assert_eq!(w1, w2);
        merge_item(&db, session.id, draft_with_id("req-t7a", t1, 10, 20, 0.10)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t7b", t2, 5, 7, 0.01)).unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_weekly(&d1, &d2)
            .unwrap();
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].bucket, w1);
        assert_eq!(actual[0].receipt_count, 2);
        assert_eq!(actual[0].total_tokens, 42);
        assert!((actual[0].total_cost - 0.11).abs() < 1e-9);
    }

    #[test]
    fn weekly_week_change_ordered() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/agg-t8");
        let sunday = 1_798_977_600i64;
        let monday = 1_799_064_000i64;
        let d1 = date_for(&db, sunday);
        let d2 = date_for(&db, monday);
        let w1 = bucket_for(&db, "%G-W%V", &d1);
        let w2 = bucket_for(&db, "%G-W%V", &d2);
        merge_item(
            &db,
            session.id,
            draft_with_id("req-t8a", monday, 1, 1, 0.01),
        )
        .unwrap();
        merge_item(
            &db,
            session.id,
            draft_with_id("req-t8b", sunday, 1, 1, 0.01),
        )
        .unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_weekly(&d1, &d2)
            .unwrap();
        assert_eq!(
            actual.iter().map(|b| b.bucket.clone()).collect::<Vec<_>>(),
            vec![w1, w2]
        );
    }

    #[test]
    fn weekly_empty_range() {
        let db = open_db();
        assert!(AggregationRepository::new(db)
            .aggregate_weekly("2026-05-01", "2026-05-07")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn monthly_month_boundary() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/agg-t10");
        let t1 = 1_777_550_400i64; // 2026-04-30 12:00 UTC
        let t2 = 1_777_636_800i64; // 2026-05-01 12:00 UTC
        let d1 = date_for(&db, t1);
        let d2 = date_for(&db, t2);
        let m1 = bucket_for(&db, "%Y-%m", &d1);
        let m2 = bucket_for(&db, "%Y-%m", &d2);
        merge_item(&db, session.id, draft_with_id("req-t10a", t1, 1, 1, 0.01)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t10b", t2, 1, 1, 0.01)).unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_monthly(&d1, &d2)
            .unwrap();
        assert_eq!(
            actual.iter().map(|b| b.bucket.clone()).collect::<Vec<_>>(),
            vec![m1, m2]
        );
    }

    #[test]
    fn monthly_multi_receipt_same_month() {
        let db = open_db();
        let t = 1_779_019_200i64;
        let d = date_for(&db, t);
        for (idx, cwd) in ["/agg-t11a", "/agg-t11b", "/agg-t11c"].iter().enumerate() {
            let session = make_session_with_cwd(&db, "claude", cwd);
            merge_item(
                &db,
                session.id,
                draft_with_id(&format!("req-t11-{idx}"), t + idx as i64 * 60, 10, 20, 0.10),
            )
            .unwrap();
        }
        let actual = AggregationRepository::new(db)
            .aggregate_monthly(&d, &d)
            .unwrap();
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].receipt_count, 3);
        assert_eq!(actual[0].total_tokens, 90);
        assert!((actual[0].total_cost - 0.30).abs() < 1e-9);
    }

    #[test]
    fn monthly_empty_range() {
        let db = open_db();
        assert!(AggregationRepository::new(db)
            .aggregate_monthly("2026-05-01", "2026-05-31")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn mixed_model_deterministic_order() {
        let db = open_db();
        let ts = 1_779_019_200i64;
        let d = date_for(&db, ts);
        let session = make_session_with_cwd(&db, "claude", "/agg-t13");
        custom_item(
            &db,
            CustomItem {
                session_id: session.id,
                cwd: "/agg-t13",
                ts,
                model: "zeta-1",
                input_tokens: 1,
                output_tokens: 1,
                cost: 0.01,
                cache_read_tokens: None,
                cache_creation_tokens: None,
            },
        );
        custom_item(
            &db,
            CustomItem {
                session_id: session.id,
                cwd: "/agg-t13",
                ts: ts + 1,
                model: "alpha-1",
                input_tokens: 1,
                output_tokens: 1,
                cost: 0.01,
                cache_read_tokens: None,
                cache_creation_tokens: None,
            },
        );
        custom_item(
            &db,
            CustomItem {
                session_id: session.id,
                cwd: "/agg-t13",
                ts: ts + 2,
                model: "middle",
                input_tokens: 1,
                output_tokens: 1,
                cost: 0.01,
                cache_read_tokens: None,
                cache_creation_tokens: None,
            },
        );

        let actual = AggregationRepository::new(db)
            .aggregate_daily(&d, &d)
            .unwrap();
        assert_eq!(
            actual[0]
                .model_breakdown
                .iter()
                .map(|m| m.model.clone())
                .collect::<Vec<_>>(),
            vec![
                "alpha-1".to_string(),
                "middle".to_string(),
                "zeta-1".to_string()
            ]
        );
    }

    #[test]
    fn multi_bucket_order_ascending() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/agg-t14");
        let t1 = 1_777_636_800i64;
        let t2 = t1 + 86_400;
        let t3 = t2 + 86_400;
        let d1 = date_for(&db, t1);
        let d2 = date_for(&db, t2);
        let d3 = date_for(&db, t3);
        merge_item(&db, session.id, draft_with_id("req-t14c", t3, 1, 1, 0.01)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t14a", t1, 1, 1, 0.01)).unwrap();
        merge_item(&db, session.id, draft_with_id("req-t14b", t2, 1, 1, 0.01)).unwrap();

        let actual = AggregationRepository::new(db)
            .aggregate_daily(&d1, &d3)
            .unwrap();
        assert_eq!(
            actual.iter().map(|b| b.bucket.clone()).collect::<Vec<_>>(),
            vec![d1, d2, d3]
        );
    }
}

// ---------------------------------------------------------------------------
// mod summary
// ---------------------------------------------------------------------------
mod summary {
    use super::*;

    fn insert_raw_item(
        db: &Database,
        receipt_id: i64,
        session_id: i64,
        request_id: &str,
        input_tokens: i64,
        output_tokens: i64,
        cost: f64,
    ) {
        ItemRepository::new(db.clone())
            .insert(&NewItem {
                receipt_id,
                session_id,
                source: "claude".to_string(),
                request_id: request_id.to_string(),
                message_id: None,
                parent_uuid: None,
                is_sidechain: false,
                occurred_at: 1_767_225_600,
                model: "claude-opus-4-5".to_string(),
                service_tier: None,
                input_tokens,
                output_tokens,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                cost,
                metadata: None,
            })
            .expect("insert raw item");
    }

    #[test]
    fn summarize_totals_across_range() {
        let db = open_db();
        let s1 = make_session_with_cwd(&db, "claude", "/summary-a");
        let s2 = make_session_with_cwd(&db, "claude", "/summary-b");

        merge_item(
            &db,
            s1.id,
            make_draft("summary-t1-a", 1_767_225_600, 100, 100, 0.25),
        )
        .unwrap();
        merge_item(
            &db,
            s1.id,
            make_draft("summary-t1-b", 1_767_312_000, 100, 200, 0.25),
        )
        .unwrap();
        merge_item(
            &db,
            s2.id,
            make_draft("summary-t1-c", 1_768_435_200, 100, 100, 0.50),
        )
        .unwrap();
        merge_item(
            &db,
            s2.id,
            make_draft("summary-t1-d", 1_769_817_600, 200, 100, 0.50),
        )
        .unwrap();

        let actual = SummaryRepository::new(db)
            .summarize("2026-01-01", "2026-01-31")
            .unwrap();

        assert!((actual.total_cost - 1.5).abs() < 1e-9);
        assert_eq!(actual.total_tokens, 1000);
        assert_eq!(actual.session_count, 2);
        assert_eq!(actual.receipt_count, 4);
    }

    #[test]
    fn summarize_empty_range_returns_zeroes() {
        let db = open_db();

        let actual = SummaryRepository::new(db)
            .summarize("2026-01-01", "2026-01-31")
            .unwrap();

        assert_eq!(actual.total_cost, 0.0);
        assert_eq!(actual.total_tokens, 0);
        assert_eq!(actual.session_count, 0);
        assert_eq!(actual.receipt_count, 0);
    }

    #[test]
    fn summarize_counts_null_session_receipt_but_not_session() {
        let db = open_db();
        let item_session = make_session_with_cwd(&db, "claude", "/summary-null-item-session");
        let receipt = ReceiptRepository::new(db.clone())
            .upsert_by_cwd_date(None, "/summary-null-receipt", 1_767_225_600)
            .unwrap();
        insert_raw_item(&db, receipt.id, item_session.id, "summary-t3", 10, 15, 0.10);

        let actual = SummaryRepository::new(db)
            .summarize("2026-01-01", "2026-01-31")
            .unwrap();

        assert_eq!(actual.receipt_count, 1);
        assert_eq!(actual.session_count, 0);
        assert_eq!(actual.total_tokens, 25);
        assert!((actual.total_cost - 0.10).abs() < 1e-9);
    }

    #[test]
    fn summarize_excludes_receipts_without_items() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/summary-no-items");
        ReceiptRepository::new(db.clone())
            .upsert_by_cwd_date(Some(session.id), "/summary-no-items", 1_767_225_600)
            .unwrap();

        let actual = SummaryRepository::new(db)
            .summarize("2026-01-01", "2026-01-31")
            .unwrap();

        assert_eq!(actual.receipt_count, 0);
        assert_eq!(actual.session_count, 0);
        assert_eq!(actual.total_tokens, 0);
        assert_eq!(actual.total_cost, 0.0);
    }

    #[test]
    fn summarize_includes_start_and_end_boundaries() {
        let db = open_db();
        let session = make_session_with_cwd(&db, "claude", "/summary-boundary");
        merge_item(
            &db,
            session.id,
            make_draft("summary-t5-start", 1_767_225_600, 1, 2, 0.10),
        )
        .unwrap();
        merge_item(
            &db,
            session.id,
            make_draft("summary-t5-end", 1_769_817_600, 3, 4, 0.20),
        )
        .unwrap();
        merge_item(
            &db,
            session.id,
            make_draft("summary-t5-before", 1_767_139_200, 10, 10, 1.00),
        )
        .unwrap();
        merge_item(
            &db,
            session.id,
            make_draft("summary-t5-after", 1_769_904_000, 10, 10, 1.00),
        )
        .unwrap();

        let actual = SummaryRepository::new(db)
            .summarize("2026-01-01", "2026-01-31")
            .unwrap();

        assert_eq!(actual.receipt_count, 2);
        assert_eq!(actual.session_count, 1);
        assert_eq!(actual.total_tokens, 10);
        assert!((actual.total_cost - 0.30).abs() < 1e-9);
    }
}

// ---------------------------------------------------------------------------
// mod app_settings — C1 + C2
// ---------------------------------------------------------------------------
mod app_settings {
    use super::*;

    // C2: schema version is 4 after all migrations
    #[test]
    fn user_version_is_4() {
        let db = open_db();
        let conn = db.lock();
        let uv: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, 4, "user_version should be 4 after migration 0004");
    }

    // C2: app_settings table schema
    #[test]
    fn table_info_has_correct_columns() {
        let db = open_db();
        let conn = db.lock();

        // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
        let mut stmt = conn.prepare("PRAGMA table_info('app_settings')").unwrap();
        let rows: Vec<(String, String, i32, i32)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(1)?, // name
                    r.get::<_, String>(2)?, // type
                    r.get::<_, i32>(3)?,    // notnull
                    r.get::<_, i32>(5)?,    // pk
                ))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let key_col = rows.iter().find(|(name, ..)| name == "key").unwrap();
        assert_eq!(key_col.1, "TEXT", "key column type must be TEXT");
        assert_eq!(key_col.3, 1, "key must be primary key (pk=1)");

        let value_col = rows.iter().find(|(name, ..)| name == "value").unwrap();
        assert_eq!(value_col.1, "TEXT", "value column type must be TEXT");
        assert_eq!(value_col.2, 1, "value must be NOT NULL (notnull=1)");
        assert_eq!(value_col.3, 0, "value must not be primary key (pk=0)");
    }

    // C2: ON CONFLICT upsert leaves exactly one row
    #[test]
    fn on_conflict_upsert_leaves_one_row() {
        let db = open_db();
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES ('k', 'v')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES ('k', 'v2') \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM app_settings", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1, "upsert must leave exactly one row");
            let val: String = conn
                .query_row("SELECT value FROM app_settings WHERE key = 'k'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(val, "v2", "upserted value must be 'v2'");
        }
    }

    // C1: get absent key → Ok(None)
    #[test]
    fn get_absent_key_returns_none() {
        let db = open_db();
        let repo = AppSettingsRepository::new(db);
        let result = repo.get("missing").unwrap();
        assert_eq!(result, None);
    }

    // C1: set then get returns the stored value
    #[test]
    fn set_then_get_returns_value() {
        let db = open_db();
        let repo = AppSettingsRepository::new(db);
        repo.set("foo", "bar").unwrap();
        let result = repo.get("foo").unwrap();
        assert_eq!(result, Some("bar".to_string()));
    }

    // C1: overwrite with second set wins
    #[test]
    fn double_set_overwrites() {
        let db = open_db();
        let repo = AppSettingsRepository::new(db);
        repo.set("foo", "bar").unwrap();
        repo.set("foo", "baz").unwrap();
        let result = repo.get("foo").unwrap();
        assert_eq!(result, Some("baz".to_string()));
    }

    // C1: independent keys do not interfere
    #[test]
    fn independent_keys_do_not_interfere() {
        let db = open_db();
        let repo = AppSettingsRepository::new(db);
        repo.set("a", "1").unwrap();
        repo.set("b", "2").unwrap();
        assert_eq!(repo.get("a").unwrap(), Some("1".to_string()));
        assert_eq!(repo.get("b").unwrap(), Some("2".to_string()));
    }
}

// ---------------------------------------------------------------------------
// p6-8 — boot catch-up: ReceiptRepository::fetch_pending_since +
// AppSettingsRepository::set_i64_max ("cursors never roll backwards").
// ---------------------------------------------------------------------------
mod catchup {
    use super::*;

    /// Insert a `receipts` row directly so the test can control `id` and
    /// `updated_at` independently of the trigger. Each row also gets a
    /// unique `session_id` and `cwd` to satisfy `UNIQUE(cwd, date)`.
    fn insert_receipt_raw(db: &Database, id: i64, updated_at: i64) {
        let conn = db.lock();
        let n = next_id();
        // Use a unique cwd so the UNIQUE(cwd, date) constraint is satisfied
        // across test rows, and assign a stable date string.
        let cwd = format!("/p6-8-test/{n}");
        let date = "2026-05-19";
        conn.execute(
            "INSERT INTO receipts (id, session_id, cwd, date, created_at, updated_at) \
             VALUES (?1, NULL, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, cwd, date, updated_at, updated_at],
        )
        .expect("raw receipt insert should succeed");
    }

    fn bump_updated_at(db: &Database, id: i64, new_updated_at: i64) {
        let conn = db.lock();
        conn.execute(
            "UPDATE receipts SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![new_updated_at, id],
        )
        .expect("updated_at bump should succeed");
    }

    // M ≤ cap: every new row appears in pending_ids; rows.len() == M (no overflow).
    #[test]
    fn pending_returns_all_when_below_cap() {
        let db = open_db();
        let receipts_repo = ReceiptRepository::new(db.clone());
        // Seed 5 rows the user has already seen.
        for i in 1..=5 {
            insert_receipt_raw(&db, i, 1_000 + i);
        }
        let last_id = 5;
        let last_ts = 1_005;
        // Now insert 3 new rows after the cursor.
        for i in 6..=8 {
            insert_receipt_raw(&db, i, 2_000 + i);
        }

        let rows = receipts_repo
            .fetch_pending_since(last_id, last_ts, 50)
            .unwrap();

        assert_eq!(rows.len(), 3);
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        // ORDER BY id DESC.
        assert_eq!(ids, vec![8, 7, 6]);
    }

    // M > cap: result has cap+1 rows; caller knows overflow occurred.
    #[test]
    fn pending_overflows_when_above_cap() {
        let db = open_db();
        let receipts_repo = ReceiptRepository::new(db.clone());
        let cap = 3usize;
        // Seen baseline.
        insert_receipt_raw(&db, 1, 1_000);
        let last_id = 1;
        let last_ts = 1_000;
        // 5 new rows; cap = 3 ⇒ expect 4 rows returned (cap + 1).
        for i in 2..=6 {
            insert_receipt_raw(&db, i, 2_000 + i);
        }

        let rows = receipts_repo
            .fetch_pending_since(last_id, last_ts, cap)
            .unwrap();
        assert_eq!(rows.len(), cap + 1, "overflow signal: rows.len() > cap");

        // Top `cap` ids in newest-first order.
        let visible: Vec<i64> = rows.iter().take(cap).map(|r| r.id).collect();
        assert_eq!(visible, vec![6, 5, 4]);

        // count_pending_since must reflect the true total beyond the cap.
        let total = receipts_repo
            .count_pending_since(last_id, last_ts)
            .unwrap();
        assert_eq!(total, 5, "true total = 5 new rows since cursor");

        // Caller advances cursors past ALL loaded rows. Even the overflow
        // row's id should be reachable from the loaded set.
        let max_id_loaded = rows.iter().map(|r| r.id).max().unwrap();
        assert_eq!(max_id_loaded, 6);
    }

    // Updated_at branch: an old row whose updated_at gets bumped past the
    // cursor must appear in pending on next boot, even though id ≤ cursor.
    // This simulates a writer reingest.
    #[test]
    fn pending_includes_old_row_with_bumped_updated_at() {
        let db = open_db();
        let receipts_repo = ReceiptRepository::new(db.clone());
        insert_receipt_raw(&db, 1, 1_000);
        insert_receipt_raw(&db, 2, 1_001);
        let last_id = 2;
        let last_ts = 1_001;

        // No newer rows, but bump id=1's updated_at past last_ts.
        bump_updated_at(&db, 1, 5_000);

        let rows = receipts_repo
            .fetch_pending_since(last_id, last_ts, 50)
            .unwrap();
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![1], "the bumped-updated_at old row must surface");
    }

    // Race-style smoke: many interleaved set_i64_max calls from two
    // simulated writers (watcher batch + focus) must never lower the
    // stored cursor. Single-statement MAX-merge in app_settings gives
    // this guarantee without explicit locking.
    #[test]
    fn cursor_never_rolls_backwards_under_interleaved_writers() {
        use std::sync::Arc;
        use std::thread;

        let db = Arc::new(open_db());
        let settings = AppSettingsRepository::new((*db).clone());
        settings.set_i64(LAST_SEEN_RECEIPT_ID, 0).unwrap();

        let mut handles = Vec::new();
        // 2 threads, each writing a strictly-increasing sequence interleaved
        // with smaller candidates that must be ignored.
        for thread_idx in 0..2 {
            let db = Arc::clone(&db);
            handles.push(thread::spawn(move || {
                let r = AppSettingsRepository::new((*db).clone());
                let base = (thread_idx + 1) * 10;
                for i in 0..200 {
                    // Sometimes write a small value (must be rejected by MAX).
                    let candidate = if i % 3 == 0 { 1 } else { base + i };
                    r.set_i64_max(LAST_SEEN_RECEIPT_ID, candidate).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let final_value = settings.get_i64(LAST_SEEN_RECEIPT_ID).unwrap().unwrap();
        // Highest possible candidate written by either thread: max(10+199, 20+199) = 219.
        assert_eq!(
            final_value, 219,
            "MAX-merge must converge to the largest candidate seen"
        );
        assert!(
            final_value > 0,
            "cursor must never roll back below earlier highs"
        );
    }

    // Empty new rows ⇒ pending_ids empty, count = 0.
    #[test]
    fn pending_empty_when_nothing_new() {
        let db = open_db();
        let receipts_repo = ReceiptRepository::new(db.clone());
        insert_receipt_raw(&db, 1, 1_000);
        let last_id = 1;
        let last_ts = 1_000;

        let rows = receipts_repo
            .fetch_pending_since(last_id, last_ts, 50)
            .unwrap();
        assert!(rows.is_empty());
        let count = receipts_repo
            .count_pending_since(last_id, last_ts)
            .unwrap();
        assert_eq!(count, 0);
    }

    // max_id_and_updated_at on empty table returns (0, 0).
    #[test]
    fn max_id_and_updated_at_empty_table_returns_zero() {
        let db = open_db();
        let repo = ReceiptRepository::new(db);
        let (max_id, max_ts) = repo.max_id_and_updated_at().unwrap();
        assert_eq!(max_id, 0);
        assert_eq!(max_ts, 0);
    }

    // First-launch seeding constants are stable and exported.
    #[test]
    fn cursor_constants_are_stable() {
        assert_eq!(LAST_SEEN_RECEIPT_ID, "last_seen_receipt_id");
        assert_eq!(LAST_SEEN_MAX_UPDATED_AT, "last_seen_max_updated_at");
    }
}
