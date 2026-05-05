use std::io::BufRead;
use std::path::Path;

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Public output type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedItem {
    pub request_id: String,
    pub message_id: Option<String>,
    pub parent_uuid: Option<String>,
    pub is_sidechain: bool,
    pub occurred_at: i64,
    pub model: String,
    pub service_tier: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub metadata: Option<String>,
}

// ---------------------------------------------------------------------------
// Private deserialization types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    parent_uuid: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    timestamp: Option<String>,
    message: Option<RawMessage>,
}

#[derive(Deserialize)]
struct RawMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<RawUsage>,
    content: Option<Value>,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    service_tier: Option<String>,
}

// ---------------------------------------------------------------------------
// Core parsing logic
// ---------------------------------------------------------------------------

pub fn parse_transcript_reader<R: BufRead>(reader: R) -> Vec<ParsedItem> {
    let mut items = Vec::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(e) => {
                log::warn!("I/O error reading transcript line: {e}");
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let raw: RawLine = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("malformed JSONL line: {e}");
                continue;
            }
        };

        // Skip non-assistant lines silently
        match raw.kind.as_deref() {
            Some("assistant") => {}
            _ => continue,
        }

        // Resolve request_id: prefer requestId, fall back to uuid
        let request_id = match raw.request_id.or(raw.uuid) {
            Some(id) => id,
            None => {
                log::warn!("assistant line missing both requestId and uuid — skipping");
                continue;
            }
        };

        // Parse timestamp
        let occurred_at = match raw.timestamp.as_deref() {
            Some(ts) => match chrono::DateTime::parse_from_rfc3339(ts) {
                Ok(dt) => dt.timestamp(),
                Err(e) => {
                    log::warn!("bad timestamp '{ts}': {e} — skipping");
                    continue;
                }
            },
            None => {
                log::warn!("assistant line missing timestamp — skipping");
                continue;
            }
        };

        // Extract model
        let model = match raw.message.as_ref().and_then(|m| m.model.clone()) {
            Some(m) => m,
            None => {
                log::warn!("assistant line missing message.model — skipping");
                continue;
            }
        };

        // Extract usage fields
        let usage = raw.message.as_ref().and_then(|m| m.usage.as_ref());
        let input_tokens = usage.and_then(|u| u.input_tokens).unwrap_or(0);
        let output_tokens = usage.and_then(|u| u.output_tokens).unwrap_or(0);
        let cache_read_tokens = usage.and_then(|u| u.cache_read_input_tokens);
        let cache_creation_tokens = usage.and_then(|u| u.cache_creation_input_tokens);
        let service_tier = usage.and_then(|u| u.service_tier.clone());

        // Extract message_id
        let message_id = raw.message.as_ref().and_then(|m| m.id.clone());

        // Serialize content to metadata
        let metadata = raw
            .message
            .as_ref()
            .and_then(|m| m.content.as_ref())
            .map(|c| c.to_string());

        items.push(ParsedItem {
            request_id,
            message_id,
            parent_uuid: raw.parent_uuid,
            is_sidechain: raw.is_sidechain.unwrap_or(false),
            occurred_at,
            model,
            service_tier,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            metadata,
        });
    }

    items
}

