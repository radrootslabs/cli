use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum ConfigCommand {
    Get,
}
