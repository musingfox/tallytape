use std::io::ErrorKind;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use tallytape_core::{
    discover, parse_transcript_file_with_stats, transcript_path, wait_for_flush, NewSession,
    ParsedItem, TokenStats,
};

use crate::payload::HookPayload;

/// Defaults aligned with p1-15 `wait_for_flush` recommendation.
const FLUSH_STABLE_MS: u64 = 300;
const FLUSH_MAX_MS: u64 = 1500;

const SOURCE: &str = "claude-code";

#[derive(Debug, Clone)]
pub struct SessionResult {
    pub session: NewSession,
    pub items: Vec<ParsedItem>,
    pub stats: TokenStats,
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn fallback_session(payload: &HookPayload) -> NewSession {
    NewSession {
        source: SOURCE.to_string(),
        external_id: payload.session_id.clone(),
        cwd: Some(payload.cwd.clone()),
        started_at: now_unix_seconds(),
        ended_at: None,
        metadata: None,
    }
}

/// Resolve the session record + parsed transcript for the given hook payload.
///
/// Never propagates errors: missing or unreadable transcripts and discovery
/// failures are logged and degrade to a minimal `SessionResult` so the writer
/// can still persist a session row. The hook is never blocked by I/O.
pub fn load_session(payload: &HookPayload, claude_home: &Path) -> SessionResult {
    let mut session = resolve_session(payload, claude_home);

    let path = transcript_path(claude_home, &payload.cwd, &payload.session_id);

    if !path.exists() {
        log::error!(
            "transcript not found for session {}: {} — degraded session",
            payload.session_id,
            path.display()
        );
        return SessionResult {
            session,
            items: Vec::new(),
            stats: TokenStats::default(),
        };
    }

    if let Err(e) = wait_for_flush(&path, FLUSH_STABLE_MS, FLUSH_MAX_MS) {
        log::warn!("wait_for_flush({}) failed: {e}", path.display());
    }

    let (mut items, mut stats) = match parse_transcript_file_with_stats(&path) {
        Ok(parsed) => (parsed.items, parsed.stats),
        Err(e) => {
            log::error!(
                "parse_transcript_file_with_stats({}) failed for session {}: {e}",
                path.display(),
                payload.session_id
            );
            return SessionResult {
                session,
                items: Vec::new(),
                stats: TokenStats::default(),
            };
        }
    };

    // Load subagent transcripts from <projects>/<slug>/<sid>/subagents/
    let subagents_dir = path
        .parent()
        .unwrap()
        .join(&payload.session_id)
        .join("subagents");

    load_subagents(&subagents_dir, &mut items, &mut stats);

    // Set ended_at to the maximum occurred_at across all items (parent + subagents).
    // If items is empty, max() is None which leaves ended_at as None — by construction.
    session.ended_at = items.iter().map(|i| i.occurred_at).max();

    SessionResult {
        session,
        items,
        stats,
    }
}

/// Discover and parse all `agent-*.jsonl` files under `subagents_dir`.
/// Never propagates errors: NotFound → silent; other errors → WARN.
fn load_subagents(subagents_dir: &Path, items: &mut Vec<ParsedItem>, stats: &mut TokenStats) {
    let read_dir = match std::fs::read_dir(subagents_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // Missing subagents dir is expected — not an error.
            return;
        }
        Err(e) => {
            log::warn!(
                "read_dir({}) failed: {e}",
                subagents_dir.display()
            );
            return;
        }
    };

    // Collect agent-*.jsonl paths and sort for determinism.
    let mut subagent_paths: Vec<std::path::PathBuf> = read_dir
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name();
            let name = file_name.to_str()?;
            if name.starts_with("agent-") && name.ends_with(".jsonl") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();

