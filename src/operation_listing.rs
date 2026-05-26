use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use crate::cli::global::{
    ListingAppRecordExportArgs, ListingCreateArgs, ListingFileArgs, ListingMutationArgs,
    ListingRebindArgs, RecordLookupArgs,
};
use crate::ops::{
    ListingAppExportRequest, ListingAppExportResult, ListingAppListRequest, ListingAppListResult,
    ListingArchiveRequest, ListingArchiveResult, ListingCreateRequest, ListingCreateResult,
    ListingGetRequest, ListingGetResult, ListingListRequest, ListingListResult,
    ListingPublishRequest, ListingPublishResult, ListingRebindRequest, ListingRebindResult,
    ListingUpdateRequest, ListingUpdateResult, ListingValidateRequest, ListingValidateResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::view::runtime::{CommandDisposition, ListingAppRecordExportView, ListingMutationView};

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
        let args = ListingCreateArgs {
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
            discount_id: string_input(&request, "discount_id"),
            discount_label: string_input(&request, "discount_label"),
            discount_kind: string_input(&request, "discount_kind"),
            discount_value: string_input(&request, "discount_value"),
            discount_amount: string_input(&request, "discount_amount"),
            discount_currency: string_input(&request, "discount_currency"),
        };
        if request.context.dry_run {
            let view = map_runtime(
                request.operation_id(),
                crate::runtime::listing::scaffold_preflight(self.config, &args),
            )?;
            return serialized_operation_result::<ListingCreateResult, _>(&view);
        }

        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::scaffold(self.config, &args),
        )?;
        serialized_operation_result::<ListingCreateResult, _>(&view)
    }
}

impl OperationService<ListingGetRequest> for ListingOperationService<'_> {
    type Result = ListingGetResult;

    fn execute(
        &self,
        request: OperationRequest<ListingGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = RecordLookupArgs {
            key: required_string(&request, "key")?,
        };
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::get(self.config, &args),
        )?;
        serialized_operation_result::<ListingGetResult, _>(&view)
    }
}

impl OperationService<ListingListRequest> for ListingOperationService<'_> {
    type Result = ListingListResult;

    fn execute(
        &self,
        request: OperationRequest<ListingListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::list(self.config),
        )?;
        serialized_operation_result::<ListingListResult, _>(&view)
    }
}

impl OperationService<ListingAppListRequest> for ListingOperationService<'_> {
    type Result = ListingAppListResult;

    fn execute(
        &self,
        request: OperationRequest<ListingAppListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::app_record_list(self.config),
        )?;
        serialized_operation_result::<ListingAppListResult, _>(&view)
    }
}

impl OperationService<ListingAppExportRequest> for ListingOperationService<'_> {
    type Result = ListingAppExportResult;

    fn execute(
        &self,
        request: OperationRequest<ListingAppExportRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = ListingAppRecordExportArgs {
            record_id: required_string(&request, "record_id")?,
            output: optional_path(&request, "output"),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::app_record_export(&config, &args),
        )?;
        listing_app_record_export_result::<ListingAppExportResult>(request.operation_id(), &view)
    }
}

impl OperationService<ListingUpdateRequest> for ListingOperationService<'_> {
    type Result = ListingUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<ListingUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if !request.context.dry_run {
            require_approval(&request)?;
        }
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::update(&config, &args),
        )?;
        mutation_result::<ListingUpdateResult>(request.operation_id(), &view)
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
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::validate(self.config, &args),
        )?;
        serialized_operation_result::<ListingValidateResult, _>(&view)
    }
}

impl OperationService<ListingRebindRequest> for ListingOperationService<'_> {
    type Result = ListingRebindResult;

    fn execute(
        &self,
        request: OperationRequest<ListingRebindRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = ListingRebindArgs {
            file: required_path(&request, "file")?,
            selector: required_string(&request, "selector")?,
            farm_d_tag: string_input(&request, "farm_d_tag"),
        };
        if request.context.dry_run {
            let view = map_runtime(
                request.operation_id(),
                crate::runtime::listing::rebind_preflight(self.config, &args),
            )?;
            return serialized_operation_result::<ListingRebindResult, _>(&view);
        }
        require_approval(&request)?;
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::rebind(self.config, &args),
        )?;
        serialized_operation_result::<ListingRebindResult, _>(&view)
    }
}

impl OperationService<ListingPublishRequest> for ListingOperationService<'_> {
    type Result = ListingPublishResult;

    fn execute(
        &self,
        request: OperationRequest<ListingPublishRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if !request.context.dry_run {
            require_approval(&request)?;
        }
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = crate::runtime::listing::publish(&config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        mutation_result::<ListingPublishResult>(request.operation_id(), &view)
    }
}

