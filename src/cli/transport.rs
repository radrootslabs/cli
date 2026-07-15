use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Clone, Args)]
pub struct TransportArgs {
    #[command(subcommand)]
    pub command: TransportCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportCommand {
    Capability(TransportCapabilityArgs),
    Config(TransportConfigArgs),
    Status(TransportStatusArgs),
    Delivery(TransportDeliveryArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TransportCapabilityArgs {
    #[command(subcommand)]
    pub command: TransportCapabilityCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportCapabilityCommand {
    List,
}

#[derive(Debug, Clone, Args)]
pub struct TransportConfigArgs {
    #[command(subcommand)]
    pub command: TransportConfigCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportConfigCommand {
    Inspect,
    Update(TransportConfigUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TransportStatusArgs {
    #[command(subcommand)]
    pub command: TransportStatusCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportStatusCommand {
    Inspect,
}

#[derive(Debug, Clone, Args)]
pub struct TransportDeliveryArgs {
    #[command(subcommand)]
    pub command: TransportDeliveryCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportDeliveryCommand {
    Inspect,
    Retry,
}

#[derive(Debug, Clone, Args)]
pub struct TransportConfigUpdateArgs {
    #[arg(long = "kind", value_enum)]
    pub kind: TransportProfileKindArg,
    #[arg(long = "nostr-relay")]
    pub nostr_relay: Vec<String>,
    #[arg(long = "reticulum-behavior", value_enum)]
    pub reticulum_behavior: Option<ReticulumBehaviorArg>,
    #[arg(long = "reticulum-scope")]
    pub reticulum_scope: Option<String>,
    #[arg(long = "reticulum-agent-endpoint")]
    pub reticulum_agent_endpoint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportProfileKindArg {
    LocalOnly,
    Nostr,
    Reticulum,
    MultiTarget,
}

impl TransportProfileKindArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::Nostr => "nostr",
            Self::Reticulum => "reticulum",
            Self::MultiTarget => "multi_target",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReticulumBehaviorArg {
    RejectDeliveryAttempts,
    DeferDeliveryPlans,
}

impl ReticulumBehaviorArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RejectDeliveryAttempts => "reject_delivery_attempts",
            Self::DeferDeliveryPlans => "defer_delivery_plans",
        }
    }
}
