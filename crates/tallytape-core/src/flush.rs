use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// Polls `path`'s mtime until it has been stable for `stable_ms` milliseconds,
/// or until `max_ms` total milliseconds have elapsed (graceful timeout).
///
/// Returns `Ok(())` on stability or timeout.
/// Returns `Err` if any filesystem I/O call fails (e.g. file missing, permission denied).
pub fn wait_for_flush(path: &Path, stable_ms: u64, max_ms: u64) -> anyhow::Result<()> {
    let start = Instant::now();
    let mut last_mtime = std::fs::metadata(path)?.modified()?;
    loop {
        if start.elapsed() >= Duration::from_millis(max_ms) {
            return Ok(());
        }
        let sleep_dur = Duration::from_millis(stable_ms)
            .min(Duration::from_millis(max_ms).saturating_sub(start.elapsed()));
        thread::sleep(sleep_dur);
        let now_mtime = std::fs::metadata(path)?.modified()?;
        if now_mtime == last_mtime {
            return Ok(());
        }
        last_mtime = now_mtime;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn write_file(path: &Path, content: &[u8]) {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
    }

    fn touch_file(path: &Path) {
        // Re-writing content bumps mtime reliably across platforms.
        let existing = std::fs::read(path).unwrap_or_default();
        let mut new_content = existing.clone();
        new_content.push(b'.');
        write_file(path, &new_content);
    }

    // Test 1: stable file — written once, mtime never changes after initial write.
    #[test]
    fn test_stable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stable.txt");
        write_file(&path, b"hello");

        let t0 = Instant::now();
        let result = wait_for_flush(&path, 20, 100);
        let elapsed = t0.elapsed();

        assert!(result.is_ok());
        assert!(elapsed >= Duration::from_millis(20), "elapsed={elapsed:?}");
    }

    // Test 2: continuously written file — writer touches the file every 10ms,
    // so wait_for_flush should time out at ~max_ms.
    #[test]
    fn test_continuously_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("busy.txt");
        write_file(&path, b"init");

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let path_clone = path.clone();

        let writer = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                touch_file(&path_clone);
                thread::sleep(Duration::from_millis(10));
            }
        });

        let t0 = Instant::now();
        let result = wait_for_flush(&path, 20, 100);
        let elapsed = t0.elapsed();

        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();

        assert!(result.is_ok());
        // Should have timed out, so elapsed is roughly max_ms (80–200ms margin)
        assert!(
            elapsed >= Duration::from_millis(80),
            "expected near-timeout, elapsed={elapsed:?}"
        );
        assert!(
            elapsed <= Duration::from_millis(200),
            "took too long, elapsed={elapsed:?}"
        );
    }

    // Test 3: missing file — must return Err.
    #[test]
    fn test_missing_file() {
        let result = wait_for_flush(Path::new("/nonexistent/path/xyz_tallytape"), 20, 100);
        assert!(result.is_err());
    }

    // Test 4: file modified once then stable — mtime changes once, then stays stable.
    #[test]
    fn test_modified_once_then_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("once.txt");
        write_file(&path, b"original");

        // Touch the file once after 25ms, then stop.
        let path_clone = path.clone();
        std::thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            touch_file(&path_clone);
        });

        let t0 = Instant::now();
        let result = wait_for_flush(&path, 20, 100);
        let elapsed = t0.elapsed();

        assert!(result.is_ok());
        // Should have detected the change and then waited another stable_ms before confirming.
        // Total elapsed should be well under max_ms (100ms) but at least stable_ms (20ms).
        assert!(elapsed >= Duration::from_millis(20), "elapsed={elapsed:?}");
    }
}
