use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct StoreArgs {
    #[command(subcommand)]
    pub command: StoreCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum StoreCommand {
    Init,
    Status(StoreStatusArgs),
    Export,
    Backup(StoreBackupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct StoreStatusArgs {
    #[command(subcommand)]
    pub command: StoreStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum StoreStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct StoreBackupArgs {
    #[command(subcommand)]
    pub command: StoreBackupCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum StoreBackupCommand {
    Create,
}
