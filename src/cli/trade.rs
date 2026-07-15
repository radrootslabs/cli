use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct TradeArgs {
    #[command(subcommand)]
    pub command: TradeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeCommand {
    Request(TradeRequestArgs),
    Get(TradeKeyArgs),
    List,
    Accept(TradeKeyArgs),
    Decline(TradeDeclineArgs),
    Cancel(TradeCancelArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeRequestArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub confirm_public_note: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TradeKeyArgs {
    pub trade_id: Option<String>,
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
