#![allow(dead_code)]

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{ListingFileArgs, ListingMutationArgs, ListingNewArgs, RecordKeyArgs};
use crate::operation_adapter::{
    ListingArchiveRequest, ListingArchiveResult, ListingCreateRequest, ListingCreateResult,
    ListingGetRequest, ListingGetResult, ListingListRequest, ListingListResult,
    ListingPublishRequest, ListingPublishResult, ListingUpdateRequest, ListingUpdateResult,
    ListingValidateRequest, ListingValidateResult, OperationAdapterError, OperationRequest,
    OperationRequestData, OperationRequestPayload, OperationResult, OperationResultData,
    OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub struct ListingOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> ListingOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<ListingCreateRequest> for ListingOperationService<'_> {
    type Result = ListingCreateResult;

    fn execute(
        &self,
        request: OperationRequest<ListingCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = ListingNewArgs {
            output: optional_path(&request, "output"),
            key: string_input(&request, "key"),
            title: string_input(&request, "title"),
            category: string_input(&request, "category"),
            summary: string_input(&request, "summary"),
            bin_id: string_input(&request, "bin_id"),
            quantity_amount: string_input(&request, "quantity_amount"),
            quantity_unit: string_input(&request, "quantity_unit"),
            price_amount: string_input(&request, "price_amount"),
            price_currency: string_input(&request, "price_currency"),
            price_per_amount: string_input(&request, "price_per_amount"),
            price_per_unit: string_input(&request, "price_per_unit"),
            available: string_input(&request, "available"),
            label: string_input(&request, "label"),
        };
        if request.context.dry_run {
            return json_operation_result::<ListingCreateResult>(json!({
                "state": "dry_run",
                "output": args.output.as_ref().map(|path| path.display().to_string()),
                "key": args.key,
                "title": args.title,
            }));
        }

        let view = map_runtime(crate::runtime::listing::scaffold(self.config, &args))?;
        serialized_operation_result::<ListingCreateResult, _>(&view)
    }
}

impl OperationService<ListingGetRequest> for ListingOperationService<'_> {
    type Result = ListingGetResult;

    fn execute(
        &self,
        request: OperationRequest<ListingGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = RecordKeyArgs {
            key: required_string(&request, "key")?,
        };
        let view = map_runtime(crate::runtime::listing::get(self.config, &args))?;
        serialized_operation_result::<ListingGetResult, _>(&view)
    }
}

impl OperationService<ListingListRequest> for ListingOperationService<'_> {
    type Result = ListingListResult;

    fn execute(
        &self,
        _request: OperationRequest<ListingListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        json_operation_result::<ListingListResult>(json!({
            "state": "empty",
            "source": "local draft - local first",
            "count": 0,
            "listings": [],
            "reason": null,
            "actions": ["radroots listing create"],
        }))
    }
}

impl OperationService<ListingUpdateRequest> for ListingOperationService<'_> {
    type Result = ListingUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<ListingUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            return mutation_dry_run::<ListingUpdateResult>(&request, "update");
        }
        let args = mutation_args(&request)?;
        let view = map_runtime(crate::runtime::listing::update(self.config, &args))?;
        serialized_operation_result::<ListingUpdateResult, _>(&view)
    }
}

impl OperationService<ListingValidateRequest> for ListingOperationService<'_> {
    type Result = ListingValidateResult;

    fn execute(
        &self,
        request: OperationRequest<ListingValidateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = ListingFileArgs {
            file: required_path(&request, "file")?,
        };
        let view = map_runtime(crate::runtime::listing::validate(self.config, &args))?;
        serialized_operation_result::<ListingValidateResult, _>(&view)
    }
}

impl OperationService<ListingPublishRequest> for ListingOperationService<'_> {
    type Result = ListingPublishResult;

    fn execute(
        &self,
        request: OperationRequest<ListingPublishRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            return mutation_dry_run::<ListingPublishResult>(&request, "publish");
        }
        require_approval(&request)?;
        let args = mutation_args(&request)?;
        let view = map_runtime(crate::runtime::listing::publish(self.config, &args))?;
        serialized_operation_result::<ListingPublishResult, _>(&view)
    }
}

impl OperationService<ListingArchiveRequest> for ListingOperationService<'_> {
    type Result = ListingArchiveResult;

    fn execute(
        &self,
        request: OperationRequest<ListingArchiveRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            return mutation_dry_run::<ListingArchiveResult>(&request, "archive");
        }
        require_approval(&request)?;
        let args = mutation_args(&request)?;
        let view = map_runtime(crate::runtime::listing::archive(self.config, &args))?;
        serialized_operation_result::<ListingArchiveResult, _>(&view)
    }
}

