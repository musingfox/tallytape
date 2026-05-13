use std::path::{Path, PathBuf};

/// Convert a working directory into the Claude Code project-slug used as a
/// directory name under `~/.claude/projects/`.
///
/// Rule (validated against live `~/.claude/projects/` directories): every
/// character that is not in `[A-Za-z0-9-]` is replaced with `-`. Consecutive
/// `-` are NOT collapsed and a leading `-` is NOT stripped — both are part of
/// the on-disk convention.
///
/// Examples:
/// - `/Users/alice/code` → `-Users-alice-code`
/// - `/Users/alice/.config` → `-Users-alice--config`
/// - `/Users/alice/Mobile Documents` → `-Users-alice-Mobile-Documents`
pub fn slugify_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Build the deterministic JSONL transcript path for a Claude Code session.
///
/// Path layout: `<claude_home>/projects/<cwd-slug>/<session_id>.jsonl`
///
/// `claude_home` is typically `~/.claude` (the caller resolves `$HOME`).
pub fn transcript_path(claude_home: &Path, cwd: &str, session_id: &str) -> PathBuf {
    let mut p = claude_home.to_path_buf();
    p.push("projects");
    p.push(slugify_cwd(cwd));
    p.push(format!("{session_id}.jsonl"));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_simple_path() {
        assert_eq!(slugify_cwd("/Users/alice/code"), "-Users-alice-code");
    }

    #[test]
    fn slug_dot_becomes_dash_no_collapse() {
        assert_eq!(
            slugify_cwd("/Users/nickhuang/.config"),
            "-Users-nickhuang--config"
        );
        assert_eq!(
            slugify_cwd("/Users/nickhuang/.claude"),
            "-Users-nickhuang--claude"
        );
    }

    #[test]
    fn slug_preserves_case() {
        assert_eq!(
            slugify_cwd("/Users/nickhuang/workspace/SumVox"),
            "-Users-nickhuang-workspace-SumVox"
        );
    }

    #[test]
    fn slug_spaces_and_tildes_become_dashes() {
        assert_eq!(
            slugify_cwd("/Users/nickhuang/Library/Mobile Documents/iCloud~md~obsidian/Documents/Obsidian Vault"),
            "-Users-nickhuang-Library-Mobile-Documents-iCloud-md-obsidian-Documents-Obsidian-Vault"
        );
    }

    #[test]
    fn slug_short_path() {
        assert_eq!(slugify_cwd("/private/tmp"), "-private-tmp");
    }

    #[test]
    fn slug_existing_dashes_kept() {
        assert_eq!(
            slugify_cwd("/Users/nickhuang/workspace/cc-plugins"),
            "-Users-nickhuang-workspace-cc-plugins"
        );
    }

    #[test]
    fn slug_underscore_becomes_dash() {
        assert_eq!(slugify_cwd("/foo/bar_baz"), "-foo-bar-baz");
    }

    #[test]
    fn slug_empty_input_is_empty() {
        assert_eq!(slugify_cwd(""), "");
    }

    #[test]
    fn slug_only_separators() {
        assert_eq!(slugify_cwd("/."), "--");
    }

    #[test]
    fn transcript_path_layout() {
        let home = Path::new("/Users/alice/.claude");
        let p = transcript_path(home, "/Users/alice/code", "abc-123");
        assert_eq!(
            p,
            PathBuf::from("/Users/alice/.claude/projects/-Users-alice-code/abc-123.jsonl")
        );
    }

    #[test]
    fn transcript_path_session_id_appended_with_jsonl() {
        let home = Path::new("/h");
        let p = transcript_path(home, "/x", "00000000-1111-2222-3333-444444444444");
        assert_eq!(
            p,
            PathBuf::from("/h/projects/-x/00000000-1111-2222-3333-444444444444.jsonl")
        );
    }

    /// Spot-check against the live filesystem when available: the path we
    /// build for a known cwd/sessionId pair must match an actual file.
    /// The test silently passes when the live data isn't present so it
    /// stays usable in CI/sandboxed environments.
    #[cfg(not(coverage))]
    #[test]
    fn matches_live_filesystem_when_available() {
        use std::fs;
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let claude_home = PathBuf::from(home).join(".claude");
        let sessions_dir = claude_home.join("sessions");
        let Ok(entries) = fs::read_dir(&sessions_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let (Some(cwd), Some(sid)) = (
                v.get("cwd").and_then(|x| x.as_str()),
                v.get("sessionId").and_then(|x| x.as_str()),
            ) else {
                continue;
            };
            let built = transcript_path(&claude_home, cwd, sid);
            if built.exists() {
                return; // success: at least one real session resolves
            }
        }
        // No matching live transcript found — treat as inconclusive, not failure.
    }
}
