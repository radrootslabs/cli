use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct ValidationArgs {
    #[command(subcommand)]
    pub command: ValidationCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ValidationCommand {
    Status,
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
    Verify(ValidationReceiptEventArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ValidationReceiptEventArgs {
    pub receipt_event_id: Option<String>,
}