fn mutation_args<P>(
    request: &OperationRequest<P>,
) -> Result<ListingMutationArgs, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    Ok(ListingMutationArgs {
        file: required_path(request, "file")?,
        idempotency_key: request
            .context
            .idempotency_key
            .clone()
            .or_else(|| string_input(request, "idempotency_key")),
        signer_session_id: request
            .context
            .signer_session_id
            .clone()
            .or_else(|| string_input(request, "signer_session_id")),
        print_job: bool_input(request, "print_job").unwrap_or(false),
        print_event: bool_input(request, "print_event").unwrap_or(false),
    })
}

fn mutation_dry_run<R>(
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
    action: &str,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    json_operation_result::<R>(json!({
        "state": "dry_run",
        "action": action,
        "file": optional_path(request, "file").map(|path| path.display().to_string()),
        "idempotency_key": request.context.idempotency_key,
        "signer_session_id": request.context.signer_session_id,
    }))
}

fn require_approval<P>(request: &OperationRequest<P>) -> Result<(), OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    if request.context.approval_token.is_none() {
        return Err(OperationAdapterError::InvalidInput {
            operation_id: request.operation_id().to_owned(),
            message: "missing required `approval_token` input".to_owned(),
        });
    }
    Ok(())
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

fn required_string<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key).ok_or_else(|| {
        invalid_input(
            request.operation_id(),
            format!("missing required `{key}` input"),
        )
    })
}

fn required_path<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<PathBuf, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    optional_path(request, key).ok_or_else(|| {
        invalid_input(
            request.operation_id(),
            format!("missing required `{key}` input"),
        )
    })
}

fn optional_path<P>(request: &OperationRequest<P>, key: &str) -> Option<PathBuf>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key).map(PathBuf::from)
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

fn bool_input<P>(request: &OperationRequest<P>, key: &str) -> Option<bool>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_bool)
}

fn invalid_input(operation_id: &str, message: String) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value};
    use tempfile::tempdir;

    use super::ListingOperationService;
    use crate::operation_adapter::{
        ListingArchiveRequest, ListingCreateRequest, ListingListRequest, ListingPublishRequest,
        OperationAdapter, OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn listing_service_supports_create_dry_run_without_sell_path() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(ListingOperationService::new(&config));
        let mut context = OperationContext::default();
        context.dry_run = true;
        let request = OperationRequest::new(
            context.clone(),
            ListingCreateRequest::from_data(data(&[("key", "eggs"), ("title", "Eggs")])),
        )
        .expect("listing create request");
        let envelope = service
            .execute(request)
            .expect("listing create result")
            .to_envelope(context.envelope_context("req_listing_create"))
            .expect("listing create envelope");

        assert_eq!(envelope.operation_id, "listing.create");
        assert_eq!(envelope.dry_run, true);
        assert_eq!(envelope.result["state"], "dry_run");
        assert_eq!(envelope.result["key"], "eggs");
    }

    #[test]
    fn listing_service_exposes_listing_list_operation() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(ListingOperationService::new(&config));
        let request =
            OperationRequest::new(OperationContext::default(), ListingListRequest::default())
                .expect("listing list request");
        let envelope = service
            .execute(request)
            .expect("listing list result")
            .to_envelope(OperationContext::default().envelope_context("req_listing_list"))
            .expect("listing list envelope");

        assert_eq!(envelope.operation_id, "listing.list");
        assert_eq!(envelope.result["state"], "empty");
        assert_eq!(envelope.result["count"], 0);
    }

    #[test]
    fn listing_publish_and_archive_require_approval_unless_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(ListingOperationService::new(&config));
        let publish = OperationRequest::new(
            OperationContext::default(),
            ListingPublishRequest::from_data(data(&[("file", "listing.toml")])),
        )
        .expect("listing publish request");
        let publish_error = service.execute(publish).expect_err("approval required");
        assert!(format!("{publish_error}").contains("approval_token"));

        let mut context = OperationContext::default();
        context.dry_run = true;
        let archive = OperationRequest::new(
            context.clone(),
            ListingArchiveRequest::from_data(data(&[("file", "listing.toml")])),
        )
        .expect("listing archive request");
        let archive_envelope = service
            .execute(archive)
            .expect("archive dry run")
            .to_envelope(context.envelope_context("req_listing_archive"))
            .expect("archive envelope");
        assert_eq!(archive_envelope.operation_id, "listing.archive");
        assert_eq!(archive_envelope.result["state"], "dry_run");
    }

    fn sample_config(root: &Path) -> RuntimeConfig {
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
                urls: Vec::new(),
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

    fn data(entries: &[(&str, &str)]) -> OperationData {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
            .collect::<Map<String, Value>>()
    }
}
