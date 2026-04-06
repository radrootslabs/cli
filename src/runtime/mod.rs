pub mod config;
pub mod identity;
pub mod logging;
pub mod signer;

use std::process::ExitCode;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("{0}")]
    Config(String),
    #[error("failed to initialize logging: {0}")]
    Logging(#[from] radroots_log::Error),
    #[error("identity error: {0}")]
    Identity(#[from] radroots_identity::IdentityError),
    #[error("failed to serialize json output: {0}")]
    Json(#[from] serde_json::Error),
    #[error("failed to write output: {0}")]
    Io(#[from] std::io::Error),
}

impl RuntimeError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Config(_) => ExitCode::from(2),
            Self::Logging(_) | Self::Identity(_) | Self::Json(_) | Self::Io(_) => ExitCode::from(1),
        }
    }
}
