use std::io::IsTerminal as _;
use std::path::PathBuf;

use log::{LevelFilter, Log, Metadata, Record};

/// Parse a log level filter from an optional env-var string.
/// Default is `Warn`; case-insensitive; invalid values fall back to `Warn`.
pub(crate) fn parse_level(value: Option<&str>) -> LevelFilter {
    match value {
        None => LevelFilter::Warn,
        Some(s) => s.parse::<LevelFilter>().unwrap_or(LevelFilter::Warn),
    }
}

struct FileLogger {
    path: Option<PathBuf>,
}

impl Log for FileLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        // max_level set globally handles filtering; we accept everything here
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Some(ref path) = self.path else {
            return;
        };
        let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let level = record.level().as_str();
        let target = record.target();
        let args = record.args();
        let line = format!("[{ts}] level={level} target={target} {args}\n");
        crate::error_log::append_line(path, &line);
        if std::io::stderr().is_terminal() {
            eprint!("{line}");
        }
    }

    fn flush(&self) {
        // open/close per write — nothing to flush
    }
}

/// Install the global `log` subscriber that writes to `writer.log`.
///
/// Reads `TALLYTAPE_LOG` for level override (default: `Warn`).
/// If `log_path()` fails the logger is still installed but writes are no-ops.
/// Returns `Err` only if a logger was already set (safe to ignore with `let _ = ...`).
pub fn init_logger() -> Result<(), log::SetLoggerError> {
    let level = parse_level(std::env::var("TALLYTAPE_LOG").ok().as_deref());
    let path = tallytape_core::log_path().ok();
    log::set_boxed_logger(Box::new(FileLogger { path }))?;
    log::set_max_level(level);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::LevelFilter;

    #[test]
    fn parse_level_none_is_warn() {
        assert_eq!(parse_level(None), LevelFilter::Warn);
    }

    #[test]
    fn parse_level_info_lowercase() {
        assert_eq!(parse_level(Some("info")), LevelFilter::Info);
    }

    #[test]
    fn parse_level_info_uppercase() {
        assert_eq!(parse_level(Some("INFO")), LevelFilter::Info);
    }

    #[test]
    fn parse_level_trace() {
        assert_eq!(parse_level(Some("trace")), LevelFilter::Trace);
    }

    #[test]
    fn parse_level_nonsense_is_warn() {
        assert_eq!(parse_level(Some("nonsense")), LevelFilter::Warn);
    }

    #[test]
    fn parse_level_off() {
        assert_eq!(parse_level(Some("off")), LevelFilter::Off);
    }
}
