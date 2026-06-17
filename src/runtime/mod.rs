pub mod account;
pub mod config;
pub mod direct_relay;
pub mod farm;
pub mod farm_config;
pub mod find;
pub mod hyf;
pub mod listing;
pub mod local_events;
pub mod logging;
pub mod network;
pub mod order;
pub mod paths;
pub mod provider;
pub mod sdk;
pub mod signer;
pub mod store;
pub mod sync;
pub mod validation_receipt;

use std::process::ExitCode;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Account(#[from] account::AccountRuntimeFailure),
    #[error("failed to initialize logging: {0}")]
    Logging(#[from] radroots_log::Error),
    #[error("accounts error: {0}")]
    Accounts(#[from] radroots_nostr_accounts::prelude::RadrootsNostrAccountsError),
    #[error("replica sql error: {0}")]
    Sql(#[from] radroots_replica_db::SqlError),
    #[error("replica sync error: {0}")]
    ReplicaSync(#[from] radroots_replica_sync::RadrootsReplicaEventsError),
    #[error("local events error: {0}")]
    LocalEvents(#[from] radroots_local_events::LocalEventsError),
    #[error("network error: {0}")]
    Network(String),
    #[error("failed to serialize json output: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to write output: {0}")]
    Io(#[from] std::io::Error),
}

impl RuntimeError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Config(_) | Self::Account(_) => ExitCode::from(2),
            Self::Logging(_)
            | Self::Accounts(_)
            | Self::Sql(_)
            | Self::ReplicaSync(_)
            | Self::LocalEvents(_)
            | Self::Network(_)
            | Self::Json(_)
            | Self::Io(_) => ExitCode::from(1),
        }
    }
}
