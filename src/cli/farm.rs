use clap::{ArgGroup, Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct FarmArgs {
    #[command(subcommand)]
    pub command: FarmCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmCommand {
    Create(FarmCreateArgs),
    Get,
    Rebind(FarmRebindArgs),
    Profile(FarmProfileArgs),
    Location(FarmLocationArgs),
    Fulfillment(FarmFulfillmentArgs),
    Readiness(FarmReadinessArgs),
    Publish,
}

#[derive(Debug, Clone, Args)]
pub struct FarmCreateArgs {
    #[arg(long = "farm-d-tag")]
    pub farm_d_tag: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long = "display-name")]
    pub display_name: Option<String>,
    #[arg(long)]
    pub about: Option<String>,
    #[arg(long)]
    pub website: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub banner: Option<String>,
    #[arg(long)]
    pub location: Option<String>,
    #[arg(long)]
    pub city: Option<String>,
    #[arg(long)]
    pub region: Option<String>,
    #[arg(long)]
    pub country: Option<String>,
    #[arg(long)]
    pub geohash: Option<String>,
    #[arg(long = "delivery-method")]
    pub delivery_method: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmRebindArgs {
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmProfileArgs {
    #[command(subcommand)]
    pub command: FarmProfileCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmProfileCommand {
    Update(FarmProfileUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct FarmProfileUpdateArgs {
    #[arg(long)]
    pub field: Option<String>,
    #[arg(long)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmLocationArgs {
    #[command(subcommand)]
    pub command: FarmLocationCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmLocationCommand {
    Set(FarmLocationSetArgs),
    Get(FarmLocationKeyArgs),
    Clear(FarmLocationKeyArgs),
}

#[derive(Debug, Clone, Args)]
#[command(group(
    ArgGroup::new("location_mode")
        .args(["lat", "city", "query", "geonames_id"])
        .required(true)
        .multiple(false)
))]
pub struct FarmLocationSetArgs {
    #[arg(long, allow_negative_numbers = true, requires = "lng")]
    pub lat: Option<f64>,
    #[arg(long, allow_negative_numbers = true, requires = "lat")]
    pub lng: Option<f64>,
    #[arg(long = "farm-d-tag")]
    pub farm_d_tag: Option<String>,
    #[arg(long)]
    pub city: Option<String>,
    #[arg(long, requires = "city", conflicts_with_all = ["lat", "query", "geonames_id"])]
    pub region: Option<String>,
    #[arg(long, requires = "city", conflicts_with_all = ["lat", "query", "geonames_id"])]
    pub country: Option<String>,
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long = "geonames-id", value_parser = clap::value_parser!(i64).range(1..))]
    pub geonames_id: Option<i64>,
    #[arg(long)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmLocationKeyArgs {
    #[arg(long = "farm-d-tag")]
    pub farm_d_tag: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmFulfillmentArgs {
    #[command(subcommand)]
    pub command: FarmFulfillmentCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmFulfillmentCommand {
    Update(FarmFulfillmentUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct FarmFulfillmentUpdateArgs {
    #[arg(long)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmReadinessArgs {
    #[command(subcommand)]
    pub command: FarmReadinessCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum FarmReadinessCommand {
    Check,
}
