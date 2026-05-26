use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct ValidationArgs {
    #[command(subcommand)]
    pub command: ValidationCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ValidationCommand {
    Receipt(ValidationReceiptArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ValidationReceiptArgs {
    #[command(subcommand)]
    pub command: ValidationReceiptCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ValidationReceiptCommand {
    Get(ValidationReceiptEventArgs),
    List(ValidationReceiptListArgs),
    Verify(ValidationReceiptEventArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ValidationReceiptEventArgs {
    pub receipt_event_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ValidationReceiptListArgs {
    #[arg(long)]
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct PathOutputArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
}
