use anyhow::{anyhow, Context};
use serde::Deserialize;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::session::NewSession;

/// Maximum size (bytes) of a session JSON file we are willing to read into
/// memory. Real session files written by Claude Code are <1 KB; anything
/// approaching this cap is either malformed or a symlink to an unrelated
/// large file.
const MAX_SESSION_FILE_BYTES: u64 = 1024 * 1024; // 1 MiB

/// Raw JSON shape of a Claude Code session file.
#[derive(Deserialize)]
struct RawSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    #[serde(rename = "startedAt")]
    started_at_ms: i64,
    #[serde(rename = "updatedAt")]
    updated_at_ms: i64,
}

/// Output of [`discover`]: successfully parsed sessions plus per-file errors.
pub struct DiscoveryResult {
    /// Sessions sorted by `updatedAt` descending; tiebreak `external_id` ascending.
    pub sessions: Vec<NewSession>,
    /// Files that could not be parsed, with the reason for each failure.
    pub errors: Vec<(PathBuf, anyhow::Error)>,
}

impl DiscoveryResult {
    /// Return the session matching `session_id`, or the most-recent session when
    /// `session_id` is `None`.  Returns `None` when the result set is empty or the
    /// id is not found.
    pub fn pick(&self, session_id: Option<&str>) -> Option<&NewSession> {
        match session_id {
            Some(id) => self.sessions.iter().find(|s| s.external_id == id),
            None => self.sessions.first(),
        }
    }
}

/// Read a single `.json` file and convert it to a `(NewSession, updated_at_ms)` pair.
fn parse_one(path: &Path) -> anyhow::Result<(NewSession, i64)> {
    let len = fs::metadata(path)
        .with_context(|| format!("discovery::parse_one: stat {:?}", path))?
        .len();
    if len > MAX_SESSION_FILE_BYTES {
        return Err(anyhow!(
            "discovery::parse_one: {:?} is {} bytes, exceeds cap {}",
            path,
            len,
            MAX_SESSION_FILE_BYTES
        ));
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("discovery::parse_one: read {:?}", path))?;

    let parsed = serde_json::from_str::<RawSession>(&raw)
        .with_context(|| format!("discovery::parse_one: parse {:?}", path))?;

    let new_session = NewSession {
        source: "claude-code".to_string(),
        external_id: parsed.session_id,
        cwd: Some(parsed.cwd),
        started_at: parsed.started_at_ms / 1000,
        ended_at: None,
        metadata: Some(raw),
    };

    Ok((new_session, parsed.updated_at_ms))
}

