use serde::Serialize;
use serde_json::{Value, json};

use crate::operation_adapter::{
    JobGetRequest, JobGetResult, JobListRequest, JobListResult, JobWatchRequest, JobWatchResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, RelayListRequest, RelayListResult,
    RuntimeConfigGetRequest, RuntimeConfigGetResult, RuntimeLogWatchRequest, RuntimeLogWatchResult,
    RuntimeRestartRequest, RuntimeRestartResult, RuntimeStartRequest, RuntimeStartResult,
    RuntimeStatusGetRequest, RuntimeStatusGetResult, RuntimeStopRequest, RuntimeStopResult,
    SignerStatusGetRequest, SignerStatusGetResult, SyncPullRequest, SyncPullResult,
    SyncPushRequest, SyncPushResult, SyncStatusGetRequest, SyncStatusGetResult, SyncWatchRequest,
    SyncWatchResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::daemon::{self, DaemonRpcError};
use crate::runtime::management::{
    RuntimeLifecycleAction, inspect_action, inspect_config_show, inspect_logs, inspect_status,
};
use crate::runtime_args::SyncWatchArgs;

const DEFAULT_RUNTIME_ID: &str = "radrootsd";

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

impl OperationService<RuntimeStatusGetRequest> for RuntimeOperationService<'_> {
    type Result = RuntimeStatusGetResult;

    fn execute(
        &self,
        request: OperationRequest<RuntimeStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let target = runtime_target(&request);
        let inspection = map_runtime(inspect_status(
            self.config,
            target.runtime_id.as_str(),
            target.instance_id.as_deref(),
        ))?;
        serialized_operation_result::<RuntimeStatusGetResult, _>(&inspection.view)
    }
}

impl OperationService<RuntimeStartRequest> for RuntimeOperationService<'_> {
    type Result = RuntimeStartResult;

    fn execute(
        &self,
        request: OperationRequest<RuntimeStartRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        runtime_action::<RuntimeStartResult>(self.config, &request, RuntimeLifecycleAction::Start)
    }
}

impl OperationService<RuntimeStopRequest> for RuntimeOperationService<'_> {
    type Result = RuntimeStopResult;

    fn execute(
        &self,
        request: OperationRequest<RuntimeStopRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        runtime_action::<RuntimeStopResult>(self.config, &request, RuntimeLifecycleAction::Stop)
    }
}

impl OperationService<RuntimeRestartRequest> for RuntimeOperationService<'_> {
    type Result = RuntimeRestartResult;

    fn execute(
        &self,
        request: OperationRequest<RuntimeRestartRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        runtime_action::<RuntimeRestartResult>(
            self.config,
            &request,
            RuntimeLifecycleAction::Restart,
        )
    }
}

impl OperationService<RuntimeLogWatchRequest> for RuntimeOperationService<'_> {
    type Result = RuntimeLogWatchResult;

    fn execute(
        &self,
        request: OperationRequest<RuntimeLogWatchRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let target = runtime_target(&request);
        let inspection = map_runtime(inspect_logs(
            self.config,
            target.runtime_id.as_str(),
            target.instance_id.as_deref(),
        ))?;
        serialized_operation_result::<RuntimeLogWatchResult, _>(&inspection.view)
    }
}

impl OperationService<RuntimeConfigGetRequest> for RuntimeOperationService<'_> {
    type Result = RuntimeConfigGetResult;

    fn execute(
        &self,
        request: OperationRequest<RuntimeConfigGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let target = runtime_target(&request);
        let inspection = map_runtime(inspect_config_show(
            self.config,
            target.runtime_id.as_str(),
            target.instance_id.as_deref(),
        ))?;
        serialized_operation_result::<RuntimeConfigGetResult, _>(&inspection.view)
    }
}

impl OperationService<JobListRequest> for RuntimeOperationService<'_> {
    type Result = JobListResult;

    fn execute(
        &self,
        _request: OperationRequest<JobListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        match daemon::bridge_job_list(self.config) {
            Ok(jobs) => json_operation_result::<JobListResult>(json!({
                "state": if jobs.is_empty() { "empty" } else { "ready" },
                "source": daemon::bridge_source(),
                "rpc_url": self.config.rpc.url,
                "count": jobs.len(),
                "reason": null,
                "jobs": jobs,
                "actions": [],
            })),
            Err(error) => job_error_result::<JobListResult>(self.config, error, None),
        }
    }
}

impl OperationService<JobGetRequest> for RuntimeOperationService<'_> {
    type Result = JobGetResult;

    fn execute(
        &self,
        request: OperationRequest<JobGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let job_id = required_string(&request, "job_id")?;
        match daemon::bridge_job(self.config, job_id.as_str()) {
            Ok(Some(job)) => json_operation_result::<JobGetResult>(json!({
                "state": "ready",
                "source": daemon::bridge_source(),
                "rpc_url": self.config.rpc.url,
                "lookup": job_id,
                "reason": null,
                "job": job,
                "actions": [],
            })),
            Ok(None) => json_operation_result::<JobGetResult>(json!({
                "state": "missing",
                "source": daemon::bridge_source(),
                "rpc_url": self.config.rpc.url,
                "lookup": job_id,
                "reason": format!("job `{job_id}` was not found in radrootsd"),
                "job": null,
                "actions": ["radroots job list"],
            })),
            Err(error) => job_error_result::<JobGetResult>(self.config, error, Some(job_id)),
        }
    }
}

