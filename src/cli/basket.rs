use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct BasketArgs {
    #[command(subcommand)]
    pub command: BasketCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketCommand {
    Create(BasketCreateArgs),
    Get(BasketKeyArgs),
    List,
    Item(BasketItemArgs),
    Adjustment(BasketAdjustmentArgs),
    Validate(BasketKeyArgs),
    Quote(BasketQuoteArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BasketCreateArgs {
    pub basket_id: Option<String>,
    #[arg(long)]
    pub listing: Option<String>,
    #[arg(long = "listing-addr")]
    pub listing_addr: Option<String>,
    #[arg(long = "bin-id")]
    pub bin_id: Option<String>,
    #[arg(long)]
    pub quantity: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketKeyArgs {
    pub basket_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketItemArgs {
    #[command(subcommand)]
    pub command: BasketItemCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketItemCommand {
    Add(BasketItemMutationArgs),
    Update(BasketItemMutationArgs),
    Remove(BasketItemRemoveArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BasketAdjustmentArgs {
    #[command(subcommand)]
    pub command: BasketAdjustmentCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketAdjustmentCommand {
    Add(BasketAdjustmentAddArgs),
    Remove(BasketAdjustmentRemoveArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BasketAdjustmentAddArgs {
    pub basket_id: Option<String>,
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub effect: Option<String>,
    #[arg(long)]
    pub amount: Option<String>,
    #[arg(long)]
    pub currency: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketAdjustmentRemoveArgs {
    pub basket_id: Option<String>,
    #[arg(long)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketItemMutationArgs {
    pub basket_id: Option<String>,
    #[arg(long = "item-id")]
    pub item_id: Option<String>,
    #[arg(long)]
    pub listing: Option<String>,
    #[arg(long = "listing-addr")]
    pub listing_addr: Option<String>,
    #[arg(long = "bin-id")]
    pub bin_id: Option<String>,
    #[arg(long)]
    pub quantity: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketItemRemoveArgs {
    pub basket_id: Option<String>,
    pub item_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketQuoteArgs {
    #[command(subcommand)]
    pub command: BasketQuoteCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketQuoteCommand {
    Create(BasketKeyArgs),
}
