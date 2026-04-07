use crate::domain::runtime::{
    AccountRuntimeView, ConfigShowView, LoggingRuntimeView, MycRuntimeView, PathsRuntimeView,
    SignerRuntimeView,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;

pub fn show(config: &RuntimeConfig, logging: &LoggingState) -> ConfigShowView {
    ConfigShowView {
        output_format: config.output_format.as_str().to_owned(),
        paths: PathsRuntimeView {
            user_config_path: config.paths.user_config_path.display().to_string(),
            workspace_config_path: config.paths.workspace_config_path.display().to_string(),
            user_state_root: config.paths.user_state_root.display().to_string(),
        },
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
        account: AccountRuntimeView {
            identity_path: config.identity.path.display().to_string(),
        },
        signer: SignerRuntimeView {
            backend: config.signer.backend.as_str().to_owned(),
        },
        myc: MycRuntimeView {
            executable: config.myc.executable.display().to_string(),
        },
    }
}
