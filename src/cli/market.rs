use clap::{Args, Subcommand};

use crate::cli::listing::LookupArgs;

#[derive(Debug, Clone, Args)]
pub struct MarketArgs {
    #[command(subcommand)]
    pub command: MarketCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketCommand {
    Refresh,
    Product(MarketProductArgs),
    Listing(MarketListingArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MarketProductArgs {
    #[command(subcommand)]
    pub command: MarketProductCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketProductCommand {
    Search(QueryArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MarketListingArgs {
    #[command(subcommand)]
    pub command: MarketListingCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketListingCommand {
    Get(LookupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct QueryArgs {
    pub query: Vec<String>,
}
