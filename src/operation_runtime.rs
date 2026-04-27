use serde::Serialize;
use serde_json::Value;

use crate::operation_adapter::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, RelayListRequest, RelayListResult,
    SignerStatusGetRequest, SignerStatusGetResult, SyncPullRequest, SyncPullResult,
    SyncPushRequest, SyncPushResult, SyncStatusGetRequest, SyncStatusGetResult, SyncWatchRequest,
    SyncWatchResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime_args::SyncWatchArgs;

pub struct RuntimeOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> RuntimeOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<SignerStatusGetRequest> for RuntimeOperationService<'_> {
    type Result = SignerStatusGetResult;

    fn execute(
        &self,
        _request: OperationRequest<SignerStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::signer::resolve_signer_status(self.config);
        serialized_operation_result::<SignerStatusGetResult, _>(&view)
    }
}

impl OperationService<RelayListRequest> for RuntimeOperationService<'_> {
    type Result = RelayListResult;

    fn execute(
        &self,
        _request: OperationRequest<RelayListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::network::relay_list(self.config);
        serialized_operation_result::<RelayListResult, _>(&view)
    }
}

impl OperationService<SyncStatusGetRequest> for RuntimeOperationService<'_> {
    type Result = SyncStatusGetResult;

    fn execute(
        &self,
        _request: OperationRequest<SyncStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::sync::status(self.config))?;
        serialized_operation_result::<SyncStatusGetResult, _>(&view)
    }
}

impl OperationService<SyncPullRequest> for RuntimeOperationService<'_> {
    type Result = SyncPullResult;

    fn execute(
        &self,
        _request: OperationRequest<SyncPullRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::sync::pull(self.config))?;
        serialized_operation_result::<SyncPullResult, _>(&view)
    }
}

impl OperationService<SyncPushRequest> for RuntimeOperationService<'_> {
    type Result = SyncPushResult;

    fn execute(
        &self,
        _request: OperationRequest<SyncPushRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::sync::push(self.config))?;
        serialized_operation_result::<SyncPushResult, _>(&view)
    }
}

impl OperationService<SyncWatchRequest> for RuntimeOperationService<'_> {
    type Result = SyncWatchResult;

    fn execute(
        &self,
        request: OperationRequest<SyncWatchRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = SyncWatchArgs {
            frames: usize_input(&request, "frames").unwrap_or(1),
            interval_ms: u64_input(&request, "interval_ms").unwrap_or(1_000),
        };
        let view = map_runtime(crate::runtime::sync::watch(self.config, &args))?;
        serialized_operation_result::<SyncWatchResult, _>(&view)
    }
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
}

fn usize_input<P>(request: &OperationRequest<P>, key: &str) -> Option<usize>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn u64_input<P>(request: &OperationRequest<P>, key: &str) -> Option<u64>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_u64)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use tempfile::tempdir;

    use super::RuntimeOperationService;
    use crate::operation_adapter::{
        OperationAdapter, OperationContext, OperationRequest, RelayListRequest,
        SignerStatusGetRequest, SyncStatusGetRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn runtime_service_backs_signer_and_relay_status() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.test".into()]);
        let service = OperationAdapter::new(RuntimeOperationService::new(&config));

        let signer = OperationRequest::new(
            OperationContext::default(),
            SignerStatusGetRequest::default(),
        )
        .expect("signer status request");
        let signer_envelope = service
            .execute(signer)
            .expect("signer status result")
            .to_envelope(OperationContext::default().envelope_context("req_signer"))
            .expect("signer envelope");
        assert_eq!(signer_envelope.operation_id, "signer.status.get");
        assert_eq!(signer_envelope.result["state"], "unconfigured");

        let relay = OperationRequest::new(OperationContext::default(), RelayListRequest::default())
            .expect("relay list request");
        let relay_envelope = service
            .execute(relay)
            .expect("relay list result")
            .to_envelope(OperationContext::default().envelope_context("req_relay"))
            .expect("relay envelope");
        assert_eq!(relay_envelope.operation_id, "relay.list");
        assert_eq!(relay_envelope.result["state"], "configured");
        assert_eq!(relay_envelope.result["count"], 1);
    }

    #[test]
    fn runtime_service_backs_sync_status() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), Vec::new());
        let service = OperationAdapter::new(RuntimeOperationService::new(&config));

        let sync =
            OperationRequest::new(OperationContext::default(), SyncStatusGetRequest::default())
                .expect("sync status request");
        let sync_envelope = service
            .execute(sync)
            .expect("sync status result")
            .to_envelope(OperationContext::default().envelope_context("req_sync"))
            .expect("sync envelope");
        assert_eq!(sync_envelope.operation_id, "sync.status.get");
        assert_eq!(sync_envelope.result["state"], "unconfigured");
    }

    fn sample_config(root: &Path, relays: Vec<String>) -> RuntimeConfig {
        let data = root.join("data");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Normal,
                color: true,
                dry_run: false,
            },
            interaction: InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: false,
                stdout_tty: false,
                prompts_allowed: false,
                confirmations_allowed: false,
            },
            paths: PathsConfig {
                profile: "interactive_user".into(),
                profile_source: "test".into(),
                allowed_profiles: vec!["interactive_user".into(), "repo_local".into()],
                root_source: "test".into(),
                repo_local_root: None,
                repo_local_root_source: None,
                subordinate_path_override_source: "runtime_config".into(),
                app_namespace: "apps/cli".into(),
                shared_accounts_namespace: "shared/accounts".into(),
                shared_identities_namespace: "shared/identities".into(),
                app_config_path: root.join("config/apps/cli/config.toml"),
                workspace_config_path: None,
                app_data_root: data.join("apps/cli"),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
            },
            migration: MigrationConfig {
                report: RadrootsMigrationReport::empty(),
            },
            logging: LoggingConfig {
                filter: "info".into(),
                directory: None,
                stdout: false,
            },
            account: AccountConfig {
                selector: None,
                store_path: data.join("shared/accounts/store.json"),
                secrets_dir: secrets.join("shared/accounts"),
                secret_backend: RadrootsSecretBackend::EncryptedFile,
                secret_fallback: None,
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
                default_fallback: Some("encrypted_file".into()),
                allowed_backends: vec!["host_vault".into(), "encrypted_file".into()],
                host_vault_policy: Some("desktop".into()),
                uses_protected_store: true,
            },
            identity: IdentityConfig {
                path: secrets.join("shared/identities/default.json"),
            },
            signer: SignerConfig {
                backend: SignerBackend::Local,
            },
            relay: RelayConfig {
                urls: relays,
                publish_policy: RelayPublishPolicy::Any,
                source: RelayConfigSource::Defaults,
            },
            local: LocalConfig {
                root: data.join("apps/cli/replica"),
                replica_db_path: data.join("apps/cli/replica/replica.sqlite"),
                backups_dir: data.join("apps/cli/replica/backups"),
                exports_dir: data.join("apps/cli/replica/exports"),
            },
            myc: MycConfig {
                executable: PathBuf::from("myc"),
                status_timeout_ms: 2_000,
            },
            hyf: HyfConfig {
                enabled: false,
                executable: PathBuf::from("hyfd"),
            },
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
                bridge_bearer_token: None,
            },
            capability_bindings: Vec::new(),
        }
    }
}
