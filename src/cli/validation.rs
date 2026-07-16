use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct ValidationArgs {
    #[command(subcommand)]
    pub command: ValidationCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ValidationCommand {
    Status,
}
