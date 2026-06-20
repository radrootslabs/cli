use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct OrderArgs {
    #[command(subcommand)]
    pub command: OrderCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderCommand {
    Submit(OrderSubmitArgs),
    Get(OrderKeyArgs),
    List,
    App(OrderAppArgs),
    Rebind(OrderRebindArgs),
    Accept(OrderKeyArgs),
    Decline(OrderDeclineArgs),
    Cancel(OrderCancelArgs),
    Revision(OrderRevisionArgs),
    Status(OrderStatusArgs),
    Event(OrderEventArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderSubmitArgs {
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderKeyArgs {
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderAppArgs {
    #[command(subcommand)]
    pub command: OrderAppCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderAppCommand {
    List,
    Export(OrderAppExportArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderAppExportArgs {
    pub record_id: Option<String>,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderRebindArgs {
    pub order_id: Option<String>,
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderDeclineArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderCancelArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderRevisionArgs {
    #[command(subcommand)]
    pub command: OrderRevisionCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderRevisionCommand {
    Propose(OrderRevisionProposeArgs),
    Accept(OrderRevisionDecisionArgs),
    Decline(OrderRevisionDeclineArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderRevisionProposeArgs {
    pub order_id: Option<String>,
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
pub struct OrderRevisionDecisionArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub revision_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderRevisionDeclineArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub revision_id: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderStatusArgs {
    #[command(subcommand)]
    pub command: OrderStatusCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderStatusCommand {
    Get(OrderKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderEventArgs {
    #[command(subcommand)]
    pub command: OrderEventCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderEventCommand {
    List(OrderKeyArgs),
    Watch(OrderKeyArgs),
}
