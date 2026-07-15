use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Clone, Args)]
pub struct TransportArgs {
    #[command(subcommand)]
    pub command: TransportCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportCommand {
    Profile(TransportProfileArgs),
    Status,
    Outbox(TransportOutboxArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TransportProfileArgs {
    #[command(subcommand)]
    pub command: TransportProfileCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TransportProfileCommand {
    Get,
    Set(TransportProfileSetArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TransportProfileSetArgs {
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

#[derive(Debug, Clone, Args)]
pub struct TransportOutboxArgs {
    #[command(subcommand)]
    pub command: TransportOutboxCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum TransportOutboxCommand {
    Status,
    Push,
}
