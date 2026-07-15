use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub command: ProfileCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProfileCommand {
    Inspect,
    Reset,
}