pub fn parse_transcript_file(path: &Path) -> anyhow::Result<Vec<ParsedItem>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open transcript file: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    Ok(parse_transcript_reader(reader))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::sync::{Mutex, OnceLock};

    // ----- Warning capture infrastructure ----------------------------------

    struct CapturingLogger {
        warnings: Mutex<Vec<String>>,
    }

    impl log::Log for CapturingLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record) {
            if record.level() <= log::Level::Warn {
                self.warnings
                    .lock()
                    .unwrap()
                    .push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    static LOGGER: OnceLock<&'static CapturingLogger> = OnceLock::new();

    fn get_logger() -> &'static CapturingLogger {
        LOGGER.get_or_init(|| {
            let logger: &'static CapturingLogger = Box::leak(Box::new(CapturingLogger {
                warnings: Mutex::new(Vec::new()),
            }));
            log::set_logger(logger).expect("failed to set logger");
            log::set_max_level(log::LevelFilter::Warn);
            logger
        })
    }

    fn init_logger() {
        let _ = get_logger();
    }

    // Mutex to serialize tests that check warning counts
    static WARNING_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn captured_warnings() -> Vec<String> {
        get_logger().warnings.lock().unwrap().clone()
    }

    fn clear_warnings() {
        get_logger().warnings.lock().unwrap().clear();
    }

    // ----- Helper ----------------------------------------------------------

    fn parse_str(s: &str) -> Vec<ParsedItem> {
        init_logger();
        parse_transcript_reader(BufReader::new(s.as_bytes()))
    }

    const VALID_ASSISTANT: &str = r#"{"type":"assistant","requestId":"req_top","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"msg_inner","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":2}}}"#;

    // ----- C1: request_id comes from top-level requestId ------------------

    #[test]
    fn c1_request_id_from_top_level() {
        let items = parse_str(VALID_ASSISTANT);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].request_id, "req_top");
        assert_eq!(items[0].message_id, Some("msg_inner".to_string()));
    }

    // ----- C2: nested usage tokens ----------------------------------------

    #[test]
    fn c2_nested_usage_tokens() {
        let items = parse_str(VALID_ASSISTANT);
        assert_eq!(items[0].input_tokens, 1);
        assert_eq!(items[0].output_tokens, 2);
    }

    // ----- C3: unknown usage fields tolerated, service_tier extracted ------

    #[test]
    fn c3_unknown_usage_fields_tolerated() {
        let line = r#"{"type":"assistant","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":10,"output_tokens":5,"service_tier":"standard","cache_creation":{"some":"object"},"iterations":3,"server_tool_use":{},"inference_geo":"us","speed":"fast"}}}"#;
        let items = parse_str(line);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].service_tier, Some("standard".to_string()));
    }

    // ----- C4: thinking content blocks tolerated, metadata captured --------

    #[test]
    fn c4_thinking_content_blocks() {
        let line = r#"{"type":"assistant","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1},"content":[{"type":"thinking","thinking":"","signature":"abc"}]}}"#;
        let items = parse_str(line);
        assert_eq!(items.len(), 1);
        assert!(items[0].metadata.is_some());
        let meta = items[0].metadata.as_ref().unwrap();
        // Should contain the thinking block
        assert!(meta.contains("thinking"));
    }

    // ----- C5: model from message.model -----------------------------------

    #[test]
    fn c5_model_from_message() {
        let items = parse_str(VALID_ASSISTANT);
        assert_eq!(items[0].model, "claude-opus-4-7");
    }

    // ----- C6: malformed line skipped + exactly 1 warning -----------------

    #[test]
    fn c6_malformed_line_skipped_with_warning() {
        init_logger();
        let _lock = WARNING_TEST_LOCK.lock().unwrap();
        clear_warnings();

        let input = format!("{VALID_ASSISTANT}\n{{not json\n{VALID_ASSISTANT}");
        let items = parse_str(&input);
        assert_eq!(items.len(), 2, "expected 2 valid items");

        let warnings = captured_warnings();
        assert_eq!(warnings.len(), 1, "expected exactly 1 warning, got: {warnings:?}");
    }

    // ----- C7: unknown top-level types skipped silently -------------------

    #[test]
    fn c7_unknown_types_skipped_silently() {
        init_logger();
        let _lock = WARNING_TEST_LOCK.lock().unwrap();
        clear_warnings();

        let input = format!(
            "{}\n{}\n{}\n{}",
            r#"{"type":"file-history-snapshot","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#,
            r#"{"type":"permission-mode","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#,
            r#"{"type":"ai-title","requestId":"r1","uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#,
            VALID_ASSISTANT
        );
        let items = parse_str(&input);
        assert_eq!(items.len(), 1);

        let warnings = captured_warnings();
        assert_eq!(warnings.len(), 0, "expected zero warnings, got: {warnings:?}");
    }

    // ----- C8: requestId fallback to uuid ---------------------------------

    #[test]
    fn c8_request_id_fallback_to_uuid() {
        let line = r#"{"type":"assistant","uuid":"u-xyz","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        let items = parse_str(line);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].request_id, "u-xyz");
    }

    // ----- C9: isSidechain default false + true ---------------------------

    #[test]
    fn c9_is_sidechain_default_false() {
        let items = parse_str(VALID_ASSISTANT);
        assert!(!items[0].is_sidechain);
    }

    #[test]
    fn c9_is_sidechain_true() {
        let line = r#"{"type":"assistant","requestId":"r1","isSidechain":true,"uuid":"u1","timestamp":"2025-11-15T10:23:45.000Z","message":{"id":"m1","model":"claude-opus-4-7","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        let items = parse_str(line);
        assert_eq!(items.len(), 1);
        assert!(items[0].is_sidechain);
    }

    // ----- C10: blank lines are silent ------------------------------------

    #[test]
    fn c10_blank_lines_silent() {
        init_logger();
        let _lock = WARNING_TEST_LOCK.lock().unwrap();
        clear_warnings();

        let input = format!("{VALID_ASSISTANT}\n\n   \n{VALID_ASSISTANT}");
        let items = parse_str(&input);
        assert_eq!(items.len(), 2);

        let warnings = captured_warnings();
        assert_eq!(warnings.len(), 0, "expected zero warnings for blank lines, got: {warnings:?}");
    }

    // ----- C11: timestamp ISO8601 → unix seconds --------------------------

    #[test]
    fn c11_timestamp_to_unix_seconds() {
        let items = parse_str(VALID_ASSISTANT);
        let expected = chrono::DateTime::parse_from_rfc3339("2025-11-15T10:23:45.000Z")
            .unwrap()
            .timestamp();
        assert_eq!(items[0].occurred_at, expected);
    }

    // ----- File-level tests -----------------------------------------------

    #[test]
    fn file_two_valid_lines_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        std::fs::write(&path, format!("{VALID_ASSISTANT}\n{VALID_ASSISTANT}\n")).unwrap();
        let result = parse_transcript_file(&path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn file_nonexistent_returns_err() {
        let result = parse_transcript_file(Path::new("/nonexistent/path/does_not_exist.jsonl"));
        assert!(result.is_err());
    }

    /// Spot-check against a live transcript when `TALLYTAPE_LIVE_TRANSCRIPT` is set.
    /// Silently passes when the env var is unset.
    #[test]
    fn live_spot_check_when_available() {
        let Ok(path_str) = std::env::var("TALLYTAPE_LIVE_TRANSCRIPT") else {
            return;
        };
        let path = Path::new(&path_str);
        let result = parse_transcript_file(path).expect("parse_transcript_file failed");
        assert!(
            result.len() >= 1,
            "expected at least 1 ParsedItem from live transcript"
        );
        assert!(
            result.iter().any(|item| item.model.starts_with("claude-")),
            "expected at least one item with model starting with 'claude-'"
        );
    }
}
