use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct FarmArgs {
    #[command(subcommand)]
    pub command: FarmCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmCommand {
    Create(Box<FarmCreateArgs>),
    Update(FarmUpdateArgs),
    Publish,
    Get,
    List,
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
pub struct FarmUpdateArgs {
    #[arg(long)]
    pub field: Option<String>,
    #[arg(long)]
    pub value: Option<String>,
}
