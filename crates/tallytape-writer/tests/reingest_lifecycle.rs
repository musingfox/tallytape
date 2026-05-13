/// Integration test: repeated `persist_with_db` calls repair `started_at` via MIN
/// and advance `ended_at` via MAX, converging on correct session lifecycle timestamps.
use rusqlite::Connection;
use tallytape_core::{Database, NewSession, ParsedItem, TokenStats};
use tallytape_writer::{persist_with_db, SessionResult};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_item(request_id: &str, occurred_at: i64) -> ParsedItem {
    ParsedItem {
        request_id: request_id.to_string(),
        message_id: Some(format!("msg-{request_id}")),
        parent_uuid: None,
        is_sidechain: false,
        occurred_at,
        model: "claude-opus-4-7".to_string(),
        service_tier: None,
        input_tokens: 100,
        output_tokens: 50,
        cache_creation_tokens: Some(0),
        cache_read_tokens: Some(0),
        metadata: None,
    }
}

fn query_session_lifecycle(
    conn: &Connection,
    source: &str,
    external_id: &str,
) -> (i64, Option<i64>) {
    conn.query_row(
        "SELECT started_at, ended_at FROM sessions WHERE source = ?1 AND external_id = ?2",
        rusqlite::params![source, external_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .expect("session row must exist")
}

fn count_items(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
        .expect("COUNT query should succeed")
}

// ---------------------------------------------------------------------------
// The integration test
// ---------------------------------------------------------------------------

#[test]
fn reingest_lifecycle_repairs_started_advances_ended() {
    let tmp = tempdir().expect("tempdir should be created");
    let db_path = tmp.path().join("reingest.db");
    let db = Database::open(&db_path).expect("Database::open should succeed");

    // --- Persist #1: wrong early started_at (simulating a fallback), two items ---
    let result1 = SessionResult {
        session: NewSession {
            source: "claude-code".to_string(),
            external_id: "reingest-session".to_string(),
            cwd: None,
            started_at: 1_700_000_900, // wrong early fallback
            ended_at: Some(1_700_001_000),
            metadata: None,
        },
        items: vec![
            make_item("r1", 1_700_000_500),
            make_item("r2", 1_700_001_000),
        ],
        stats: TokenStats::default(),
    };

    let out1 = persist_with_db(&db, result1).expect("first persist should succeed");
    assert_eq!(out1.inserted, 2, "persist 1: expected 2 items inserted");
    assert_eq!(
        out1.skipped_duplicates, 0,
        "persist 1: expected 0 duplicates"
    );

    // --- Persist #2: corrected started_at, advanced ended_at, same r1/r2 + new r3 ---
    let result2 = SessionResult {
        session: NewSession {
            source: "claude-code".to_string(),
            external_id: "reingest-session".to_string(),
            cwd: None,
            started_at: 1_700_000_500, // corrected
            ended_at: Some(1_700_002_000),
            metadata: None,
        },
        items: vec![
            make_item("r1", 1_700_000_500), // duplicate
            make_item("r2", 1_700_001_000), // duplicate
            make_item("r3", 1_700_002_000), // new
        ],
        stats: TokenStats::default(),
    };

    let out2 = persist_with_db(&db, result2).expect("second persist should succeed");
    assert_eq!(out2.inserted, 1, "persist 2: expected 1 new item (r3)");
    assert_eq!(
        out2.skipped_duplicates, 2,
        "persist 2: expected 2 duplicates (r1, r2)"
    );

    // --- Verify session lifecycle via raw rusqlite ---
    let conn = Connection::open(&db_path).expect("raw rusqlite connection should open");

    let (started_at, ended_at) = query_session_lifecycle(&conn, "claude-code", "reingest-session");

    assert_eq!(
        started_at, 1_700_000_500,
        "started_at should have been repaired to the corrected earlier value"
    );
    assert_eq!(
        ended_at,
        Some(1_700_002_000),
        "ended_at should have advanced to the later value"
    );

    // --- Verify total item count ---
    let item_count = count_items(&conn);
    assert_eq!(
        item_count, 3,
        "exactly 3 distinct items must exist (r1, r2, r3)"
    );
}
