use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: SyncCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SyncCommand {
    Status(SyncStatusArgs),
    Pull,
    Push,
    Watch,
}

#[derive(Debug, Clone, Args)]
pub struct SyncStatusArgs {
    #[command(subcommand)]
    pub command: SyncStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SyncStatusCommand {
    Get,
}
