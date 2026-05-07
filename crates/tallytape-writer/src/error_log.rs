use std::io::IsTerminal as _;
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;

const MAX_BYTES: u64 = 1_048_576;

static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// Append `line` to the file at `path`. Caller must supply the trailing `\n`.
/// If the file is ≥ 1 MB before the append, it is truncated first.
/// All IO errors are swallowed silently; metadata read failure is treated as size 0.
/// If the parent directory does not exist the write will fail silently (no panic).
pub(crate) fn append_line(path: &Path, line: &str) {
    let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size >= MAX_BYTES {
        let _ = std::fs::write(path, b"");
    }
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

pub fn write_log(path: &Path, session_id: Option<&str>, err: &anyhow::Error) {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let sid = session_id.unwrap_or("?");
    let line = format!("[{ts}] session={sid} error={err:#}\n");
    append_line(path, &line);
    if std::io::stderr().is_terminal() {
        eprint!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use tempfile::tempdir;

    // --- LogSink (append_line) unit tests ---

    #[test]
    fn append_line_creates_file_with_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        append_line(&path, "hello\n");
        assert!(path.exists(), "file should be created");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "hello\n");
    }

    #[test]
    fn append_line_truncates_at_1_5mb() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        // Write 1.5 MB of 'x'
        let big = vec![b'x'; 1_572_864];
        std::fs::write(&path, &big).unwrap();
        append_line(&path, "x\n");
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size < 2048, "file should be truncated, got {size} bytes");
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.ends_with("x\n"), "file should end with appended line");
    }

    #[test]
    fn append_line_nonexistent_subdir_no_panic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent_subdir").join("writer.log");
        // Must not panic; file is not created because parent dir doesn't exist
        append_line(&path, "y\n");
        assert!(!path.exists(), "file should NOT be created when parent dir missing");
    }

    #[test]
    fn creates_log_file_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        let err = anyhow!("something went wrong");
        write_log(&path, Some("sess1"), &err);
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("error="));
        assert!(contents.contains("something went wrong"));
    }

    #[test]
    fn appends_to_existing_log() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        std::fs::write(&path, "old line\n").unwrap();
        let err = anyhow!("new error");
        write_log(&path, Some("sess2"), &err);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("old line"));
        assert!(contents.contains("new error"));
        // two lines: old + new
        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn truncates_when_over_1mb() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        // write 1.5 MB of 'x'
        let big = vec![b'x'; 1_572_864];
        std::fs::write(&path, &big).unwrap();
        let err = anyhow!("truncation test");
        write_log(&path, Some("sess3"), &err);
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size < 2048, "file should be truncated, got {size} bytes");
    }

    #[test]
    fn swallows_io_errors() {
        let dir = tempdir().unwrap();
        // path inside a non-existent sub-directory — write will fail, must not panic
        let path = dir.path().join("nonexistent_subdir").join("writer.log");
        let err = anyhow!("io error swallow test");
        // must not panic and has no return value to check
        write_log(&path, None, &err);
    }

    #[test]
    fn formats_with_session_id() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        let err = anyhow!("format test");

        write_log(&path, Some("abc"), &err);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("session=abc"),
            "expected session=abc in: {contents}"
        );

        // reset and test None
        std::fs::remove_file(&path).unwrap();
        write_log(&path, None, &err);
        let contents2 = std::fs::read_to_string(&path).unwrap();
        assert!(
            contents2.contains("session=?"),
            "expected session=? in: {contents2}"
        );
    }

    #[test]
    fn timestamp_is_iso8601_utc() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("writer.log");
        let err = anyhow!("ts test");
        write_log(&path, None, &err);
        let contents = std::fs::read_to_string(&path).unwrap();
        // Expect format: [YYYY-MM-DDTHH:MM:SSZ]
        // Find the bracketed timestamp manually
        let bracket_start = contents.find('[').expect("no '[' found");
        let bracket_end = contents.find(']').expect("no ']' found");
        let ts = &contents[bracket_start + 1..bracket_end];
        // Should look like: 2026-05-06T12:34:56Z
        assert_eq!(ts.len(), 20, "timestamp length should be 20, got: {ts}");
        assert_eq!(&ts[4..5], "-", "year-month separator");
        assert_eq!(&ts[7..8], "-", "month-day separator");
        assert_eq!(&ts[10..11], "T", "date-time separator");
        assert_eq!(&ts[13..14], ":", "hour-minute separator");
        assert_eq!(&ts[16..17], ":", "minute-second separator");
        assert_eq!(&ts[19..20], "Z", "UTC suffix");
        assert!(ts[..4].chars().all(|c| c.is_ascii_digit()), "year digits");
    }
}
