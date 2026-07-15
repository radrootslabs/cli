use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use crate::cli::global::{ListingCreateArgs, ListingMutationArgs, RecordLookupArgs};
use crate::ops::{
    ListingCreateRequest, ListingCreateResult, ListingGetRequest, ListingGetResult,
    ListingListRequest, ListingListResult, ListingPauseRequest, ListingPauseResult,
    ListingPublishRequest, ListingPublishResult, ListingUpdateRequest, ListingUpdateResult,
    ListingWithdrawRequest, ListingWithdrawResult, OperationAdapterError, OperationNetworkMode,
    OperationRequest, OperationRequestData, OperationRequestPayload, OperationResult,
    OperationResultData, OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::view::runtime::{CommandDisposition, ListingMutationView};

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

impl OperationService<ListingUpdateRequest> for ListingOperationService<'_> {
    type Result = ListingUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<ListingUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = crate::runtime::listing::update(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        mutation_result::<ListingUpdateResult>(request.operation_id(), &view)
    }
}

impl OperationService<ListingPublishRequest> for ListingOperationService<'_> {
    type Result = ListingPublishResult;

    fn execute(
        &self,
        request: OperationRequest<ListingPublishRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = crate::runtime::listing::publish_via_sdk(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        mutation_result::<ListingPublishResult>(request.operation_id(), &view)
    }
}

impl OperationService<ListingPauseRequest> for ListingOperationService<'_> {
    type Result = ListingPauseResult;

    fn execute(
        &self,
        request: OperationRequest<ListingPauseRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = crate::runtime::listing::pause(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        mutation_result::<ListingPauseResult>(request.operation_id(), &view)
    }
}

impl OperationService<ListingWithdrawRequest> for ListingOperationService<'_> {
    type Result = ListingWithdrawResult;

    fn execute(
        &self,
        request: OperationRequest<ListingWithdrawRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = mutation_args(&request)?;
        let config = mutation_config(self.config, &request);
        let view = crate::runtime::listing::withdraw(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        mutation_result::<ListingWithdrawResult>(request.operation_id(), &view)
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
        print_event: bool_input(request, "print_event").unwrap_or(false),
        offline: matches!(request.context.network_mode, OperationNetworkMode::Offline),
    })
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
        CommandDisposition::ExternalUnavailable if listing_transport_delivery_unavailable(view) => {
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

fn listing_transport_delivery_unavailable(view: &ListingMutationView) -> bool {
    matches!(
        view.source.as_str(),
        "Nostr transport publish · local key" | "SDK listing publish · configured signer"
    ) && (view.reason.as_deref().is_some_and(|reason| {
        reason.contains("configured Nostr relay")
            || reason.contains("Nostr transport connection failed")
            || reason.contains("SDK transport publish")
    }) || !view.target_transport_endpoints.is_empty()
        || !view.attempted_transport_endpoints.is_empty()
        || !view.failed_transport_targets.is_empty())
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
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value};
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    use super::ListingOperationService;
    use crate::ops::{
        ListingCreateRequest, ListingListRequest, ListingPublishRequest, ListingWithdrawRequest,
        OperationAdapter, OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MycConfig, OutputConfig, OutputFormat, PathsConfig, RpcConfig,
        RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn listing_service_requires_seller_actor_for_create_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(ListingOperationService::new(&config));
        let context = OperationContext {
            dry_run: true,
            ..Default::default()
        };
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
    fn listing_publish_and_withdraw_errors_do_not_use_retired_approval_language() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(ListingOperationService::new(&config));
        let retired_approval_language = ["approval", "token"].join("_");
        let publish = OperationRequest::new(
            OperationContext::default(),
            ListingPublishRequest::from_data(data(&[("file", "listing.toml")])),
        )
        .expect("listing publish request");
        let publish_error = service.execute(publish).expect_err("publish preflight");
        assert!(!format!("{publish_error}").contains(retired_approval_language.as_str()));

        let context = OperationContext {
            dry_run: true,
            ..Default::default()
        };
        let withdraw = OperationRequest::new(
            context.clone(),
            ListingWithdrawRequest::from_data(data(&[("file", "listing.toml")])),
        )
        .expect("listing withdraw request");
        let withdraw_error = service.execute(withdraw).expect_err("withdraw preflight");
        assert!(!format!("{withdraw_error}").contains(retired_approval_language.as_str()));
    }

    fn sample_config(root: &Path) -> RuntimeConfig {
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
            transport: crate::runtime::config::TransportConfig::local_only(),
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

    fn data(entries: &[(&str, &str)]) -> OperationData {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
            .collect::<Map<String, Value>>()
    }
}
