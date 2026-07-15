use serde::Serialize;
use serde_json::{Value, json};

use crate::ops::{
    DiagnosticsInspectRequest, DiagnosticsInspectResult, OperationAdapterError, OperationRequest,
    OperationRequestData, OperationResult, OperationResultData, OperationService,
    SignerStatusRequest, SignerStatusResult, SyncPullRequest, SyncPullResult, SyncPushRequest,
    SyncPushResult, SyncStatusRequest, SyncStatusResult, TransportCapabilityListRequest,
    TransportCapabilityListResult, TransportConfigInspectRequest, TransportConfigInspectResult,
    TransportConfigUpdateRequest, TransportConfigUpdateResult, TransportDeliveryInspectRequest,
    TransportDeliveryInspectResult, TransportDeliveryRetryRequest, TransportDeliveryRetryResult,
    TransportStatusInspectRequest, TransportStatusInspectResult,
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

impl OperationService<SignerStatusRequest> for RuntimeOperationService<'_> {
    type Result = SignerStatusResult;

    fn execute(
        &self,
        _request: OperationRequest<SignerStatusRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::signer::resolve_signer_status(self.config);
        serialized_operation_result::<SignerStatusResult, _>(&view)
    }
}

impl OperationService<TransportConfigInspectRequest> for RuntimeOperationService<'_> {
    type Result = TransportConfigInspectResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportConfigInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::profile(self.config);
        serialized_operation_result::<TransportConfigInspectResult, _>(&view)
    }
}

impl OperationService<TransportConfigUpdateRequest> for RuntimeOperationService<'_> {
    type Result = TransportConfigUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<TransportConfigUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::transport::set_profile(self.config, request.payload.input()),
        )?;
        serialized_operation_result::<TransportConfigUpdateResult, _>(&view)
    }
}

impl OperationService<TransportCapabilityListRequest> for RuntimeOperationService<'_> {
    type Result = TransportCapabilityListResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportCapabilityListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let status = crate::runtime::transport::status(self.config);
        let view = json!({
            "state": "ready",
            "source": "Runtime Contract V1 transport capability registry",
            "active_profile": status.active_profile,
            "transports": status.transports,
        });
        serialized_operation_result::<TransportCapabilityListResult, _>(&view)
    }
}

impl OperationService<TransportStatusInspectRequest> for RuntimeOperationService<'_> {
    type Result = TransportStatusInspectResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportStatusInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::status(self.config);
        serialized_operation_result::<TransportStatusInspectResult, _>(&view)
    }
}

impl OperationService<TransportDeliveryInspectRequest> for RuntimeOperationService<'_> {
    type Result = TransportDeliveryInspectResult;

    fn execute(
        &self,
        _request: OperationRequest<TransportDeliveryInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::outbox_status(self.config).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure("transport.delivery.inspect", error)
        })?;
        serialized_operation_result::<TransportDeliveryInspectResult, _>(&view)
    }
}

impl OperationService<TransportDeliveryRetryRequest> for RuntimeOperationService<'_> {
    type Result = TransportDeliveryRetryResult;

    fn execute(
        &self,
        request: OperationRequest<TransportDeliveryRetryRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::transport::outbox_push(self.config).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        serialized_operation_result::<TransportDeliveryRetryResult, _>(&view)
    }
}

impl OperationService<SyncStatusRequest> for RuntimeOperationService<'_> {
    type Result = SyncStatusResult;

    fn execute(
        &self,
        _request: OperationRequest<SyncStatusRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::sync::status(self.config)
            .map_err(|error| OperationAdapterError::sdk_adapter_failure("sync.status", error))?;
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
        _request: OperationRequest<SyncPushRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = crate::runtime::sync::push(self.config)
            .map_err(|error| OperationAdapterError::sdk_adapter_failure("sync.push", error))?;
        sync_action_result::<SyncPushResult>("sync.push", &view)
    }
}

impl OperationService<DiagnosticsInspectRequest> for RuntimeOperationService<'_> {
    type Result = DiagnosticsInspectResult;

    fn execute(
        &self,
        _request: OperationRequest<DiagnosticsInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let signer = crate::runtime::signer::resolve_signer_status(self.config);
        let transport = crate::runtime::transport::status(self.config);
        let sync = match crate::runtime::sync::status(self.config) {
            Ok(view) => serde_json::to_value(view).unwrap_or_else(|_| json!({ "state": "error" })),
            Err(error) => json!({
                "state": "unavailable",
                "reason": error.to_string(),
            }),
        };
        let state = if sync.get("state").and_then(Value::as_str) == Some("unavailable") {
            "degraded"
        } else {
            "ready"
        };
        let view = json!({
            "state": state,
            "source": "Runtime Contract V1 diagnostics",
            "signer": signer,
            "transport": transport,
            "sync": sync,
        });
        serialized_operation_result::<DiagnosticsInspectResult, _>(&view)
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
) -> Result<OperationResult<SyncStatusResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<SyncStatusResult, _>(view),
        disposition => Err(sync_view_error(
            "sync.status",
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

#[cfg(test)]
mod tests {
    use radroots_secret_vault::RadrootsSecretBackend;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    use super::RuntimeOperationService;
    use crate::ops::{
        OperationAdapter, OperationContext, OperationRequest, SignerStatusRequest,
        SyncStatusRequest, TransportConfigInspectRequest,
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

        let signer =
            OperationRequest::new(OperationContext::default(), SignerStatusRequest::default())
                .expect("signer status request");
        let signer_envelope = service
            .execute(signer)
            .expect("signer status result")
            .to_envelope(OperationContext::default().envelope_context("req_signer"))
            .expect("signer envelope");
        assert_eq!(signer_envelope.operation_id, "signer.status");
        assert_eq!(signer_envelope.result["state"], "unconfigured");

        let profile = OperationRequest::new(
            OperationContext::default(),
            TransportConfigInspectRequest::default(),
        )
        .expect("transport profile request");
        let profile_envelope = service
            .execute(profile)
            .expect("transport profile result")
            .to_envelope(OperationContext::default().envelope_context("req_transport"))
            .expect("transport profile envelope");
        assert_eq!(profile_envelope.operation_id, "transport.config.inspect");
        assert_eq!(profile_envelope.result["state"], "configured");
        assert_eq!(profile_envelope.result["profile_id"], "nostr");
    }

    #[test]
    fn runtime_service_backs_sync_status() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), Vec::new());
        let service = OperationAdapter::new(RuntimeOperationService::new(&config));

        let sync = OperationRequest::new(OperationContext::default(), SyncStatusRequest::default())
            .expect("sync status request");
        let envelope = service
            .execute(sync)
            .expect("sync status result")
            .to_envelope(OperationContext::default().envelope_context("req_sync_status"))
            .expect("sync status envelope");

        assert_eq!(envelope.operation_id, "sync.status");
        assert_eq!(envelope.result["state"], "ready");
        assert_eq!(
            envelope.result["source"],
            "SDK canonical event store and outbox"
        );
        assert_eq!(
            envelope.result["replica_store"],
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
                replica_store_path: data.join("apps/cli/replica/replica.sqlite"),
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
                validator_set: None,
                require_cryptographic_proof: false,
            },
            capability_bindings: Vec::new(),
        }
    }
}
