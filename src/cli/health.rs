use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct HealthArgs {
    #[command(subcommand)]
    pub command: HealthCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum HealthCommand {
    Inspect,
}
