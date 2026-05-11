/// E2E test: `tallytape-writer __worker` reads a HookPayload from stdin,
/// parses the referenced transcript, and writes rows to the SQLite DB.
///
/// DB path is controlled by setting HOME to a temp dir so `directories` resolves
/// to `$HOME/Library/Application Support/tallytape/tallytape.sqlite` (macOS) or
/// `$HOME/.local/share/tallytape/tallytape.sqlite` (Linux).
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tempfile::TempDir;

/// Build the DB path that `tallytape-core::db_path()` will produce given HOME.
///
/// On macOS: `$HOME/Library/Application Support/tallytape/tallytape.sqlite`
/// On Linux: `$HOME/.local/share/tallytape/tallytape.sqlite`
fn expected_db_path(home: &TempDir) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.path()
            .join("Library")
            .join("Application Support")
            .join("tallytape")
            .join("tallytape.sqlite")
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.path()
            .join(".local")
            .join("share")
            .join("tallytape")
            .join("tallytape.sqlite")
    }
}

/// JSONL transcript line with a known model so cost can be calculated.
const ASSISTANT_LINE: &str = concat!(
    r#"{"type":"assistant","requestId":"req-worker-1","uuid":"u1","timestamp":"#,
    r#""2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","#,
    r#""usage":{"input_tokens":100,"output_tokens":200}}}"#
);

/// Write a minimal Claude session file and transcript fixture under a fake HOME.
///
/// Returns the session_id and cwd used.
fn setup_fixture(home: &TempDir) -> (String, String) {
    let session_id = "worker-test-session-001".to_string();
    let cwd = "/tmp/worker-test-cwd".to_string();

    // Write session JSON under $HOME/.claude/sessions/
    let sessions_dir = home.path().join(".claude").join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    let session_json = format!(
        r#"{{"sessionId":"{session_id}","cwd":"{cwd}","startedAt":1700000000000,"updatedAt":1700000000000}}"#
    );
    std::fs::write(
        sessions_dir.join(format!("{session_id}.json")),
        &session_json,
    )
    .unwrap();

    // Write transcript at $HOME/.claude/projects/<cwd-slug>/<session_id>.jsonl
    let slug: String = cwd
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let projects_dir = home.path().join(".claude").join("projects").join(&slug);
    std::fs::create_dir_all(&projects_dir).unwrap();
    std::fs::write(
        projects_dir.join(format!("{session_id}.jsonl")),
        ASSISTANT_LINE,
    )
    .unwrap();

    (session_id, cwd)
}

fn make_assistant_jsonl_line(request_id: &str) -> String {
    format!(
        r#"{{"type":"assistant","requestId":"{request_id}","uuid":"u-{request_id}","timestamp":"2025-11-15T10:23:45.000Z","message":{{"id":"msg-{request_id}","model":"claude-opus-4-7","usage":{{"input_tokens":5,"output_tokens":10}}}}}}"#
    )
}

/// Write subagent transcript at `<home>/.claude/projects/<slug>/<sid>/subagents/<file_name>`.
fn write_subagent_transcript(home: &TempDir, slug: &str, sid: &str, file_name: &str, contents: &str) {
    let subagents_dir = home
        .path()
        .join(".claude")
        .join("projects")
        .join(slug)
        .join(sid)
        .join("subagents");
    std::fs::create_dir_all(&subagents_dir).unwrap();
    std::fs::write(subagents_dir.join(file_name), contents).unwrap();
}

/// Count rows in a table from the DB at `db_path`.
///
/// The helper is kept (vs. inline queries) because multiple tests reuse it.
/// Table name is validated against an allowlist to avoid accidental SQL injection
/// from a typo in a test — the format! is purely for convenience, not user input.
fn count_rows(db_path: &PathBuf, table: &str) -> i64 {
    assert!(
        matches!(table, "sessions" | "items" | "receipts"),
        "count_rows: unknown table '{table}'"
    );
    use rusqlite::Connection;
    let conn = Connection::open(db_path).expect("open DB for counting");
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap_or(0)
}

#[test]
fn worker_completes_ingest_and_writes_db_rows() {
    let home = TempDir::new().expect("tempdir for fake HOME");
    let (session_id, cwd) = setup_fixture(&home);

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .arg("__worker")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn __worker");

    // Write payload then close stdin.
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("worker wait failed");
    assert!(status.success(), "worker must exit 0, got: {status}");

    // Verify DB was created and has rows.
    let db_path = expected_db_path(&home);
    assert!(
        db_path.exists(),
        "DB file must exist at {}",
        db_path.display()
    );

    let sessions = count_rows(&db_path, "sessions");
    let items = count_rows(&db_path, "items");

    assert_eq!(sessions, 1, "expected 1 session row, got {sessions}");
    assert_eq!(
        items, 1,
        "expected 1 item row (from ASSISTANT_LINE), got {items}"
    );
}

/// TC-A integration: parent + 2 subagent files all share the same DB session_id.
#[test]
fn worker_ingests_subagent_transcripts_under_parent_session() {
    let home = TempDir::new().expect("tempdir for fake HOME");
    let (session_id, cwd) = setup_fixture(&home);

    // Compute slug (same logic as tallytape_core::slugify_cwd).
    let slug: String = cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();

    // Add two subagent transcript files.
    write_subagent_transcript(
        &home,
        &slug,
        &session_id,
        "agent-1.jsonl",
        &make_assistant_jsonl_line("req-sub-1"),
    );
    write_subagent_transcript(
        &home,
        &slug,
        &session_id,
        "agent-2.jsonl",
        &make_assistant_jsonl_line("req-sub-2"),
    );

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .arg("__worker")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn __worker");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("worker wait failed");
    assert!(status.success(), "worker must exit 0, got: {status}");

    let db_path = expected_db_path(&home);
    assert!(db_path.exists(), "DB must exist at {}", db_path.display());

    // 3 items total: 1 parent + 2 subagents.
    let items = count_rows(&db_path, "items");
    assert_eq!(items, 3, "expected 3 item rows (parent + 2 subagents), got {items}");

    // All 3 items must share the same session_id row.
    use rusqlite::Connection;
    let conn = Connection::open(&db_path).expect("open DB");
    let distinct_session_ids: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT session_id) FROM items",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        distinct_session_ids, 1,
        "all items must share the parent's DB session_id"
    );
}

#[test]
fn worker_exits_zero_and_writes_session_on_missing_transcript() {
    // If transcript is missing, session_loader degrades gracefully (no items,
    // session row still written). Worker exits 0 in this case.
    let home = TempDir::new().expect("tempdir for fake HOME");

    // Only create the session file, not the transcript.
    let session_id = "worker-notranscript-001";
    let cwd = "/tmp/worker-notranscript";
    let sessions_dir = home.path().join(".claude").join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();
    let session_json = format!(
        r#"{{"sessionId":"{session_id}","cwd":"{cwd}","startedAt":1700000000000,"updatedAt":1700000000000}}"#
    );
    std::fs::write(
        sessions_dir.join(format!("{session_id}.json")),
        &session_json,
    )
    .unwrap();

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");
    let mut child = Command::new(bin)
        .arg("__worker")
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn __worker");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("worker wait failed");
    // session_loader degrades gracefully → persist still succeeds → exit 0
    assert!(
        status.success(),
        "worker must exit 0 even with missing transcript"
    );

    let db_path = expected_db_path(&home);
    assert!(
        db_path.exists(),
        "DB must exist even with missing transcript"
    );
    let sessions = count_rows(&db_path, "sessions");
    assert_eq!(
        sessions, 1,
        "session row must be written even without transcript"
    );
}
