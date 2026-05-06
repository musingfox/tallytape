use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::payload::HookPayload;

/// Spawn a detached `__worker` child that performs the actual ingest.
///
/// The parent writes the serialized `payload` JSON to the child's stdin,
/// then drops the write handle (= EOF). The child's stdout/stderr are
/// redirected to `/dev/null` so the parent's own stdio is unaffected.
/// The `Child` handle is dropped without calling `.wait()`, leaving the
/// child to run independently.
///
/// Returns `Err` only if spawning the child process fails. Payload
/// serialisation errors are treated as spawn failures.
pub fn spawn_worker(payload: &HookPayload) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    spawn_worker_with_exe(exe, payload)
}

/// Like `spawn_worker` but uses the given `exe` path instead of `current_exe()`.
///
/// This exists primarily for unit testing the spawn-failure branch without
/// depending on environment-specific `current_exe()` failures.
pub(crate) fn spawn_worker_with_exe(exe: PathBuf, payload: &HookPayload) -> io::Result<()> {
    let mut child = Command::new(exe)
        .arg("__worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Write payload JSON into child's stdin, then drop the writer (= EOF).
    if let Some(mut stdin) = child.stdin.take() {
        serde_json::to_writer(&mut stdin, payload)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // stdin writer dropped here → child receives EOF
    }

    // Drop child without waiting — parent exits immediately after spawn.
    // The OS (init/launchd on macOS) adopts the orphaned child process and
    // reaps it after it completes. No zombie accumulation occurs because this
    // parent is a short-lived hook process that exits right after this drop.
    drop(child);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::HookPayload;

    fn dummy_payload() -> HookPayload {
        HookPayload {
            session_id: "test-session".into(),
            cwd: "/tmp".into(),
            transcript_path: None,
        }
    }

    #[test]
    fn spawn_worker_with_exe_returns_err_on_nonexistent_path() {
        let bad_exe = PathBuf::from("/this/path/does/not/exist/tallytape-writer");
        let result = spawn_worker_with_exe(bad_exe, &dummy_payload());
        assert!(
            result.is_err(),
            "spawn_worker_with_exe must return Err for a nonexistent exe path"
        );
    }
}
