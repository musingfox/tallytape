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
    /// Print Claude Code settings.json snippet for hook installation.
    InstallHook,
    /// Internal: background worker that performs the actual ingest.
    #[command(hide = true, name = "__worker")]
    Worker,
}

impl Cli {
    pub fn resolved_command(&self) -> Command {
        match &self.command {
            Some(Command::InstallHook) => Command::InstallHook,
            Some(Command::Worker) => Command::Worker,
            _ => Command::Run,
        }
    }
}
