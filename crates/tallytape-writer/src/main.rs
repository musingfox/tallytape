mod cli;
mod error_log;
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

/// Parse the hook payload and run load_session + persist.
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

fn main() -> ExitCode {
    let args = Cli::parse();
    match args.resolved_command() {
        Command::Run => {
            let mut sid: Option<String> = None;
            if let Err(e) = run_hook(&mut sid) {
                if let Ok(path) = tallytape_core::log_path() {
                    error_log::write_log(&path, sid.as_deref(), &e);
                }
            }
            ExitCode::SUCCESS
        }
        Command::InstallHook => {
            eprintln!("install-hook: not implemented yet (see p2-6)");
            ExitCode::FAILURE
        }
    }
}
