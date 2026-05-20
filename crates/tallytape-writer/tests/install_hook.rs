//! CLI integration tests for `install-hook` / `uninstall-hook` (p6-7).
//!
//! Each test runs the real `tallytape-writer` binary with `HOME` set to a
//! tempdir so the hook patcher operates on a sandboxed
//! `<HOME>/.claude/settings.json`.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn writer_bin() -> &'static str {
    env!("CARGO_BIN_EXE_tallytape-writer")
}

fn read_json(path: &PathBuf) -> Value {
    let text = std::fs::read_to_string(path).expect("settings.json should be readable");
    serde_json::from_str(&text).expect("settings.json should be valid JSON")
}

fn run(cmd: &str, home: &TempDir) -> std::process::Output {
    Command::new(writer_bin())
        .arg(cmd)
        .env("HOME", home.path())
        .output()
        .expect("failed to spawn tallytape-writer")
}

#[test]
fn install_hook_creates_settings_with_tallytape_entry() {
    let home = tempfile::tempdir().unwrap();
    let settings = home.path().join(".claude").join("settings.json");

    let output = run("install-hook", &home);
    assert!(
        output.status.success(),
        "expected exit 0, got {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(settings.exists(), "settings.json should have been created");
    let doc = read_json(&settings);
    let entries = doc["hooks"]["SessionEnd"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["matcher"], "");
    let command = entries[0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(command, writer_bin(), "command must be the running binary");
}

#[test]
fn install_hook_is_idempotent_when_invoked_twice() {
    let home = tempfile::tempdir().unwrap();
    let settings = home.path().join(".claude").join("settings.json");

    let first = run("install-hook", &home);
    assert!(first.status.success());
    let after_first = std::fs::read(&settings).unwrap();

    let second = run("install-hook", &home);
    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("unchanged"),
        "second install should be a no-op, stdout was: {stdout}"
    );

    let after_second = std::fs::read(&settings).unwrap();
    assert_eq!(
        after_first, after_second,
        "no-op install must not modify the file"
    );
}

#[test]
fn install_hook_preserves_existing_unrelated_settings_and_writes_backup() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings = claude_dir.join("settings.json");
    let initial = "{\n  \"theme\": \"solarized\"\n}\n";
    std::fs::write(&settings, initial).unwrap();

    let output = run("install-hook", &home);
    assert!(output.status.success());

    // Backup carries the prior bytes verbatim.
    let backup = claude_dir.join("settings.json.bak");
    assert!(backup.exists(), "backup file should exist after modification");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), initial);

    // Patched settings preserves the unrelated key and adds the hook.
    let doc = read_json(&settings);
    assert_eq!(doc["theme"], "solarized");
    assert!(doc["hooks"]["SessionEnd"].is_array());
}

#[test]
fn uninstall_hook_removes_only_tallytape_entry() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings = claude_dir.join("settings.json");

    // First install ours, then prepend a foreign entry, then uninstall.
    assert!(run("install-hook", &home).status.success());
    let mut doc = read_json(&settings);
    doc["hooks"]["SessionEnd"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            serde_json::json!({
                "matcher": "",
                "hooks": [{ "type": "command", "command": "/usr/bin/foreign" }]
            }),
        );
    std::fs::write(&settings, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let output = run("uninstall-hook", &home);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("removed tallytape entry"),
        "stdout: {stdout}"
    );

    let after = read_json(&settings);
    let entries = after["hooks"]["SessionEnd"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["hooks"][0]["command"], "/usr/bin/foreign",
        "foreign hook must be preserved"
    );
}

#[test]
fn uninstall_hook_is_noop_when_settings_absent() {
    let home = tempfile::tempdir().unwrap();
    let output = run("uninstall-hook", &home);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no-op"), "stdout: {stdout}");

    let settings = home.path().join(".claude").join("settings.json");
    assert!(
        !settings.exists(),
        "uninstall must not create an empty settings.json"
    );
}