impl OperationService<JobWatchRequest> for RuntimeOperationService<'_> {
    type Result = JobWatchResult;

    fn execute(
        &self,
        request: OperationRequest<JobWatchRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let job_id = required_string(&request, "job_id")?;
        let interval_ms = u64_input(&request, "interval_ms").unwrap_or(1_000);
        match daemon::bridge_job(self.config, job_id.as_str()) {
            Ok(Some(job)) => json_operation_result::<JobWatchResult>(json!({
                "state": if job.terminal { job.state.as_str() } else { "watching" },
                "source": daemon::bridge_source(),
                "rpc_url": self.config.rpc.url,
                "job_id": job_id,
                "interval_ms": interval_ms,
                "reason": null,
                "frames": [{
                    "sequence": 1,
                    "observed_at_unix": job.completed_at_unix.unwrap_or(job.requested_at_unix),
                    "state": job.state,
                    "terminal": job.terminal,
                    "signer": job.signer,
                    "signer_session_id": job.signer_session_id,
                    "summary": job.relay_outcome_summary,
                }],
                "actions": [],
            })),
            Ok(None) => json_operation_result::<JobWatchResult>(json!({
                "state": "missing",
                "source": daemon::bridge_source(),
                "rpc_url": self.config.rpc.url,
                "job_id": job_id,
                "interval_ms": interval_ms,
                "reason": format!("job `{job_id}` was not found in radrootsd"),
                "frames": [],
                "actions": ["radroots job list"],
            })),
            Err(error) => job_error_result::<JobWatchResult>(self.config, error, Some(job_id)),
        }
    }
}

fn runtime_action<R>(
    config: &RuntimeConfig,
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
    action: RuntimeLifecycleAction,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    let target = runtime_target(request);
    let inspection = map_runtime(inspect_action(
        config,
        target.runtime_id.as_str(),
        target.instance_id.as_deref(),
        action,
    ))?;
    serialized_operation_result::<R, _>(&inspection.view)
}

fn job_error_result<R>(
    config: &RuntimeConfig,
    error: DaemonRpcError,
    lookup: Option<String>,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    let (state, reason, actions) = match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => (
            "unconfigured",
            reason,
            vec![
                "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell",
                "start radrootsd with bridge ingress enabled",
            ],
        ),
        DaemonRpcError::External(reason) => (
            "unavailable",
            reason,
            vec!["start radrootsd and verify the rpc url"],
        ),
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => ("error", reason, Vec::new()),
    };
    json_operation_result::<R>(json!({
        "state": state,
        "source": daemon::bridge_source(),
        "rpc_url": config.rpc.url,
        "lookup": lookup,
        "reason": reason,
        "count": 0,
        "jobs": [],
        "job": null,
        "actions": actions,
    }))
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn json_operation_result<R>(value: Value) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    OperationResult::new(R::from_value(value))
}

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeTargetInput {
    runtime_id: String,
    instance_id: Option<String>,
}

fn runtime_target<P>(request: &OperationRequest<P>) -> RuntimeTargetInput
where
    P: OperationRequestPayload + OperationRequestData,
{
    RuntimeTargetInput {
        runtime_id: string_input(request, "runtime_id")
            .unwrap_or_else(|| DEFAULT_RUNTIME_ID.into()),
        instance_id: string_input(request, "instance_id"),
    }
}

fn required_string<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key).ok_or_else(|| OperationAdapterError::InvalidInput {
        operation_id: request.operation_id().to_owned(),
        message: format!("missing required `{key}` input"),
    })
}

fn string_input<P>(request: &OperationRequest<P>, key: &str) -> Option<String>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
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
        JobListRequest, OperationAdapter, OperationContext, OperationRequest, RelayListRequest,
        RuntimeStatusGetRequest, SignerStatusGetRequest, SyncStatusGetRequest,
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
    fn runtime_service_backs_sync_and_job_unavailable_states() {
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

        let job = OperationRequest::new(OperationContext::default(), JobListRequest::default())
            .expect("job list request");
        let job_envelope = service
            .execute(job)
            .expect("job list result")
            .to_envelope(OperationContext::default().envelope_context("req_job"))
            .expect("job envelope");
        assert_eq!(job_envelope.operation_id, "job.list");
        assert_eq!(job_envelope.result["state"], "unconfigured");
        assert!(job_envelope.result["reason"].is_string());
    }

    #[test]
    fn runtime_service_backs_runtime_status_default_target() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), Vec::new());
        let service = OperationAdapter::new(RuntimeOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            RuntimeStatusGetRequest::default(),
        )
        .expect("runtime status request");
        let envelope = service
            .execute(request)
            .expect("runtime status result")
            .to_envelope(OperationContext::default().envelope_context("req_runtime"))
            .expect("runtime envelope");

        assert_eq!(envelope.operation_id, "runtime.status.get");
        assert_eq!(envelope.result["runtime_id"], "radrootsd");
        assert!(envelope.result["state"].is_string());
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