/// Scan `dir` for `.json` session files and return parsed sessions plus any
/// per-file errors.
///
/// Returns a top-level `Err` only when `read_dir(dir)` itself fails (directory
/// does not exist, is not a directory, or permission denied).  Parse failures
/// for individual files are collected into `DiscoveryResult::errors`.
///
/// Non-`.json` files and subdirectories are silently skipped.
pub fn discover(dir: &Path) -> anyhow::Result<DiscoveryResult> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("discovery::discover: read_dir {:?}", dir))?;

    let mut buffer: Vec<(NewSession, i64)> = Vec::new();
    let mut errors: Vec<(PathBuf, anyhow::Error)> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                // Cannot determine path; skip this entry but record it under a
                // synthetic path so the caller still sees an error.
                errors.push((
                    dir.to_path_buf(),
                    anyhow::Error::from(e).context("discovery::discover: read entry"),
                ));
                continue;
            }
        };

        let path = entry.path();

        // Skip subdirectories, symlinks, and non-.json files silently.
        // `file_type()` does NOT follow symlinks (unlike `path.is_file()`),
        // so a symlink to a regular file is rejected here.
        let is_regular_file = match entry.file_type() {
            Ok(ft) => ft.is_file(),
            Err(e) => {
                errors.push((
                    path.clone(),
                    anyhow::Error::from(e).context("discovery::discover: file_type"),
                ));
                continue;
            }
        };
        if !is_regular_file {
            continue;
        }
        if path.extension() != Some(OsStr::new("json")) {
            continue;
        }

        match parse_one(&path) {
            Ok(pair) => buffer.push(pair),
            Err(e) => errors.push((path, e)),
        }
    }

    // Sort: updatedAt desc, tiebreak external_id asc — deterministic regardless
    // of read_dir order.
    buffer.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.external_id.cmp(&b.0.external_id))
    });

    let sessions = buffer.into_iter().map(|(s, _)| s).collect();

    Ok(DiscoveryResult { sessions, errors })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    /// Write `contents` to `dir/name`, panicking on any I/O error.
    fn write(dir: &Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    // -------------------------------------------------------------------------
    // 1. happy path, two files, sorted, ms→s conversion + metadata round-trip
    // -------------------------------------------------------------------------
    #[test]
    fn happy_path_two_files_sorted() {
        let dir = tempdir().unwrap();
        let a_contents = r#"{"sessionId":"AAA","cwd":"/x","startedAt":2000000,"updatedAt":2000}"#;
        let b_contents = r#"{"sessionId":"BBB","cwd":"/y","startedAt":1000000,"updatedAt":1000}"#;
        write(dir.path(), "a.json", a_contents);
        write(dir.path(), "b.json", b_contents);

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 2);
        assert_eq!(result.errors.len(), 0);

        assert_eq!(result.sessions[0].external_id, "AAA");
        assert_eq!(result.sessions[1].external_id, "BBB");
        assert_eq!(result.sessions[0].started_at, 2000);   // 2000000 / 1000
        assert_eq!(result.sessions[0].source, "claude-code");
        assert_eq!(result.sessions[0].ended_at, None);
        assert_eq!(result.sessions[0].metadata, Some(a_contents.to_string()));
    }

    // -------------------------------------------------------------------------
    // 2. broken json mixed with valid
    // -------------------------------------------------------------------------
    #[test]
    fn broken_json_mixed_with_valid() {
        let dir = tempdir().unwrap();
        write(dir.path(), "valid.json", r#"{"sessionId":"AAA","cwd":"/x","startedAt":1000000,"updatedAt":1000}"#);
        write(dir.path(), "broken.json", "not json");

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].0.ends_with("broken.json"));
    }

    // -------------------------------------------------------------------------
    // 3. missing required field (no sessionId)
    // -------------------------------------------------------------------------
    #[test]
    fn missing_required_field_no_session_id() {
        let dir = tempdir().unwrap();
        write(dir.path(), "nosid.json", r#"{"cwd":"/x","startedAt":1,"updatedAt":1}"#);

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 0);
        assert_eq!(result.errors.len(), 1);
    }

    // -------------------------------------------------------------------------
    // 4. non-json files and subdirs skipped silently
    // -------------------------------------------------------------------------
    #[test]
    fn non_json_files_and_subdirs_skipped() {
        let dir = tempdir().unwrap();
        write(dir.path(), "valid.json", r#"{"sessionId":"AAA","cwd":"/x","startedAt":1000000,"updatedAt":1000}"#);
        write(dir.path(), "notes.txt", "some text here");
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.errors.len(), 0);
    }

    // -------------------------------------------------------------------------
    // 5. non-existent dir → top-level Err
    // -------------------------------------------------------------------------
    #[test]
    fn non_existent_dir_returns_top_level_err() {
        let result = discover(Path::new("/does/not/exist/at/all"));
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // 6. deterministic tiebreak on identical updatedAt
    // -------------------------------------------------------------------------
    #[test]
    fn deterministic_tiebreak_identical_updated_at() {
        let dir = tempdir().unwrap();
        write(dir.path(), "z.json", r#"{"sessionId":"ZZZ","cwd":"/z","startedAt":1000000,"updatedAt":5000}"#);
        write(dir.path(), "a.json", r#"{"sessionId":"AAA","cwd":"/a","startedAt":1000000,"updatedAt":5000}"#);

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 2);
        // Tiebreak: external_id ascending → "AAA" before "ZZZ"
        assert_eq!(result.sessions[0].external_id, "AAA");
        assert_eq!(result.sessions[1].external_id, "ZZZ");
    }

    // -------------------------------------------------------------------------
    // 7. pick by id with hit
    // -------------------------------------------------------------------------
    #[test]
    fn pick_by_id_hit() {
        let dir = tempdir().unwrap();
        write(dir.path(), "a.json", r#"{"sessionId":"AAA","cwd":"/x","startedAt":2000000,"updatedAt":2000}"#);
        write(dir.path(), "b.json", r#"{"sessionId":"BBB","cwd":"/y","startedAt":1000000,"updatedAt":1000}"#);

        let result = discover(dir.path()).unwrap();
        let picked = result.pick(Some("BBB"));
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().external_id, "BBB");
        // Verify it's the second element (index 1)
        assert_eq!(picked.unwrap() as *const _, &result.sessions[1] as *const _);
    }

    // -------------------------------------------------------------------------
    // 8. pick None returns first (most-recent)
    // -------------------------------------------------------------------------
    #[test]
    fn pick_none_returns_first() {
        let dir = tempdir().unwrap();
        write(dir.path(), "a.json", r#"{"sessionId":"AAA","cwd":"/x","startedAt":2000000,"updatedAt":2000}"#);
        write(dir.path(), "b.json", r#"{"sessionId":"BBB","cwd":"/y","startedAt":1000000,"updatedAt":1000}"#);

        let result = discover(dir.path()).unwrap();
        let picked = result.pick(None);
        assert!(picked.is_some());
        assert_eq!(picked.unwrap().external_id, "AAA");
        assert_eq!(picked.unwrap() as *const _, &result.sessions[0] as *const _);
    }

    // -------------------------------------------------------------------------
    // 9. pick miss returns None
    // -------------------------------------------------------------------------
    #[test]
    fn pick_miss_returns_none() {
        let dir = tempdir().unwrap();
        write(dir.path(), "a.json", r#"{"sessionId":"AAA","cwd":"/x","startedAt":2000000,"updatedAt":2000}"#);
        write(dir.path(), "b.json", r#"{"sessionId":"BBB","cwd":"/y","startedAt":1000000,"updatedAt":1000}"#);

        let result = discover(dir.path()).unwrap();
        assert!(result.pick(Some("missing")).is_none());
    }

    // -------------------------------------------------------------------------
    // 10. pick on empty
    // -------------------------------------------------------------------------
    #[test]
    fn pick_on_empty() {
        let dir = tempdir().unwrap();
        let result = discover(dir.path()).unwrap();
        assert!(result.pick(None).is_none());
        assert!(result.pick(Some("x")).is_none());
    }

    // -------------------------------------------------------------------------
    // 12. symlink to regular file is skipped (not followed)
    // -------------------------------------------------------------------------
    #[cfg(unix)]
    #[test]
    fn symlink_to_json_is_skipped() {
        let dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target = target_dir.path().join("real.json");
        std::fs::write(
            &target,
            r#"{"sessionId":"AAA","cwd":"/x","startedAt":1000000,"updatedAt":1000}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("link.json")).unwrap();

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 0, "symlinks must not be followed");
        assert_eq!(result.errors.len(), 0);
    }

    // -------------------------------------------------------------------------
    // 13. oversized file becomes a per-file error, not OOM
    // -------------------------------------------------------------------------
    #[test]
    fn oversized_file_returns_error_not_panic() {
        let dir = tempdir().unwrap();
        let big = "a".repeat((super::MAX_SESSION_FILE_BYTES + 1) as usize);
        write(dir.path(), "big.json", &big);

        let result = discover(dir.path()).unwrap();
        assert_eq!(result.sessions.len(), 0);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].0.ends_with("big.json"));
    }

    // -------------------------------------------------------------------------
    // 11. performance: 100 files <5ms
    // Run with: cargo test --release -- --ignored discover_100_files_perf
    // -------------------------------------------------------------------------
    #[test]
    #[ignore]
    fn discover_100_files_perf() {
        let dir = tempdir().unwrap();
        for i in 0..100u64 {
            let contents = format!(
                r#"{{"sessionId":"ID{i:03}","cwd":"/x","startedAt":{ms},"updatedAt":{ms}}}"#,
                i = i,
                ms = (i + 1) * 1000,
            );
            write(dir.path(), &format!("{i:03}.json"), &contents);
        }

        let start = std::time::Instant::now();
        let result = discover(dir.path()).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.sessions.len(), 100);
        assert_eq!(result.errors.len(), 0);
        assert!(
            elapsed.as_millis() < 5,
            "discover took {}ms, expected <5ms",
            elapsed.as_millis()
        );
    }
}
