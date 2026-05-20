//! Patch `~/.claude/settings.json` to register tallytape's `SessionEnd`
//! hook (and remove it again). Pure file-system functions; no CLI / IO
//! beyond the paths the caller hands in, so the suite is unit-testable
//! against a `tempfile::TempDir`.
//!
//! ## Identification
//! A `SessionEnd` entry is considered "tallytape's" when any of its
//! inner `hooks[]` items has `type == "command"` and a `command` whose
//! filename component starts with `tallytape-writer`. This catches
//! `tallytape-writer`, `tallytape-writer.exe`, plus future suffixes,
//! and survives a user moving the binary between installs.
//!
//! ## Atomicity & safety
//! Writes go via `<settings>.tmp` followed by an in-place rename, so a
//! crash mid-write cannot corrupt `settings.json`. Before any write to
//! a pre-existing file, the prior bytes are copied to `<settings>.bak`
//! (overwriting any earlier backup). Missing-file install skips the
//! backup since there is nothing to preserve.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

/// Outcome of a single `install_hook` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallAction {
    /// `settings.json` did not exist; created a fresh one with our entry.
    Created,
    /// `settings.json` existed; appended a new tallytape entry.
    Added,
    /// `settings.json` had a tallytape entry; updated its `command` path.
    UpdatedCommand,
    /// `settings.json` already pointed at this exact binary; nothing written.
    NoOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub action: InstallAction,
    pub settings_path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallReport {
    /// `true` ⇒ a tallytape entry was found and removed.
    pub removed: bool,
    pub settings_path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

const TALLYTAPE_BIN_PREFIX: &str = "tallytape-writer";

/// Install (or refresh) tallytape's `SessionEnd` hook in `settings_path`,
/// pointing at `exe`. Idempotent: re-running with the same `exe` is a
/// `NoOp`, and re-running with a different `exe` rewrites the path.
pub fn install_hook(settings_path: &Path, exe: &Path) -> Result<InstallReport> {
    let existing = read_settings(settings_path)?;
    let had_file = existing.is_some();
    let mut doc = existing.unwrap_or_else(|| Value::Object(Map::new()));

    let session_end = ensure_session_end_array(&mut doc)?;
    let exe_str = exe.to_string_lossy().into_owned();

    // Pass 1: search for any existing tallytape entry.
    let mut matching_index: Option<usize> = None;
    for (i, entry) in session_end.iter().enumerate() {
        if is_tallytape_entry(entry) {
            matching_index = Some(i);
            break;
        }
    }

    let action = match matching_index {
        Some(idx) => {
            let current = tallytape_command_path(&session_end[idx]).unwrap_or_default();
            if current == exe_str {
                return Ok(InstallReport {
                    action: InstallAction::NoOp,
                    settings_path: settings_path.to_path_buf(),
                    backup_path: None,
                });
            }
            session_end[idx] = build_entry(&exe_str);
            InstallAction::UpdatedCommand
        }
        None => {
            session_end.push(build_entry(&exe_str));
            if had_file {
                InstallAction::Added
            } else {
                InstallAction::Created
            }
        }
    };

    let backup_path = if had_file {
        Some(backup_existing(settings_path)?)
    } else {
        None
    };
    atomic_write(settings_path, &doc)?;

    Ok(InstallReport {
        action,
        settings_path: settings_path.to_path_buf(),
        backup_path,
    })
}

/// Remove tallytape's `SessionEnd` hook entry from `settings_path`.
/// No-op (no backup, no write) if no tallytape entry is present or the
/// file does not exist.
pub fn uninstall_hook(settings_path: &Path) -> Result<UninstallReport> {
    let Some(mut doc) = read_settings(settings_path)? else {
        return Ok(UninstallReport {
            removed: false,
            settings_path: settings_path.to_path_buf(),
            backup_path: None,
        });
    };

    let Some(session_end) = session_end_array_mut(&mut doc) else {
        return Ok(UninstallReport {
            removed: false,
            settings_path: settings_path.to_path_buf(),
            backup_path: None,
        });
    };

    let original_len = session_end.len();
    session_end.retain(|entry| !is_tallytape_entry(entry));
    if session_end.len() == original_len {
        return Ok(UninstallReport {
            removed: false,
            settings_path: settings_path.to_path_buf(),
            backup_path: None,
        });
    }

    // Clean up empty containers so we don't leave dangling `{ "hooks": { "SessionEnd": [] } }`.
    let session_end_empty = session_end.is_empty();
    if session_end_empty {
        if let Some(hooks) = doc.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            hooks.remove("SessionEnd");
            if hooks.is_empty() {
                if let Some(root) = doc.as_object_mut() {
                    root.remove("hooks");
                }
            }
        }
    }

    let backup_path = Some(backup_existing(settings_path)?);
    atomic_write(settings_path, &doc)?;

    Ok(UninstallReport {
        removed: true,
        settings_path: settings_path.to_path_buf(),
        backup_path,
    })
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn read_settings(path: &Path) -> Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(Some(Value::Object(Map::new())));
            }
            let parsed: Value = serde_json::from_str(&text).with_context(|| {
                format!(
                    "settings.json at {} is not valid JSON; aborting to avoid corrupting it",
                    path.display()
                )
            })?;
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read settings.json at {}", path.display())),
    }
}

