use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct DiagnosticsArgs {
    #[command(subcommand)]
    pub command: DiagnosticsCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum DiagnosticsCommand {
    Inspect,
}
