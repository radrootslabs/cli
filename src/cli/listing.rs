use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct ListingArgs {
    #[command(subcommand)]
    pub command: ListingCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ListingCommand {
    Create(Box<ListingCreateArgs>),
    Update(FileArgs),
    Publish(FileArgs),
    Pause(FileArgs),
    Withdraw(FileArgs),
    Get(LookupArgs),
    List,
}

#[derive(Debug, Clone, Args)]
pub struct ListingCreateArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub key: Option<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub category: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "bin-id")]
    pub bin_id: Option<String>,
    #[arg(long = "quantity-amount")]
    pub quantity_amount: Option<String>,
    #[arg(long = "quantity-unit")]
    pub quantity_unit: Option<String>,
    #[arg(long = "price-amount")]
    pub price_amount: Option<String>,
    #[arg(long = "price-currency")]
    pub price_currency: Option<String>,
    #[arg(long = "price-per-amount")]
    pub price_per_amount: Option<String>,
    #[arg(long = "price-per-unit")]
    pub price_per_unit: Option<String>,
    #[arg(long)]
    pub available: Option<String>,
    #[arg(long)]
    pub label: Option<String>,
    #[arg(long = "discount-id")]
    pub discount_id: Option<String>,
    #[arg(long = "discount-label")]
    pub discount_label: Option<String>,
    #[arg(long = "discount-kind")]
    pub discount_kind: Option<String>,
    #[arg(long = "discount-value")]
    pub discount_value: Option<String>,
    #[arg(long = "discount-amount")]
    pub discount_amount: Option<String>,
    #[arg(long = "discount-currency")]
    pub discount_currency: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FileArgs {
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct LookupArgs {
    pub key: Option<String>,
}