impl OperationService<ListingArchiveRequest> for ListingOperationService<'_> {
    type Result = ListingArchiveResult;

    fn execute(
        &self,
        request: OperationRequest<ListingArchiveRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if !request.context.dry_run {
            require_approval(&request)?;
        }
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = map_runtime(
            request.operation_id(),
            crate::runtime::listing::archive(&config, &args),
        )?;
        mutation_result::<ListingArchiveResult>(request.operation_id(), &view)
    }
}

fn mutation_config<P>(config: &RuntimeConfig, request: &OperationRequest<P>) -> RuntimeConfig
where
    P: OperationRequestPayload,
{
    let mut config = config.clone();
    if request.context.dry_run {
        config.output.dry_run = true;
    }
    config
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
        signer_session_id: string_input(request, "signer_session_id"),
        print_event: bool_input(request, "print_event").unwrap_or(false),
    })
}

fn require_approval<P>(request: &OperationRequest<P>) -> Result<(), OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    if request.context.requires_approval_token() {
        return Err(OperationAdapterError::approval_required(
            request.operation_id(),
        ));
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

fn mutation_result<R>(
    operation_id: &str,
    view: &ListingMutationView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<R, _>(view),
        CommandDisposition::ExternalUnavailable if listing_relay_unavailable(view) => {
            Err(OperationAdapterError::network_unavailable_with_detail(
                operation_id,
                view.reason.clone().unwrap_or_else(|| {
                    format!(
                        "listing {} finished with state `{}`",
                        view.operation, view.state
                    )
                }),
                serde_json::to_value(view).unwrap_or(Value::Null),
            ))
        }
        disposition => Err(OperationAdapterError::from_command_disposition(
            operation_id,
            disposition,
            view.reason.clone().unwrap_or_else(|| {
                format!(
                    "listing {} finished with state `{}`",
                    view.operation, view.state
                )
            }),
        )),
    }
}

fn listing_relay_unavailable(view: &ListingMutationView) -> bool {
    view.source == "direct Nostr relay publish · local key"
        && (view.reason.as_deref().is_some_and(|reason| {
            reason.contains("configured relay") || reason.contains("direct relay connection failed")
        }) || !view.target_relays.is_empty()
            || !view.connected_relays.is_empty()
            || !view.failed_relays.is_empty())
}

fn listing_app_record_export_result<R>(
    operation_id: &str,
    view: &ListingAppRecordExportView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<R, _>(view),
        CommandDisposition::NotFound => Err(OperationAdapterError::not_found_with_detail(
            operation_id,
            view.reason.clone().unwrap_or_else(|| {
                format!(
                    "app-authored local record `{}` was not found",
                    view.record_id
                )
            }),
            serde_json::to_value(view).unwrap_or(Value::Null),
        )),
        CommandDisposition::ValidationFailed => {
            Err(OperationAdapterError::validation_failed_with_detail(
                operation_id,
                view.reason.clone().unwrap_or_else(|| {
                    format!(
                        "app-authored local record `{}` cannot be exported",
                        view.record_id
                    )
                }),
                serde_json::to_value(view).unwrap_or(Value::Null),
            ))
        }
        disposition => Err(OperationAdapterError::from_command_disposition(
            operation_id,
            disposition,
            view.reason.clone().unwrap_or_else(|| {
                format!(
                    "app-authored local record export finished with state `{}`",
                    view.state
                )
            }),
        )),
    }
}

fn map_runtime<T>(
    operation_id: &str,
    result: Result<T, RuntimeError>,
) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::runtime_failure(operation_id, error))
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
    use crate::ops::{
        ListingArchiveRequest, ListingCreateRequest, ListingListRequest, ListingPublishRequest,
        OperationAdapter, OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, PublishConfig, PublishMode, PublishModeSource, RelayConfig, RelayConfigSource,
        RelayPublishPolicy, RpcConfig, RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn listing_service_requires_seller_actor_for_create_dry_run() {
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
        let error = service
            .execute(request)
            .expect_err("listing create seller actor");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "account_unresolved");
        assert!(output_error.detail.expect("detail")["seller_actor_source"] == "resolved_account");
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
        assert_eq!(publish_error.to_output_error().code, "approval_required");
        assert_eq!(publish_error.to_output_error().exit_code, 6);

        let mut context = OperationContext::default();
        context.dry_run = true;
        let archive = OperationRequest::new(
            context.clone(),
            ListingArchiveRequest::from_data(data(&[("file", "listing.toml")])),
        )
        .expect("listing archive request");
        let archive_error = service.execute(archive).expect_err("archive preflight");
        assert!(!format!("{archive_error}").contains("approval_token"));
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
            publish: PublishConfig {
                mode: PublishMode::NostrRelay,
                source: PublishModeSource::Defaults,
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
            rhi: crate::runtime::config::RhiConfig {
                trusted_worker_pubkeys: Vec::new(),
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
