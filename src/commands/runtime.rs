use crate::domain::runtime::{
    AccountRuntimeView, ConfigFilesRuntimeView, ConfigShowView, LoggingRuntimeView, MycRuntimeView,
    OutputRuntimeView, PathsRuntimeView, SignerRuntimeView,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;

pub fn show(config: &RuntimeConfig, logging: &LoggingState) -> ConfigShowView {
    ConfigShowView {
        source: "local runtime state".to_owned(),
        output: OutputRuntimeView {
            format: config.output.format.as_str().to_owned(),
            verbosity: config.output.verbosity.as_str().to_owned(),
            color: config.output.color,
            dry_run: config.output.dry_run,
        },
        config_files: ConfigFilesRuntimeView {
            user_present: config.paths.user_config_path.exists(),
            workspace_present: config.paths.workspace_config_path.exists(),
        },
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
            selector: config.account.selector.clone(),
            store_path: config.account.store_path.display().to_string(),
            secrets_dir: config.account.secrets_dir.display().to_string(),
            legacy_identity_path: config.identity.path.display().to_string(),
        },
        signer: SignerRuntimeView {
            backend: config.signer.backend.as_str().to_owned(),
        },
        myc: MycRuntimeView {
            executable: config.myc.executable.display().to_string(),
        },
    }
}
