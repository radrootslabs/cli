use std::path::PathBuf;

use clap::{ArgAction, Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct StoreArgs {
    #[command(subcommand)]
    pub command: StoreCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum StoreCommand {
    Inspect,
    Backup,
    Restore(StoreRestoreArgs),
}

#[derive(Debug, Clone, Args)]
pub struct StoreRestoreArgs {
    pub source: PathBuf,
    #[arg(long = "destination")]
    pub destination: Option<PathBuf>,
    #[arg(long = "overwrite", action = ArgAction::SetTrue)]
    pub overwrite: bool,
}
