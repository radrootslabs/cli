use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum WorkspaceCommand {
    Init,
    Get,
}
