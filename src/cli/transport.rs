use std::path::PathBuf;

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
    #[arg(long = "reticulum-preview-behavior", value_enum)]
    pub reticulum_preview_behavior: Option<ReticulumPreviewBehaviorArg>,
    #[arg(long = "proxy-url")]
    pub proxy_url: Option<String>,
    #[arg(long = "proxy-token-file", value_name = "PATH")]
    pub proxy_token_file: Option<PathBuf>,
    #[arg(long = "proxy-token-secret-id", value_name = "SECRET_ID")]
    pub proxy_token_secret_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportProfileKindArg {
    LocalOnly,
    Nostr,
    ReticulumPreview,
    Proxy,
}

impl TransportProfileKindArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::Nostr => "nostr",
            Self::ReticulumPreview => "reticulum_preview",
            Self::Proxy => "proxy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReticulumPreviewBehaviorArg {
    RejectDeliveryAttempts,
    DeferDeliveryPlans,
}

impl ReticulumPreviewBehaviorArg {
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
