use std::path::PathBuf;

use clap::{ArgAction, Args, Subcommand, ValueEnum};

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
    Fulfillment(OrderFulfillmentArgs),
    Receipt(OrderReceiptArgs),
    Payment(OrderPaymentArgs),
    Settlement(OrderSettlementArgs),
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
pub struct OrderFulfillmentArgs {
    #[command(subcommand)]
    pub command: OrderFulfillmentCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderFulfillmentCommand {
    Update(OrderFulfillmentUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderFulfillmentUpdateArgs {
    pub order_id: Option<String>,
    #[arg(long, value_enum)]
    pub state: Option<OrderFulfillmentStateArg>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum OrderFulfillmentStateArg {
    Preparing,
    ReadyForPickup,
    OutForDelivery,
    Delivered,
    SellerCancelled,
}

impl OrderFulfillmentStateArg {
    pub const fn as_protocol_state(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::ReadyForPickup => "ready_for_pickup",
            Self::OutForDelivery => "out_for_delivery",
            Self::Delivered => "delivered",
            Self::SellerCancelled => "seller_cancelled",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct OrderReceiptArgs {
    #[command(subcommand)]
    pub command: OrderReceiptCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderReceiptCommand {
    Record(OrderReceiptRecordArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderReceiptRecordArgs {
    pub order_id: Option<String>,
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "issue")]
    pub received: bool,
    #[arg(long)]
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderPaymentArgs {
    #[command(subcommand)]
    pub command: OrderPaymentCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderPaymentCommand {
    Record(OrderPaymentRecordArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderPaymentRecordArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub amount: Option<String>,
    #[arg(long)]
    pub currency: Option<String>,
    #[arg(long)]
    pub method: Option<String>,
    #[arg(long)]
    pub reference: Option<String>,
    #[arg(long)]
    pub paid_at: Option<u64>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderSettlementArgs {
    #[command(subcommand)]
    pub command: OrderSettlementCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderSettlementCommand {
    Accept(OrderSettlementAcceptArgs),
    Reject(OrderSettlementRejectArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderSettlementAcceptArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub payment_event_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderSettlementRejectArgs {
    pub order_id: Option<String>,
    #[arg(long)]
    pub payment_event_id: Option<String>,
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
