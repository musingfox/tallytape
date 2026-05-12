/// Frozen-fixture regression test for TokenStats, per-row cost, aggregate cost,
/// in-memory dedup, and DB UNIQUE-constraint dedup.
///
/// All assertions are hermetic: a fresh temp-dir SQLite DB is used — no HOME-
/// based path resolution occurs.
use std::path::Path;

use rusqlite::Connection;
use tallytape_core::{Database, NewSession, TokenStats, parse_transcript_file_with_stats};
use tallytape_writer::{persist_with_db, SessionResult};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Goldens
// ---------------------------------------------------------------------------

const EXPECTED_INPUT_TOKENS: i64 = 3500;
const EXPECTED_OUTPUT_TOKENS: i64 = 1600;
const EXPECTED_CACHE_CREATION_TOKENS: i64 = 2100;
const EXPECTED_CACHE_READ_TOKENS: i64 = 5200;
const EXPECTED_ITEMS: usize = 4;

const EXPECTED_COST_REQ_A: f64 = 0.032;
const EXPECTED_COST_REQ_B: f64 = 0.0183;
const EXPECTED_COST_REQ_C: f64 = 0.002145;
const EXPECTED_COST_TOTAL: f64 = 0.052445;

const COST_TOLERANCE: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Helper: query cost by request_id from a raw rusqlite Connection
// ---------------------------------------------------------------------------

fn cost_for_request(conn: &Connection, request_id: &str) -> f64 {
    conn.query_row(
        "SELECT cost FROM items WHERE request_id = ?1",
        rusqlite::params![request_id],
        |r| r.get(0),
    )
    .expect("expected row for request_id")
}

fn sum_cost_for_session(conn: &Connection, session_id: i64) -> f64 {
    conn.query_row(
        "SELECT SUM(cost) FROM items WHERE session_id = ?1",
        rusqlite::params![session_id],
        |r| r.get(0),
    )
    .expect("SUM query should succeed")
}

fn count_items(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
        .expect("COUNT query should succeed")
}

// ---------------------------------------------------------------------------
// The regression test
// ---------------------------------------------------------------------------

#[test]
fn cost_fixture_regression() {
    // Resolve fixture path relative to the crate manifest directory.
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/regression_session.jsonl");

    // --- TC1: stats dedup ---
    let parsed =
        parse_transcript_file_with_stats(&fixture_path).expect("fixture must parse successfully");

    assert_eq!(
        parsed.stats,
        TokenStats {
            input_tokens: EXPECTED_INPUT_TOKENS,
            output_tokens: EXPECTED_OUTPUT_TOKENS,
            cache_creation_tokens: EXPECTED_CACHE_CREATION_TOKENS,
            cache_read_tokens: EXPECTED_CACHE_READ_TOKENS,
        },
        "TC1: stats mismatch (in-memory dedup should deduplicate req-A duplicate)"
    );
    assert_eq!(
        parsed.items.len(),
        EXPECTED_ITEMS,
        "TC1: expected 4 ParsedItems (line 5 is user type, skipped; line 2 is dup of line 1 → both emitted as ParsedItems)"
    );

    // Build SessionResult for first persist.
    let session_result = SessionResult {
        session: NewSession {
            source: "claude-code".to_string(),
            external_id: "regression-session".to_string(),
            cwd: None,
            started_at: 1_735_689_600,
            ended_at: None,
            metadata: None,
        },
        items: parsed.items.clone(),
        stats: parsed.stats,
    };

    // Open hermetic temp-dir DB.
    let tmp = tempdir().expect("tempdir creation should succeed");
    let db_path = tmp.path().join("regression.db");
    let db = Database::open(&db_path).expect("Database::open should succeed");

    // --- TC4: first persist — 3 inserted, 1 dup (req-A collision on UNIQUE(source,request_id)) ---
    let outcome = persist_with_db(&db, session_result.clone())
        .expect("first persist_with_db should succeed");

    assert_eq!(
        outcome.inserted, 3,
        "TC4: expected 3 inserted (req-A, req-B, req-C)"
    );
    assert_eq!(
        outcome.skipped_duplicates, 1,
        "TC4: expected 1 skipped duplicate (second req-A hits UNIQUE constraint)"
    );

    // --- TC2 & TC3: per-row costs and aggregate ---
    {
        let conn = Connection::open(&db_path).expect("raw rusqlite connection should open");

        let cost_a = cost_for_request(&conn, "req-A");
        assert!(
            (cost_a - EXPECTED_COST_REQ_A).abs() < COST_TOLERANCE,
            "TC2: req-A cost mismatch: expected {EXPECTED_COST_REQ_A}, got {cost_a}"
        );

        let cost_b = cost_for_request(&conn, "req-B");
        assert!(
            (cost_b - EXPECTED_COST_REQ_B).abs() < COST_TOLERANCE,
            "TC2: req-B cost mismatch: expected {EXPECTED_COST_REQ_B}, got {cost_b}"
        );

        let cost_c = cost_for_request(&conn, "req-C");
        assert!(
            (cost_c - EXPECTED_COST_REQ_C).abs() < COST_TOLERANCE,
            "TC2: req-C cost mismatch: expected {EXPECTED_COST_REQ_C}, got {cost_c}"
        );

        let total = sum_cost_for_session(&conn, outcome.session_id);
        assert!(
            (total - EXPECTED_COST_TOTAL).abs() < COST_TOLERANCE,
            "TC3: aggregate cost mismatch: expected {EXPECTED_COST_TOTAL}, got {total}"
        );
    }

    // --- TC5: second persist on same SessionResult → all 4 items skipped ---
    let outcome2 = persist_with_db(&db, session_result).expect("second persist_with_db should succeed");

    assert_eq!(
        outcome2.inserted, 0,
        "TC5: expected 0 inserted on rerun"
    );
    assert_eq!(
        outcome2.skipped_duplicates, 4,
        "TC5: expected 4 skipped duplicates on rerun (all 4 items hit UNIQUE constraint)"
    );

    {
        let conn = Connection::open(&db_path).expect("raw rusqlite connection should open");
        let item_count = count_items(&conn);
        assert_eq!(
            item_count, 3,
            "TC5: DB must still contain exactly 3 rows after rerun"
        );
    }
}
