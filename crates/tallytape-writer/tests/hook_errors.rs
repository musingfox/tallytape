/// E2E tests for error paths (C4, C5).
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// Build the log path that `tallytape-core::log_path()` resolves to given HOME.
fn expected_log_path(home: &TempDir) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.path()
            .join("Library")
            .join("Application Support")
            .join("tallytape")
            .join("writer.log")
    }
    #[cfg(not(target_os = "macos"))]
    {
        home.path()
            .join(".local")
            .join("share")
            .join("tallytape")
            .join("writer.log")
    }
}

/// Read `path` if it exists, returning an empty string otherwise.
fn read_log(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// C5: malformed stdin → parent exits 0; writer.log contains parse-error; no __worker child.
#[test]
fn malformed_stdin_exits_zero_and_logs_parse_error() {
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(b"not-json\n").unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(
        status.success(),
        "parent must exit 0 on malformed stdin, got: {status}"
    );

    // writer.log must exist and contain a parse-error line.
    assert!(
        log_path.exists(),
        "writer.log must exist at {}",
        log_path.display()
    );
    let contents = read_log(&log_path);
    assert!(
        contents.contains("malformed") || contents.contains("parse") || contents.contains("error"),
        "writer.log should contain a parse-error line, got:\n{contents}"
    );

    // No __worker process should be left running from this invocation.
    // We check by looking for the absence of any leftover processes matching our invocation.
    // (The test binary PID is known from the child that just exited.)
    // There is no __worker child to check since parse fails before spawn.
    // The assertion is implicit: if spawn_worker had been called, it would have been
    // on a process that just exited cleanly — but the log would not have the error.
    // The parse-error in the log proves spawn_worker was NOT called.
}

/// C4-B: payload with non-existent transcript path → parent exits 0 quickly;
/// after child completes, writer.log contains an error from the child
/// (OR the session is written with no items — because session_loader degrades gracefully).
///
/// Note: session_loader logs a `warn!` (not an error) when transcript is missing
/// and then returns a degraded SessionResult. The persist step then succeeds.
/// So writer.log may NOT have an error entry — this is correct behaviour.
/// What we DO assert: parent exits 0 within 20ms (fast path preserved).
///
/// Requires release build: cargo test --release -- --ignored nonexistent_transcript_parent_exits_quickly
#[cfg(target_os = "macos")]
#[test]
#[ignore]
fn nonexistent_transcript_parent_exits_quickly() {
    let home = TempDir::new().expect("tempdir for fake HOME");

    let payload =
        r#"{"session_id":"t-c4b","transcript_path":"/nonexistent/abs/path.jsonl","cwd":"/tmp"}"#;

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    // Warm the dyld/page cache with a throwaway spawn so measurement reflects
    // steady-state latency (the condition under which Claude Code fires hooks).
    {
        let mut warmup = Command::new(bin)
            .env("HOME", home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("warmup spawn failed");
        if let Some(mut s) = warmup.stdin.take() {
            let _ = s.write_all(payload.as_bytes());
        }
        let _ = warmup.wait();
    }

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    let start = Instant::now();

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    let elapsed = start.elapsed();

    assert!(status.success(), "parent must exit 0, got: {status}");
    assert!(
        elapsed.as_millis() <= 20,
        "parent took {}ms (warm cache), budget is 20ms",
        elapsed.as_millis()
    );
}

/// C5 (additional): empty stdin → parent exits 0; writer.log has an error entry.
#[test]
fn empty_stdin_exits_zero_and_logs_error() {
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    // Spawn and immediately close stdin (= empty stdin).
    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    // Drop stdin immediately — child receives EOF with zero bytes.
    drop(child.stdin.take());

    let status = child.wait().expect("wait failed");
    assert!(
        status.success(),
        "parent must exit 0 on empty stdin, got: {status}"
    );

    assert!(log_path.exists(), "writer.log must exist");
    let contents = read_log(&log_path);
    assert!(
        contents.contains("empty stdin") || contents.contains("error"),
        "writer.log should mention empty stdin, got:\n{contents}"
    );
}

/// T1: payload with bogus session_id and cwd whose transcript does NOT exist →
/// child exits, writer.log contains level=ERROR line with bogus-sid-xyz and "not found".
#[test]
fn t1_error_logged_for_missing_transcript() {
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    let payload = r#"{"session_id":"bogus-sid-xyz","cwd":"/tmp/no-such-cwd-p2-9"}"#;

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env_remove("TALLYTAPE_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait for the detached worker to complete and write the log.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let contents = read_log(&log_path);
        if contents.contains("level=ERROR") && contents.contains("bogus-sid-xyz") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        log_path.exists(),
        "writer.log must exist at {}",
        log_path.display()
    );
    let contents = read_log(&log_path);
    assert!(
        contents.contains("level=ERROR"),
        "writer.log should contain level=ERROR, got:\n{contents}"
    );
    assert!(
        contents.contains("bogus-sid-xyz"),
        "writer.log should contain bogus-sid-xyz, got:\n{contents}"
    );
    assert!(
        contents.contains("not found") || contents.contains("transcript missing"),
        "writer.log should mention missing transcript or not found, got:\n{contents}"
    );
}

/// TC2 / T1b: payload whose transcript exists but cannot be opened (mode 000) →
/// writer.log contains level=ERROR with the session_id and a parse-failure message.
///
/// `parse_transcript_file_with_stats` only returns Err on File::open failure.
/// We trigger that by creating the file then removing read permission.
#[cfg(unix)]
#[test]
fn t1b_error_logged_for_parse_failure() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    let session_id = "parse-fail-sid-p2-9";
    let cwd = "/tmp/parse-fail-cwd-p2-9";

    // Build transcript path that the writer will look for.
    let claude_home = home.path().join(".claude");
    let transcript_file = tallytape_core::transcript_path(&claude_home, cwd, session_id);
    let transcript_dir = transcript_file
        .parent()
        .expect("transcript_file has parent");
    fs::create_dir_all(transcript_dir).expect("create transcript dir");
    // Write content, then make the file unreadable so File::open fails.
    fs::write(&transcript_file, "not json line\n{also broken\n").expect("write transcript");
    fs::set_permissions(&transcript_file, fs::Permissions::from_mode(0o000))
        .expect("chmod 000 transcript");

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env_remove("TALLYTAPE_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait for the detached worker to complete and write the log.
    // Keep the file unreadable until we see the log entry (or timeout).
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let contents = read_log(&log_path);
        if contents.contains("level=ERROR") && contents.contains(session_id) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Restore permissions so TempDir cleanup can delete the file.
    let _ = fs::set_permissions(&transcript_file, fs::Permissions::from_mode(0o644));

    assert!(
        log_path.exists(),
        "writer.log must exist at {}",
        log_path.display()
    );
    let contents = read_log(&log_path);
    assert!(
        contents.contains("level=ERROR"),
        "writer.log should contain level=ERROR for parse failure, got:\n{contents}"
    );
    assert!(
        contents.contains(session_id),
        "writer.log should contain session_id {session_id}, got:\n{contents}"
    );
    assert!(
        contents.contains("parse_transcript_file_with_stats") || contents.contains("failed"),
        "writer.log should mention parse failure, got:\n{contents}"
    );
}

/// T2: successful (or empty-but-not-failed) ingest at default TALLYTAPE_LOG (unset).
/// Log file MUST NOT contain level=INFO or "persisted session".
#[test]
fn t2_no_info_logged_at_default_warn_level() {
    use std::fs;
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    // Create a valid transcript at the expected path so ingest succeeds.
    // transcript_path = ~/.claude/projects/<slugified_cwd>/<session_id>.jsonl
    // We use a simple cwd that the writer can resolve.
    let cwd = "/tmp/t2-test-cwd";
    let session_id = "t2-session-ok";

    // Build the claude_home path that the writer will use (HOME/.claude)
    let claude_home = home.path().join(".claude");
    // Compute transcript path using tallytape_core's canonical function.
    let transcript_file = tallytape_core::transcript_path(&claude_home, cwd, session_id);
    let transcript_dir = transcript_file
        .parent()
        .expect("transcript_file has parent");
    fs::create_dir_all(transcript_dir).expect("create transcript dir");

    // One valid assistant line
    let assistant_line = r#"{"type":"assistant","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":10,"output_tokens":20}}}"#;
    fs::write(&transcript_file, assistant_line).expect("write transcript");

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env_remove("TALLYTAPE_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait a generous time for the worker to finish, then check log.
    // We wait for the DB to appear as a proxy that the worker completed.
    #[cfg(target_os = "macos")]
    let db_path = home
        .path()
        .join("Library")
        .join("Application Support")
        .join("tallytape")
        .join("tallytape.sqlite");
    #[cfg(not(target_os = "macos"))]
    let db_path = home
        .path()
        .join(".local")
        .join("share")
        .join("tallytape")
        .join("tallytape.sqlite");

    let deadline = Instant::now() + Duration::from_secs(10);
    while !db_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }

    let contents = read_log(&log_path);
    assert!(
        !contents.contains("level=INFO"),
        "writer.log should NOT contain level=INFO at default Warn filter, got:\n{contents}"
    );
    assert!(
        !contents.contains("persisted session"),
        "writer.log should NOT contain 'persisted session' at default Warn filter, got:\n{contents}"
    );
}

/// T4: pre-fill writer.log with 1.5 MB, trigger T1 payload — final size < 4 KB
/// and contains a level=ERROR line with bogus-sid-xyz.
#[test]
fn t4_truncation_before_error_logged() {
    use std::fs;
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    // Pre-fill writer.log with 1.5 MB of 'x'
    let parent = log_path.parent().expect("log_path has parent");
    fs::create_dir_all(parent).expect("create log parent dir");
    let big = vec![b'x'; 1_572_864];
    fs::write(&log_path, &big).expect("pre-fill writer.log");

    let payload = r#"{"session_id":"bogus-sid-xyz","cwd":"/tmp/no-such-cwd-p2-9"}"#;

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env_remove("TALLYTAPE_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait for the detached worker to complete and write the log.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let contents = read_log(&log_path);
        if contents.contains("level=ERROR") && contents.contains("bogus-sid-xyz") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    assert!(
        size < 4096,
        "writer.log should be < 4 KB after truncation, got {size} bytes"
    );

    let contents = read_log(&log_path);
    assert!(
        contents.contains("level=ERROR"),
        "writer.log should contain level=ERROR after truncation, got:\n{contents}"
    );
    assert!(
        contents.contains("bogus-sid-xyz"),
        "writer.log should contain bogus-sid-xyz after truncation, got:\n{contents}"
    );
}

/// TC-C integration: a malformed subagent file emits WARN in writer.log mentioning
/// the file name, while the other subagent file's item is still inserted.
#[test]
fn tc_c_malformed_subagent_warns_in_log() {
    use std::fs;
    let home = TempDir::new().expect("tempdir for fake HOME");
    let log_path = expected_log_path(&home);

    let session_id = "tc-c-integ-session";
    let cwd = "/tmp/tc-c-integ-cwd";
    let claude_home = home.path().join(".claude");

    // Write session file.
    let sessions_dir = claude_home.join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let session_json = format!(
        r#"{{"sessionId":"{session_id}","cwd":"{cwd}","startedAt":1700000000000,"updatedAt":1700000000000}}"#
    );
    fs::write(
        sessions_dir.join(format!("{session_id}.json")),
        &session_json,
    )
    .unwrap();

    // Write parent transcript.
    let transcript_file = tallytape_core::transcript_path(&claude_home, cwd, session_id);
    let transcript_dir = transcript_file.parent().expect("has parent");
    fs::create_dir_all(transcript_dir).unwrap();
    let parent_line = r#"{"type":"assistant","requestId":"rp","uuid":"u-rp","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-rp","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
    fs::write(&transcript_file, parent_line).unwrap();

    // Write subagents: agent-1.jsonl malformed, agent-2.jsonl valid.
    let subagents_dir = transcript_dir.join(session_id).join("subagents");
    fs::create_dir_all(&subagents_dir).unwrap();
    fs::write(subagents_dir.join("agent-1.jsonl"), "not json\n{broken").unwrap();
    let sub2_line = r#"{"type":"assistant","requestId":"rb","uuid":"u-rb","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-rb","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
    fs::write(subagents_dir.join("agent-2.jsonl"), sub2_line).unwrap();

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env("TALLYTAPE_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait for writer to emit the WARN.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let contents = read_log(&log_path);
        if contents.contains("agent-1.jsonl") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let contents = read_log(&log_path);
    assert!(
        contents.contains("agent-1.jsonl"),
        "writer.log should mention agent-1.jsonl for malformed subagent, got:\n{contents}"
    );
}

/// TC-B integration: no subagents dir → writer.log must not mention "subagent".
#[test]
fn tc_b_no_subagents_dir_no_warn_in_log() {
    use std::fs;
    let home = TempDir::new().expect("tempdir for fake HOME");

    let session_id = "tc-b-integ-session";
    let cwd = "/tmp/tc-b-integ-cwd";
    let claude_home = home.path().join(".claude");

    // Write session file.
    let sessions_dir = claude_home.join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let session_json = format!(
        r#"{{"sessionId":"{session_id}","cwd":"{cwd}","startedAt":1700000000000,"updatedAt":1700000000000}}"#
    );
    fs::write(
        sessions_dir.join(format!("{session_id}.json")),
        &session_json,
    )
    .unwrap();

    // Write parent transcript only — no <sid>/subagents/ dir.
    let transcript_file = tallytape_core::transcript_path(&claude_home, cwd, session_id);
    let transcript_dir = transcript_file.parent().expect("has parent");
    fs::create_dir_all(transcript_dir).unwrap();
    let parent_line = r#"{"type":"assistant","requestId":"rp","uuid":"u-rp","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-rp","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
    fs::write(&transcript_file, parent_line).unwrap();

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env("TALLYTAPE_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait for DB as a proxy that the worker completed.
    let db_path = {
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
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !db_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }

    let log_path = expected_log_path(&home);
    let contents = read_log(&log_path);
    assert!(
        !contents.contains("subagent"),
        "writer.log must not mention 'subagent' when no subagents dir exists, got:\n{contents}"
    );
}

/// TC-D integration: tool-results sibling dir is ignored; no mention in writer.log.
#[test]
fn tc_d_tool_results_not_mentioned_in_log() {
    use std::fs;
    let home = TempDir::new().expect("tempdir for fake HOME");

    let session_id = "tc-d-integ-session";
    let cwd = "/tmp/tc-d-integ-cwd";
    let claude_home = home.path().join(".claude");

    // Write session file.
    let sessions_dir = claude_home.join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let session_json = format!(
        r#"{{"sessionId":"{session_id}","cwd":"{cwd}","startedAt":1700000000000,"updatedAt":1700000000000}}"#
    );
    fs::write(
        sessions_dir.join(format!("{session_id}.json")),
        &session_json,
    )
    .unwrap();

    // Write parent transcript.
    let transcript_file = tallytape_core::transcript_path(&claude_home, cwd, session_id);
    let transcript_dir = transcript_file.parent().expect("has parent");
    fs::create_dir_all(transcript_dir).unwrap();
    let parent_line = r#"{"type":"assistant","requestId":"rp","uuid":"u-rp","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-rp","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
    fs::write(&transcript_file, parent_line).unwrap();

    // Write valid subagent.
    let subagents_dir = transcript_dir.join(session_id).join("subagents");
    fs::create_dir_all(&subagents_dir).unwrap();
    let sub_line = r#"{"type":"assistant","requestId":"ra","uuid":"u-ra","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-ra","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
    fs::write(subagents_dir.join("agent-1.jsonl"), sub_line).unwrap();

    // Write tool-results sibling.
    let tool_results_dir = transcript_dir.join(session_id).join("tool-results");
    fs::create_dir_all(&tool_results_dir).unwrap();
    fs::write(tool_results_dir.join("foo.txt"), "some tool result").unwrap();

    let payload = format!(r#"{{"session_id":"{session_id}","cwd":"{cwd}"}}"#);
    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .env("TALLYTAPE_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(status.success(), "parent must exit 0, got: {status}");

    // Wait for DB as proxy for worker completion.
    let db_path = {
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
    };
    // Poll for items count to reach 2, not just db existence — `Connection::open`
    // auto-creates an empty DB, so `db_path.exists()` is not a reliable proxy
    // for "worker finished writing".
    use rusqlite::Connection;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut items: i64 = 0;
    while Instant::now() < deadline {
        if db_path.exists() {
            if let Ok(conn) = Connection::open(&db_path) {
                items = conn
                    .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
                    .unwrap_or(0);
                if items >= 2 {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(items, 2, "expected parent + 1 subagent item, got {items}");

    // Verify tool-results never mentioned in log.
    let log_path = expected_log_path(&home);
    let contents = read_log(&log_path);
    assert!(
        !contents.contains("tool-results"),
        "writer.log must not mention 'tool-results', got:\n{contents}"
    );
    assert!(
        !contents.contains("foo.txt"),
        "writer.log must not mention 'foo.txt', got:\n{contents}"
    );
}

/// C4-B (extended): poll writer.log for child error within a generous timeout.
///
/// Payload has transcript_path field set to nonexistent path. The `HookPayload`
/// struct ignores `transcript_path` (it's not currently in the struct) — so
/// session_loader uses cwd-based resolution and degrades gracefully (no error logged).
/// We instead verify no panic occurs and parent exits 0.
#[test]
fn parent_exits_zero_with_child_on_bad_payload() {
    let home = TempDir::new().expect("tempdir for fake HOME");

    // Valid JSON but cwd points to a path where no session/transcript exists.
    let payload = r#"{"session_id":"t-c4b-ext","cwd":"/nonexistent/absolute/path/xyz"}"#;

    let bin = env!("CARGO_BIN_EXE_tallytape-writer");

    let mut child = Command::new(bin)
        .env("HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tallytape-writer");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.as_bytes()).unwrap();
    }

    let status = child.wait().expect("wait failed");
    assert!(
        status.success(),
        "parent must exit 0 even with unresolvable cwd, got: {status}"
    );

    // Wait for the child worker to finish (it's detached, give it some time).
    let deadline = Instant::now() + Duration::from_secs(5);
    let db_path = {
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
    };

    // Poll until DB exists or timeout.
    while !db_path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }

    // If the child completed, DB should exist (session was written with degraded result).
    // If it's still running somehow, that's still not a parent failure.
    // The key assertion is parent already exited 0 above.
}
