use clap::{Args, Subcommand};

use crate::cli::listing::LookupArgs;

#[derive(Debug, Clone, Args)]
pub struct MarketArgs {
    #[command(subcommand)]
    pub command: MarketCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketCommand {
    Pull,
    Search(QueryArgs),
    Get(LookupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct QueryArgs {
    pub query: Vec<String>,
}
