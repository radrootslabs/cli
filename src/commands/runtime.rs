use crate::domain::runtime::{
    IdentityRuntimeView, LoggingRuntimeView, MycRuntimeView, RuntimeShowView, SignerRuntimeView,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;

pub fn show(config: &RuntimeConfig, logging: &LoggingState) -> RuntimeShowView {
    RuntimeShowView {
        output_format: config.output_format.as_str().to_owned(),
        logging: LoggingRuntimeView {
            initialized: logging.initialized,
            filter: config.logging.filter.clone(),
            stdout: config.logging.stdout,
            directory: config
                .logging
                .directory
                .as_ref()
                .map(|path| path.display().to_string()),
            current_file: logging
                .current_file
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        identity: IdentityRuntimeView {
            path: config.identity.path.display().to_string(),
        },
        signer: SignerRuntimeView {
            backend: config.signer.backend.as_str().to_owned(),
        },
        myc: MycRuntimeView {
            executable: config.myc.executable.display().to_string(),
        },
    }
}
