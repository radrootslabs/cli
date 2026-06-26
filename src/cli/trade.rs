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
    Revision(TradeRevisionArgs),
    Status(TradeStatusArgs),
    Event(TradeEventArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeSubmitArgs {
    pub trade_id: Option<String>,
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
}

#[derive(Debug, Clone, Args)]
pub struct TradeCancelArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeRevisionArgs {
    #[command(subcommand)]
    pub command: TradeRevisionCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeRevisionCommand {
    Propose(TradeRevisionProposeArgs),
    Accept(TradeRevisionDecisionArgs),
    Decline(TradeRevisionDeclineArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeRevisionProposeArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long)]
    pub bin_id: Option<String>,
    #[arg(long)]
    pub bin_count: Option<u32>,
    #[arg(long)]
    pub adjustment_id: Option<String>,
    #[arg(long)]
    pub adjustment_effect: Option<String>,
    #[arg(long)]
    pub adjustment_amount: Option<String>,
    #[arg(long)]
    pub adjustment_currency: Option<String>,
    #[arg(long)]
    pub adjustment_reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeRevisionDecisionArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub revision_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeRevisionDeclineArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub revision_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
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
