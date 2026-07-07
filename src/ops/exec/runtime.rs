use serde::Serialize;
use serde_json::Value;

use crate::cli::global::SyncWatchArgs;
use crate::ops::{
    MeshPolicyCheckRequest, MeshPolicyCheckResult, MeshScopeGetRequest, MeshScopeGetResult,
    MeshScopeSetRequest, MeshScopeSetResult, MeshStatusRequest, MeshStatusResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, SignerStatusGetRequest,
    SignerStatusGetResult, SyncPullRequest, SyncPullResult, SyncPushRequest, SyncPushResult,
    SyncStatusGetRequest, SyncStatusGetResult, SyncWatchRequest, SyncWatchResult,
    TransportOutboxPushRequest, TransportOutboxPushResult, TransportOutboxStatusRequest,
    TransportOutboxStatusResult, TransportProfileGetRequest, TransportProfileGetResult,
    TransportProfileSetRequest, TransportProfileSetResult, TransportStatusRequest,
    TransportStatusResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::view::runtime::{CommandDisposition, SyncActionView, SyncStatusView};

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

impl OperationService<TransportProfileGetRequest> for RuntimeOperationService<'_> {
    type Result = TransportProfileGetResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportProfileGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::profile(self.config);
        serialized_operation_result::<TransportProfileGetResult, _>(&view)
    }
}

impl OperationService<TransportProfileSetRequest> for RuntimeOperationService<'_> {
    type Result = TransportProfileSetResult;

    fn execute(
        &self,
        request: OperationRequest<TransportProfileSetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(
            "transport.profile.set",
            crate::runtime::transport::set_profile(self.config, request.payload.input()),
        )?;
        serialized_operation_result::<TransportProfileSetResult, _>(&view)
    }
}

impl OperationService<TransportStatusRequest> for RuntimeOperationService<'_> {
    type Result = TransportStatusResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportStatusRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::status(self.config);
        serialized_operation_result::<TransportStatusResult, _>(&view)
    }
}

impl OperationService<TransportOutboxStatusRequest> for RuntimeOperationService<'_> {
    type Result = TransportOutboxStatusResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportOutboxStatusRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::outbox_status(self.config).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure("transport.outbox.status", error)
        })?;
        serialized_operation_result::<TransportOutboxStatusResult, _>(&view)
    }
}

impl OperationService<TransportOutboxPushRequest> for RuntimeOperationService<'_> {
    type Result = TransportOutboxPushResult;

    fn execute(
        &self,
        request: OperationRequest<TransportOutboxPushRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                "transport.outbox.push",
            ));
        }
        let view = crate::runtime::transport::outbox_push(self.config).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure("transport.outbox.push", error)
        })?;
        serialized_operation_result::<TransportOutboxPushResult, _>(&view)
    }
}

impl OperationService<MeshScopeGetRequest> for RuntimeOperationService<'_> {
    type Result = MeshScopeGetResult;

    fn execute(
        &self,
        _request: OperationRequest<MeshScopeGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::mesh::scope(self.config);
        serialized_operation_result::<MeshScopeGetResult, _>(&view)
    }
}

impl OperationService<MeshScopeSetRequest> for RuntimeOperationService<'_> {
    type Result = MeshScopeSetResult;

    fn execute(
        &self,
        request: OperationRequest<MeshScopeSetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(
            "mesh.scope.set",
            crate::runtime::mesh::set_scope(self.config, request.payload.input()),
        )?;
        serialized_operation_result::<MeshScopeSetResult, _>(&view)
    }
}

impl OperationService<MeshStatusRequest> for RuntimeOperationService<'_> {
    type Result = MeshStatusResult;

    fn execute(
        &self,
        _request: OperationRequest<MeshStatusRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::mesh::status(self.config);
        serialized_operation_result::<MeshStatusResult, _>(&view)
    }
}

impl OperationService<MeshPolicyCheckRequest> for RuntimeOperationService<'_> {
    type Result = MeshPolicyCheckResult;

    fn execute(
        &self,
        _request: OperationRequest<MeshPolicyCheckRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::mesh::policy_check(self.config);
        serialized_operation_result::<MeshPolicyCheckResult, _>(&view)
    }
}

impl OperationService<SyncStatusGetRequest> for RuntimeOperationService<'_> {
    type Result = SyncStatusGetResult;

    fn execute(
        &self,
        _request: OperationRequest<SyncStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::sync::status(self.config).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure("sync.status.get", error)
        })?;
        sync_status_result(&view)
    }
}

impl OperationService<SyncPullRequest> for RuntimeOperationService<'_> {
    type Result = SyncPullResult;

    fn execute(
        &self,
        _request: OperationRequest<SyncPullRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime("sync.pull", crate::runtime::sync::pull(self.config))?;
        sync_action_result::<SyncPullResult>("sync.pull", &view)
    }
}

