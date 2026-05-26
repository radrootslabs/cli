use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct SignerArgs {
    #[command(subcommand)]
    pub command: SignerCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SignerCommand {
    Status(SignerStatusArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SignerStatusArgs {
    #[command(subcommand)]
    pub command: SignerStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SignerStatusCommand {
    Get,
}
