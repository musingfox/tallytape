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
    let session = resolve_session(payload, claude_home);

    let path = transcript_path(claude_home, &payload.cwd, &payload.session_id);

    if !path.exists() {
        log::warn!("transcript missing: {} — degraded session", path.display());
        return SessionResult {
            session,
            items: Vec::new(),
            stats: TokenStats::default(),
        };
    }

    if let Err(e) = wait_for_flush(&path, FLUSH_STABLE_MS, FLUSH_MAX_MS) {
        log::warn!("wait_for_flush({}) failed: {e}", path.display());
    }

    match parse_transcript_file_with_stats(&path) {
        Ok(parsed) => SessionResult {
            session,
            items: parsed.items,
            stats: parsed.stats,
        },
        Err(e) => {
            log::warn!("parse_transcript_file_with_stats({}) failed: {e}", path.display());
            SessionResult {
                session,
                items: Vec::new(),
                stats: TokenStats::default(),
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

    const ASSISTANT_LINE: &str = r#"{"type":"assistant","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":10,"output_tokens":20}}}"#;

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
        assert!(result.session.started_at > 0, "fallback started_at uses now()");
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
}
