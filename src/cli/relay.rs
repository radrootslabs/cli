use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct RelayArgs {
    #[command(subcommand)]
    pub command: RelayCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum RelayCommand {
    List,
}
