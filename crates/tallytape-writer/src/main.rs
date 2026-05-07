mod cli;
mod detach;
mod error_log;
mod log_subscriber;
mod payload;
mod persist;
mod session_loader;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::payload::parse_payload;
use crate::session_loader::load_session;

fn claude_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude"))
}

/// Parse the hook payload and run load_session + persist synchronously.
/// Used by the `__worker` subcommand (child process).
///
/// Writes `*session_id` as soon as the payload is parsed so the caller can
/// include it in error log entries even when a later step fails.
fn run_hook(session_id: &mut Option<String>) -> anyhow::Result<()> {
    let payload = parse_payload(io::stdin().lock())?;
    *session_id = Some(payload.session_id.clone());

    let home = claude_home().unwrap_or_else(|| PathBuf::from(".claude"));
    let result = load_session(&payload, &home);

    let outcome = persist::persist(result)?;
    log::info!(
        "persisted session {} (inserted={}, dup={}, unknown_model={})",
        outcome.session_id,
        outcome.inserted,
        outcome.skipped_duplicates,
        outcome.skipped_unknown_model
    );
    Ok(())
}

/// Parse the hook payload from stdin synchronously, then spawn a detached
/// `__worker` child to perform the actual ingest. Parent always exits 0.
fn run_hook_fast(session_id: &mut Option<String>) -> anyhow::Result<()> {
    // Parse synchronously in parent — this is necessary to detect malformed
    // payloads (C5) without spawning a child, and is fast (just stdin read).
    let payload = parse_payload(io::stdin().lock())?;
    *session_id = Some(payload.session_id.clone());

    // Spawn detached child. Errors here are logged; parent still exits 0.
    detach::spawn_worker(&payload)?;
    Ok(())
}

fn main() -> ExitCode {
    let args = Cli::parse();
    match args.resolved_command() {
        Command::Run => {
            let _ = log_subscriber::init_logger();
            let mut sid: Option<String> = None;
            if let Err(e) = run_hook_fast(&mut sid) {
                if let Ok(path) = tallytape_core::log_path() {
                    error_log::write_log(&path, sid.as_deref(), &e);
                }
            }
            ExitCode::SUCCESS
        }
        Command::Worker => {
            let _ = log_subscriber::init_logger();
            let mut sid: Option<String> = None;
            if let Err(e) = run_hook(&mut sid) {
                if let Ok(path) = tallytape_core::log_path() {
                    error_log::write_log(&path, sid.as_deref(), &e);
                }
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Command::InstallHook => {
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("install-hook: failed to resolve current executable: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let snippet = serde_json::json!({
                "hooks": {
                    "SessionEnd": [
                        {
                            "matcher": "",
                            "hooks": [
                                { "type": "command", "command": exe.to_string_lossy() }
                            ]
                        }
                    ]
                }
            });
            println!("{}", serde_json::to_string_pretty(&snippet).expect("static json! value always serializes"));
            ExitCode::SUCCESS
        }
    }
}
