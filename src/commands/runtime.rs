use crate::domain::runtime::{
    AccountRuntimeView, AccountSecretRuntimeView, ConfigFilesRuntimeView, ConfigShowView,
    LocalRuntimeView, LoggingRuntimeView, MycRuntimeView, OutputRuntimeView, PathsRuntimeView,
    RelayRuntimeView, RpcRuntimeView, SignerRuntimeView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;

pub fn show(
    config: &RuntimeConfig,
    logging: &LoggingState,
) -> Result<ConfigShowView, RuntimeError> {
    let secret_backend = crate::runtime::accounts::secret_backend_status(config);
    Ok(ConfigShowView {
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
            secret_backend: AccountSecretRuntimeView {
                configured_primary: secret_backend.configured_primary,
                configured_fallback: secret_backend.configured_fallback,
                state: secret_backend.state,
                active_backend: secret_backend.active_backend,
                used_fallback: secret_backend.used_fallback,
                reason: secret_backend.reason,
            },
        },
        signer: SignerRuntimeView {
            mode: config.signer.backend.as_str().to_owned(),
        },
        relay: RelayRuntimeView {
            count: config.relay.urls.len(),
            urls: config.relay.urls.clone(),
            publish_policy: config.relay.publish_policy.as_str().to_owned(),
            source: config.relay.source.as_str().to_owned(),
        },
        local: LocalRuntimeView {
            root: config.local.root.display().to_string(),
            replica_db_path: config.local.replica_db_path.display().to_string(),
            backups_dir: config.local.backups_dir.display().to_string(),
            exports_dir: config.local.exports_dir.display().to_string(),
        },
        myc: MycRuntimeView {
            executable: config.myc.executable.display().to_string(),
        },
        rpc: RpcRuntimeView {
            url: config.rpc.url.clone(),
            bridge_auth_configured: config.rpc.bridge_bearer_token.is_some(),
        },
    })
}
