use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct TradeArgs {
    #[command(subcommand)]
    pub command: TradeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeCommand {
    Submit(TradeSubmitArgs),
    Get(TradeKeyArgs),
    List,
    App(TradeAppArgs),
    Rebind(TradeRebindArgs),
    Accept(TradeKeyArgs),
    Decline(TradeDeclineArgs),
    Cancel(TradeCancelArgs),
    Status(TradeStatusArgs),
    Event(TradeEventArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeSubmitArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub confirm_public_note: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TradeKeyArgs {
    pub trade_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeAppArgs {
    #[command(subcommand)]
    pub command: TradeAppCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeAppCommand {
    List,
    Export(TradeAppExportArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeAppExportArgs {
    pub record_id: Option<String>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeRebindArgs {
    pub trade_id: Option<String>,
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeDeclineArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long)]
    pub confirm_public_note: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TradeCancelArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long)]
    pub confirm_public_note: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TradeStatusArgs {
    #[command(subcommand)]
    pub command: TradeStatusCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeStatusCommand {
    Get(TradeKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeEventArgs {
    #[command(subcommand)]
    pub command: TradeEventCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeEventCommand {
    List(TradeKeyArgs),
    Watch(TradeKeyArgs),
}