fn ensure_session_end_array(doc: &mut Value) -> Result<&mut Vec<Value>> {
    let root = doc
        .as_object_mut()
        .context("settings.json top-level must be a JSON object")?;
    let hooks_entry = root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks_entry
        .as_object_mut()
        .context("settings.json `hooks` field must be a JSON object")?;
    let se_entry = hooks
        .entry("SessionEnd".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    se_entry
        .as_array_mut()
        .context("settings.json `hooks.SessionEnd` must be a JSON array")
}

fn session_end_array_mut(doc: &mut Value) -> Option<&mut Vec<Value>> {
    doc.get_mut("hooks")?
        .get_mut("SessionEnd")?
        .as_array_mut()
}

fn build_entry(exe: &str) -> Value {
    json!({
        "matcher": "",
        "hooks": [
            { "type": "command", "command": exe }
        ]
    })
}

fn is_tallytape_entry(entry: &Value) -> bool {
    tallytape_command_path(entry).is_some()
}

fn tallytape_command_path(entry: &Value) -> Option<String> {
    let hooks = entry.get("hooks")?.as_array()?;
    for hook in hooks {
        if hook.get("type").and_then(Value::as_str) != Some("command") {
            continue;
        }
        let cmd = hook.get("command").and_then(Value::as_str)?;
        let file_name = Path::new(cmd).file_name().and_then(|n| n.to_str())?;
        if file_name.starts_with(TALLYTAPE_BIN_PREFIX) {
            return Some(cmd.to_string());
        }
    }
    None
}

fn backup_existing(settings_path: &Path) -> Result<PathBuf> {
    let backup = backup_path_for(settings_path);
    fs::copy(settings_path, &backup).with_context(|| {
        format!(
            "copy {} → {} for backup",
            settings_path.display(),
            backup.display()
        )
    })?;
    Ok(backup)
}

fn backup_path_for(settings_path: &Path) -> PathBuf {
    let mut name = settings_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    settings_path
        .parent()
        .map(|p| p.join(&name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn atomic_write(settings_path: &Path, doc: &Value) -> Result<()> {
    let mut tmp = settings_path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp_path = PathBuf::from(tmp);

    if let Some(parent) = settings_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create parent dir {}", parent.display()))?;
        }
    }

    let mut serialized = serde_json::to_string_pretty(doc)
        .context("serialize patched settings.json failed")?;
    serialized.push('\n');
    fs::write(&tmp_path, serialized.as_bytes())
        .with_context(|| format!("write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, settings_path).with_context(|| {
        format!(
            "rename {} → {}",
            tmp_path.display(),
            settings_path.display()
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn td() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        (dir, path)
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn install_creates_settings_when_file_absent() {
        let (_d, path) = td();
        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");

        let report = install_hook(&path, &exe).unwrap();

        assert_eq!(report.action, InstallAction::Created);
        assert!(report.backup_path.is_none());

        let doc = read(&path);
        let se = doc["hooks"]["SessionEnd"].as_array().unwrap();
        assert_eq!(se.len(), 1);
        assert_eq!(se[0]["matcher"], "");
        assert_eq!(
            se[0]["hooks"][0]["command"].as_str().unwrap(),
            exe.to_string_lossy()
        );
    }

    #[test]
    fn install_added_appends_to_existing_session_end_with_other_entries() {
        let (_d, path) = td();
        let initial = json!({
            "hooks": {
                "SessionEnd": [
                    {
                        "matcher": "",
                        "hooks": [{ "type": "command", "command": "/usr/bin/other-tool" }]
                    }
                ]
            }
        });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");
        let report = install_hook(&path, &exe).unwrap();

        assert_eq!(report.action, InstallAction::Added);
        assert!(report.backup_path.is_some());

        let doc = read(&path);
        let se = doc["hooks"]["SessionEnd"].as_array().unwrap();
        assert_eq!(se.len(), 2, "existing entry preserved + new appended");
        assert_eq!(se[0]["hooks"][0]["command"], "/usr/bin/other-tool");
        assert_eq!(
            se[1]["hooks"][0]["command"].as_str().unwrap(),
            exe.to_string_lossy()
        );
    }

    #[test]
    fn install_is_idempotent_with_same_path() {
        let (_d, path) = td();
        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");
        install_hook(&path, &exe).unwrap();

        let report = install_hook(&path, &exe).unwrap();

        assert_eq!(report.action, InstallAction::NoOp);
        assert!(report.backup_path.is_none(), "NoOp must not write a backup");

        // Backup file from the first install is allowed to be absent (file didn't
        // exist back then); the second NoOp call must NOT produce a new backup.
    }

    #[test]
    fn install_updates_command_when_path_changes() {
        let (_d, path) = td();
        let exe_v1 = PathBuf::from("/opt/v1/tallytape-writer");
        install_hook(&path, &exe_v1).unwrap();

        let exe_v2 = PathBuf::from("/opt/v2/tallytape-writer");
        let report = install_hook(&path, &exe_v2).unwrap();

        assert_eq!(report.action, InstallAction::UpdatedCommand);
        assert!(report.backup_path.is_some());

        let doc = read(&path);
        let se = doc["hooks"]["SessionEnd"].as_array().unwrap();
        assert_eq!(se.len(), 1, "still exactly one tallytape entry");
        assert_eq!(
            se[0]["hooks"][0]["command"].as_str().unwrap(),
            exe_v2.to_string_lossy()
        );
    }

    #[test]
    fn install_creates_backup_with_prior_bytes() {
        let (_d, path) = td();
        let initial_text = "{\n  \"hooks\": {},\n  \"theme\": \"dark\"\n}\n";
        fs::write(&path, initial_text).unwrap();

        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");
        let report = install_hook(&path, &exe).unwrap();

        let backup_path = report.backup_path.expect("backup expected on Added");
        let backup_text = fs::read_to_string(&backup_path).unwrap();
        assert_eq!(backup_text, initial_text);

        // And the patched file preserves the unrelated `theme` key.
        assert_eq!(read(&path)["theme"], "dark");
    }

    #[test]
    fn install_with_existing_hooks_no_session_end_adds_one() {
        let (_d, path) = td();
        let initial = json!({ "hooks": { "PreToolUse": [] } });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");
        install_hook(&path, &exe).unwrap();

        let doc = read(&path);
        assert!(doc["hooks"]["PreToolUse"].is_array(), "other key preserved");
        assert!(doc["hooks"]["SessionEnd"].is_array(), "SessionEnd created");
    }

    #[test]
    fn install_returns_error_on_invalid_json() {
        let (_d, path) = td();
        fs::write(&path, "{ not json").unwrap();
        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");

        let err = install_hook(&path, &exe).unwrap_err();
        assert!(
            err.to_string().contains("not valid JSON"),
            "should refuse to clobber: {err}"
        );
    }

    #[test]
    fn install_detects_unix_exe_suffix() {
        // Identification matches `tallytape-writer.exe` and other suffixed
        // variants as long as the basename starts with `tallytape-writer`.
        let (_d, path) = td();
        let initial = json!({
            "hooks": {
                "SessionEnd": [
                    {
                        "matcher": "",
                        "hooks": [
                            { "type": "command", "command": "/usr/local/bin/tallytape-writer.exe" }
                        ]
                    }
                ]
            }
        });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let exe = PathBuf::from("/new/tallytape-writer");
        let report = install_hook(&path, &exe).unwrap();
        assert_eq!(
            report.action,
            InstallAction::UpdatedCommand,
            "must recognise suffixed entry as tallytape's, not append a duplicate"
        );
    }

    #[test]
    fn uninstall_removes_only_tallytape_entry_keeps_others() {
        let (_d, path) = td();
        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");
        install_hook(&path, &exe).unwrap();
        // Add a foreign hook alongside ours.
        let mut doc = read(&path);
        doc["hooks"]["SessionEnd"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "matcher": "",
                "hooks": [{ "type": "command", "command": "/usr/bin/other" }]
            }));
        fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

        let report = uninstall_hook(&path).unwrap();
        assert!(report.removed);
        assert!(report.backup_path.is_some());

        let after = read(&path);
        let se = after["hooks"]["SessionEnd"].as_array().unwrap();
        assert_eq!(se.len(), 1);
        assert_eq!(se[0]["hooks"][0]["command"], "/usr/bin/other");
    }

    #[test]
    fn uninstall_cleans_empty_session_end_and_hooks() {
        let (_d, path) = td();
        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");
        install_hook(&path, &exe).unwrap();

        let report = uninstall_hook(&path).unwrap();
        assert!(report.removed);

        let after = read(&path);
        assert!(
            after.get("hooks").is_none(),
            "fully-empty hooks tree should be removed"
        );
    }

    #[test]
    fn uninstall_keeps_other_hook_types_when_session_end_was_only_ours() {
        let (_d, path) = td();
        let initial = json!({
            "hooks": {
                "PreToolUse": [{ "matcher": "", "hooks": [] }],
                "SessionEnd": [
                    {
                        "matcher": "",
                        "hooks": [{ "type": "command", "command": "/opt/tallytape-writer" }]
                    }
                ]
            }
        });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        uninstall_hook(&path).unwrap();
        let after = read(&path);
        assert!(after["hooks"]["PreToolUse"].is_array());
        assert!(after["hooks"].get("SessionEnd").is_none());
    }

    #[test]
    fn uninstall_is_noop_when_tallytape_entry_absent() {
        let (_d, path) = td();
        let initial = json!({
            "hooks": {
                "SessionEnd": [
                    {
                        "matcher": "",
                        "hooks": [{ "type": "command", "command": "/usr/bin/other-tool" }]
                    }
                ]
            }
        });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let report = uninstall_hook(&path).unwrap();
        assert!(!report.removed);
        assert!(report.backup_path.is_none(), "NoOp must not write backup");
    }

    #[test]
    fn uninstall_is_noop_when_settings_file_absent() {
        let (_d, path) = td();
        let report = uninstall_hook(&path).unwrap();
        assert!(!report.removed);
        assert!(report.backup_path.is_none());
        assert!(!path.exists(), "must not create an empty settings.json");
    }

    #[test]
    fn install_then_uninstall_then_install_roundtrip() {
        let (_d, path) = td();
        let exe = PathBuf::from("/opt/tallytape/tallytape-writer");

        let r1 = install_hook(&path, &exe).unwrap();
        assert_eq!(r1.action, InstallAction::Created);

        let u1 = uninstall_hook(&path).unwrap();
        assert!(u1.removed);

        let r2 = install_hook(&path, &exe).unwrap();
        assert_eq!(
            r2.action,
            InstallAction::Added,
            "file exists but no tallytape entry ⇒ Added (not Created)"
        );
    }
}
