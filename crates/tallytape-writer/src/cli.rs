use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "tallytape-writer", about = "tallytape session writer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read hook payload from stdin and process the session (default).
    Run,
    /// Patch `~/.claude/settings.json` to register tallytape's SessionEnd hook.
    /// Idempotent; creates a `.bak` before any modification.
    InstallHook,
    /// Remove tallytape's SessionEnd entry from `~/.claude/settings.json`.
    /// No-op if the entry is absent. Creates a `.bak` before any modification.
    UninstallHook,
    /// Internal: background worker that performs the actual ingest.
    #[command(hide = true, name = "__worker")]
    Worker,
}

impl Cli {
    pub fn resolved_command(&self) -> Command {
        match &self.command {
            Some(Command::InstallHook) => Command::InstallHook,
            Some(Command::UninstallHook) => Command::UninstallHook,
            Some(Command::Worker) => Command::Worker,
            _ => Command::Run,
        }
    }
}