    subagent_paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    for subagent_path in subagent_paths {
        // wait_for_flush is advisory; on failure warn but still attempt parse.
        if let Err(e) = wait_for_flush(&subagent_path, FLUSH_STABLE_MS, FLUSH_MAX_MS) {
            log::warn!(
                "wait_for_flush({}) failed: {e}",
                subagent_path.display()
            );
        }

        match parse_transcript_file_with_stats(&subagent_path) {
            Ok(parsed) => {
                // If the file is non-empty but produced zero items, it's likely malformed.
                let file_size = std::fs::metadata(&subagent_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                if parsed.items.is_empty() && file_size > 0 {
                    log::warn!(
                        "subagent file {} yielded no items (likely malformed)",
                        subagent_path.display()
                    );
                }
                // Sum stats field-wise.
                stats.input_tokens += parsed.stats.input_tokens;
                stats.output_tokens += parsed.stats.output_tokens;
                stats.cache_creation_tokens += parsed.stats.cache_creation_tokens;
                stats.cache_read_tokens += parsed.stats.cache_read_tokens;
                items.extend(parsed.items);
            }
            Err(e) => {
                log::warn!(
                    "parse subagent {} failed: {e}",
                    subagent_path.display()
                );
            }
        }
    }
}

fn resolve_session(payload: &HookPayload, claude_home: &Path) -> NewSession {
    let sessions_dir = claude_home.join("sessions");
    match discover(&sessions_dir) {
        Ok(result) => match result.pick(Some(&payload.session_id)) {
            Some(s) => s.clone(),
            None => {
                log::warn!(
                    "session {} not found under {} — using payload fallback",
                    payload.session_id,
                    sessions_dir.display()
                );
                fallback_session(payload)
            }
        },
        Err(e) => {
            log::warn!(
                "discover({}) failed: {e} — using payload fallback",
                sessions_dir.display()
            );
            fallback_session(payload)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ---------------------------------------------------------------------------
    // Fixture helpers
    // ---------------------------------------------------------------------------

    fn payload(session_id: &str, cwd: &str) -> HookPayload {
        HookPayload {
            session_id: session_id.to_string(),
            cwd: cwd.to_string(),
            transcript_path: None,
        }
    }

    fn write_session_file(claude_home: &Path, session_id: &str, cwd: &str) {
        let dir = claude_home.join("sessions");
        fs::create_dir_all(&dir).unwrap();
        let json = format!(
            r#"{{"sessionId":"{session_id}","cwd":"{cwd}","startedAt":1700000000000,"updatedAt":1700000000000}}"#
        );
        fs::write(dir.join(format!("{session_id}.json")), json).unwrap();
    }

    fn write_transcript(claude_home: &Path, cwd: &str, session_id: &str, contents: &str) {
        let path = transcript_path(claude_home, cwd, session_id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// Write a subagent transcript at
    /// `<claude_home>/projects/<cwd-slug>/<sid>/subagents/<file_name>`.
    fn write_subagent(claude_home: &Path, cwd: &str, sid: &str, file_name: &str, contents: &str) {
        let parent_path = transcript_path(claude_home, cwd, sid);
        let subagents_dir = parent_path.parent().unwrap().join(sid).join("subagents");
        fs::create_dir_all(&subagents_dir).unwrap();
        fs::write(subagents_dir.join(file_name), contents).unwrap();
    }

    const ASSISTANT_LINE: &str = r#"{"type":"assistant","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":10,"output_tokens":20}}}"#;

    fn make_assistant_line(request_id: &str) -> String {
        format!(
            r#"{{"type":"assistant","requestId":"{request_id}","uuid":"u-{request_id}","timestamp":"2025-11-15T10:23:45.000Z","message":{{"id":"msg-{request_id}","model":"claude-opus-4-7","usage":{{"input_tokens":5,"output_tokens":10}}}}}}"#
        )
    }

    #[test]
    fn loads_full_session_with_transcript() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "abc-123";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, ASSISTANT_LINE);

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.session.external_id, sid);
        assert_eq!(result.session.cwd.as_deref(), Some(cwd));
        assert_eq!(result.session.started_at, 1_700_000_000);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.stats.input_tokens, 10);
        assert_eq!(result.stats.output_tokens, 20);
    }

    #[test]
    fn missing_transcript_yields_empty_items_with_session() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "abc-123";
        write_session_file(home.path(), sid, cwd);
        // No transcript written.

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.session.external_id, sid);
        assert!(result.items.is_empty());
        assert_eq!(result.stats, TokenStats::default());
    }

    #[test]
    fn missing_session_file_falls_back_to_payload() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "abc-123";
        // sessions dir does not exist at all.

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.session.external_id, sid);
        assert_eq!(result.session.cwd.as_deref(), Some(cwd));
        assert_eq!(result.session.source, SOURCE);
        assert!(
            result.session.started_at > 0,
            "fallback started_at uses now()"
        );
        assert!(result.items.is_empty());
    }

    #[test]
    fn session_dir_exists_but_session_id_missing_uses_fallback() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "abc-123";
        write_session_file(home.path(), "other-sid", cwd);

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.session.external_id, sid);
        assert_eq!(result.session.cwd.as_deref(), Some(cwd));
    }

    #[test]
    fn malformed_transcript_degrades_to_empty_stats() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "abc-123";
        write_session_file(home.path(), sid, cwd);
        // File exists but contains junk; parser will skip lines and yield zero items.
        write_transcript(home.path(), cwd, sid, "not json line\n{also broken\n");

        let result = load_session(&payload(sid, cwd), home.path());
        assert!(result.items.is_empty());
        assert_eq!(result.stats, TokenStats::default());
    }

    #[test]
    fn cwd_with_spaces_resolves_transcript() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/Mobile Documents";
        let sid = "abc-123";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, ASSISTANT_LINE);

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.items.len(), 1);
    }

    // =========================================================================
    // Subagent tests (TC-A through TC-E)
    // =========================================================================

    /// TC-A: two subagent files merged under one session.
    #[test]
    fn tc_a_two_subagent_files_merged() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "tc-a-session";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, &make_assistant_line("rp"));
        write_subagent(home.path(), cwd, sid, "agent-1.jsonl", &make_assistant_line("ra"));
        write_subagent(home.path(), cwd, sid, "agent-2.jsonl", &make_assistant_line("rb"));

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.items.len(), 3, "expected parent + 2 subagent items");

        let ids: Vec<&str> = result.items.iter().map(|i| i.request_id.as_str()).collect();
        assert!(ids.contains(&"rp"), "parent item missing");
        assert!(ids.contains(&"ra"), "subagent-1 item missing");
        assert!(ids.contains(&"rb"), "subagent-2 item missing");

        // Stats should be summed (3 items × 5 input + 10 output each = 15/30).
        assert_eq!(result.stats.input_tokens, 15);
        assert_eq!(result.stats.output_tokens, 30);
    }

    /// TC-B: no subagents dir → no errors, only parent item returned.
    #[test]
    fn tc_b_no_subagents_dir_no_errors() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "tc-b-session";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, &make_assistant_line("rp"));
        // No subagents dir created.

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.items.len(), 1, "expected only parent item");
    }

    /// TC-C: one bad subagent file does not abort others.
    #[test]
    fn tc_c_bad_subagent_file_does_not_abort_others() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "tc-c-session";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, &make_assistant_line("rp"));
        // agent-1.jsonl is malformed (no valid assistant lines).
        write_subagent(home.path(), cwd, sid, "agent-1.jsonl", "not json\n{broken");
        // agent-2.jsonl is valid.
        write_subagent(home.path(), cwd, sid, "agent-2.jsonl", &make_assistant_line("rb"));

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(
            result.items.len(),
            2,
            "expected parent + rb (agent-1 malformed, agent-2 valid)"
        );
        // WARN logging for agent-1.jsonl is verified in integration tests (hook_errors.rs).
    }

    /// TC-D: tool-results sibling directory is ignored.
    #[test]
    fn tc_d_tool_results_dir_ignored() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "tc-d-session";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, &make_assistant_line("rp"));
        write_subagent(home.path(), cwd, sid, "agent-1.jsonl", &make_assistant_line("ra"));

        // Create the tool-results sibling directory with a file.
        let parent_path = transcript_path(home.path(), cwd, sid);
        let tool_results_dir = parent_path.parent().unwrap().join(sid).join("tool-results");
        fs::create_dir_all(&tool_results_dir).unwrap();
        fs::write(tool_results_dir.join("foo.txt"), "some tool result").unwrap();

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(
            result.items.len(),
            2,
            "expected parent + 1 subagent item; tool-results must be ignored"
        );
    }

    // =========================================================================
    // C2 tests: session.ended_at is set from items
    // =========================================================================

    /// C2-T1: single assistant line — session.ended_at equals that item's occurred_at.
    #[test]
    fn c2_t1_single_item_ended_at_matches() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "c2-t1-session";
        write_session_file(home.path(), sid, cwd);
        write_transcript(home.path(), cwd, sid, ASSISTANT_LINE);

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.items.len(), 1);
        let expected_max = result.items.iter().map(|i| i.occurred_at).max();
        assert_eq!(result.session.ended_at, expected_max);
        assert_eq!(result.session.ended_at, Some(result.items[0].occurred_at));
    }

    /// C2-T2: two lines with different timestamps — max wins.
    #[test]
    fn c2_t2_two_items_max_wins() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "c2-t2-session";
        write_session_file(home.path(), sid, cwd);
        // Two assistant lines with different timestamps.
        let line_earlier = r#"{"type":"assistant","requestId":"r-early","uuid":"u-early","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-early","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
        let line_later = r#"{"type":"assistant","requestId":"r-later","uuid":"u-later","timestamp":"2025-11-15T11:00:00.000Z","message":{"id":"msg-later","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
        write_transcript(
            home.path(),
            cwd,
            sid,
            &format!("{line_earlier}\n{line_later}"),
        );

        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(result.items.len(), 2);

        // Find the max occurred_at
        let max_occurred_at = result.items.iter().map(|i| i.occurred_at).max().unwrap();
        assert_eq!(result.session.ended_at, Some(max_occurred_at));

        // The later timestamp should be 2025-11-15T11:00:00Z = 1731668400
        let later_item = result
            .items
            .iter()
            .find(|i| i.request_id == "r-later")
            .unwrap();
        assert_eq!(result.session.ended_at, Some(later_item.occurred_at));
    }

    /// C2-T3: missing transcript — degraded path, ended_at stays None.
    #[test]
    fn c2_t3_missing_transcript_ended_at_none() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "c2-t3-session";
        write_session_file(home.path(), sid, cwd);
        // No transcript written — degraded path.

        let result = load_session(&payload(sid, cwd), home.path());
        assert!(result.items.is_empty());
        assert!(result.session.ended_at.is_none(), "degraded path must leave ended_at as None");
    }

    /// TC-E: request_id shared between parent and subagent deduplicated by persist.
    /// This test verifies session_loader merges items (persist handles dedup).
    #[test]
    fn tc_e_duplicate_request_id_both_in_items() {
        let home = tempdir().unwrap();
        let cwd = "/Users/alice/code";
        let sid = "tc-e-session";
        write_session_file(home.path(), sid, cwd);
        // Parent and subagent both have "rdup" but different message_ids.
        let parent_line = r#"{"type":"assistant","requestId":"rdup","uuid":"u-parent","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-parent","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
        let subagent_line = r#"{"type":"assistant","requestId":"rdup","uuid":"u-sub","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg-sub","model":"claude-opus-4-7","usage":{"input_tokens":5,"output_tokens":10}}}"#;
        write_transcript(home.path(), cwd, sid, parent_line);
        write_subagent(home.path(), cwd, sid, "agent-1.jsonl", subagent_line);

        // load_session returns BOTH items (dedup is persist's job).
        let result = load_session(&payload(sid, cwd), home.path());
        assert_eq!(
            result.items.len(),
            2,
            "load_session should return both items; persist handles dedup"
        );
        assert!(result.items.iter().all(|i| i.request_id == "rdup"));
    }
}