impl OperationService<SyncPushRequest> for RuntimeOperationService<'_> {
    type Result = SyncPushResult;

    fn execute(
        &self,
        request: OperationRequest<SyncPushRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required("sync.push"));
        }
        let view = crate::runtime::sync::push(self.config)
            .map_err(|error| OperationAdapterError::sdk_adapter_failure("sync.push", error))?;
        sync_action_result::<SyncPushResult>("sync.push", &view)
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
        let view = map_runtime(
            "sync.watch",
            crate::runtime::sync::watch(self.config, &args),
        )?;
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

fn sync_status_result(
    view: &SyncStatusView,
) -> Result<OperationResult<SyncStatusGetResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<SyncStatusGetResult, _>(view),
        disposition => Err(sync_view_error(
            "sync.status.get",
            disposition,
            view,
            view.reason.as_deref(),
        )),
    }
}

fn sync_action_result<R>(
    operation_id: &str,
    view: &SyncActionView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<R, _>(view),
        disposition => Err(sync_view_error(
            operation_id,
            disposition,
            view,
            view.reason.as_deref(),
        )),
    }
}

fn sync_view_error<T>(
    operation_id: &str,
    disposition: CommandDisposition,
    view: &T,
    reason: Option<&str>,
) -> OperationAdapterError
where
    T: Serialize,
{
    let detail = serde_json::to_value(view).unwrap_or_else(|_| Value::Object(Default::default()));
    let message = reason
        .map(str::to_owned)
        .unwrap_or_else(|| format!("`{operation_id}` is not ready"));
    match disposition {
        CommandDisposition::Unconfigured => {
            OperationAdapterError::operation_unavailable_with_detail(operation_id, message, detail)
        }
        CommandDisposition::ExternalUnavailable => {
            OperationAdapterError::network_unavailable_with_detail(operation_id, message, detail)
        }
        CommandDisposition::Unsupported => OperationAdapterError::InvalidInput {
            operation_id: operation_id.to_owned(),
            message,
        },
        CommandDisposition::ValidationFailed => OperationAdapterError::ValidationFailed {
            operation_id: operation_id.to_owned(),
            message,
        },
        CommandDisposition::NotFound => OperationAdapterError::NotFound {
            operation_id: operation_id.to_owned(),
            message,
        },
        CommandDisposition::InternalError | CommandDisposition::Success => {
            OperationAdapterError::Runtime(message)
        }
    }
}

fn map_runtime<T>(
    operation_id: &str,
    result: Result<T, RuntimeError>,
) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::runtime_failure(operation_id, error))
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
    use radroots_secret_vault::RadrootsSecretBackend;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    use super::RuntimeOperationService;
    use crate::ops::{
        OperationAdapter, OperationContext, OperationRequest, SignerStatusGetRequest,
        SyncStatusGetRequest, TransportProfileGetRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MycConfig, OutputConfig, OutputFormat, PathsConfig, RpcConfig,
        RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn runtime_service_backs_signer_and_transport_profile() {
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

        let profile = OperationRequest::new(
            OperationContext::default(),
            TransportProfileGetRequest::default(),
        )
        .expect("transport profile request");
        let profile_envelope = service
            .execute(profile)
            .expect("transport profile result")
            .to_envelope(OperationContext::default().envelope_context("req_transport"))
            .expect("transport profile envelope");
        assert_eq!(profile_envelope.operation_id, "transport.profile.get");
        assert_eq!(profile_envelope.result["state"], "configured");
        assert_eq!(profile_envelope.result["profile_id"], "nostr");
    }

    #[test]
    fn runtime_service_backs_sync_status() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), Vec::new());
        let service = OperationAdapter::new(RuntimeOperationService::new(&config));

        let sync =
            OperationRequest::new(OperationContext::default(), SyncStatusGetRequest::default())
                .expect("sync status request");
        let envelope = service
            .execute(sync)
            .expect("sync status result")
            .to_envelope(OperationContext::default().envelope_context("req_sync_status"))
            .expect("sync status envelope");

        assert_eq!(envelope.operation_id, "sync.status.get");
        assert_eq!(envelope.result["state"], "ready");
        assert_eq!(
            envelope.result["source"],
            "SDK canonical event store and outbox"
        );
        assert_eq!(
            envelope.result["replica_db"],
            "derived_projection_not_checked"
        );
        assert_eq!(envelope.result["queue"]["pending_count"], 0);
        assert_eq!(envelope.result["queue"]["total_count"], 0);
        assert_eq!(envelope.result["actions"][0], "radroots sync pull");
    }

    fn sample_config(root: &Path, relays: Vec<String>) -> RuntimeConfig {
        let data = root.join("data");
        let cache = root.join("cache");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Terminal,
                verbosity: Verbosity::Normal,
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
                shared_cache_root: cache.clone(),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
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
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
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
            transport: crate::runtime::config::TransportConfig::from_nostr_relay_urls(
                relays.clone(),
            ),
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
            mesh: crate::runtime::config::MeshConfig::disabled(),
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
            },
            rhi: crate::runtime::config::RhiConfig {
                trusted_worker_pubkeys: Vec::new(),
            },
            capability_bindings: Vec::new(),
        }
    }
}
