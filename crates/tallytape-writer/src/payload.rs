use std::io::Read;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookPayload {
    pub session_id: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
}

pub fn parse_payload<R: Read>(mut reader: R) -> anyhow::Result<HookPayload> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty stdin: expected hook payload JSON");
    }
    let payload: HookPayload = serde_json::from_str(trimmed)
        .map_err(|e| anyhow::anyhow!("malformed hook payload: {e}"))?;
    if payload.session_id.is_empty() {
        anyhow::bail!("hook payload missing session_id");
    }
    if payload.cwd.is_empty() {
        anyhow::bail!("hook payload missing cwd");
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_payload() {
        let json = r#"{"session_id":"abc","cwd":"/tmp/x","transcript_path":"/t.jsonl"}"#;
        let p = parse_payload(json.as_bytes()).unwrap();
        assert_eq!(p.session_id, "abc");
        assert_eq!(p.cwd, "/tmp/x");
        assert_eq!(p.transcript_path.as_deref(), Some("/t.jsonl"));
    }

    #[test]
    fn parses_payload_without_transcript_path() {
        let json = r#"{"session_id":"abc","cwd":"/tmp/x"}"#;
        let p = parse_payload(json.as_bytes()).unwrap();
        assert!(p.transcript_path.is_none());
    }

    #[test]
    fn round_trips() {
        let original = HookPayload {
            session_id: "s1".into(),
            cwd: "/work".into(),
            transcript_path: Some("/t".into()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed = parse_payload(json.as_bytes()).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn rejects_empty_stdin() {
        let err = parse_payload(b"".as_slice()).unwrap_err();
        assert!(err.to_string().contains("empty stdin"));
    }

    #[test]
    fn rejects_whitespace_only() {
        let err = parse_payload(b"   \n\t".as_slice()).unwrap_err();
        assert!(err.to_string().contains("empty stdin"));
    }

    #[test]
    fn rejects_missing_session_id() {
        let json = r#"{"cwd":"/tmp/x"}"#;
        let err = parse_payload(json.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("malformed"));
    }

    #[test]
    fn rejects_empty_session_id() {
        let json = r#"{"session_id":"","cwd":"/tmp/x"}"#;
        let err = parse_payload(json.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("session_id"));
    }

    #[test]
    fn rejects_empty_cwd() {
        let json = r#"{"session_id":"abc","cwd":""}"#;
        let err = parse_payload(json.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("cwd"));
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse_payload(b"{not json".as_slice()).unwrap_err();
        assert!(err.to_string().contains("malformed"));
    }
}
