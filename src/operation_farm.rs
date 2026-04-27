use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::runtime::{CommandDisposition, FarmPublishView};
use crate::operation_adapter::{
    FarmCreateRequest, FarmCreateResult, FarmFulfillmentUpdateRequest, FarmFulfillmentUpdateResult,
    FarmGetRequest, FarmGetResult, FarmLocationUpdateRequest, FarmLocationUpdateResult,
    FarmProfileUpdateRequest, FarmProfileUpdateResult, FarmPublishRequest, FarmPublishResult,
    FarmReadinessCheckRequest, FarmReadinessCheckResult, OperationAdapterError, OperationRequest,
    OperationRequestData, OperationRequestPayload, OperationResult, OperationResultData,
    OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime_args::{
    FarmCreateArgs, FarmFieldArg, FarmPublishArgs, FarmScopeArg, FarmScopedArgs, FarmUpdateArgs,
};

pub struct FarmOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> FarmOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<FarmCreateRequest> for FarmOperationService<'_> {
    type Result = FarmCreateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmCreateArgs {
            scope: scope_input(&request)?,
            farm_d_tag: string_input(&request, "farm_d_tag"),
            name: string_input(&request, "name"),
            display_name: string_input(&request, "display_name"),
            about: string_input(&request, "about"),
            website: string_input(&request, "website"),
            picture: string_input(&request, "picture"),
            banner: string_input(&request, "banner"),
            location: string_input(&request, "location"),
            city: string_input(&request, "city"),
            region: string_input(&request, "region"),
            country: string_input(&request, "country"),
            delivery_method: string_input(&request, "delivery_method"),
        };
        if request.context.dry_run {
            return json_operation_result::<FarmCreateResult>(json!({
                "state": "dry_run",
                "scope": args.scope.map(scope_name),
                "name": args.name,
                "location": args.location,
            }));
        }

        let view = map_runtime(crate::runtime::farm::init(self.config, &args))?;
        serialized_operation_result::<FarmCreateResult, _>(&view)
    }
}

impl OperationService<FarmGetRequest> for FarmOperationService<'_> {
    type Result = FarmGetResult;

    fn execute(
        &self,
        request: OperationRequest<FarmGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmScopedArgs {
            scope: scope_input(&request)?,
        };
        let view = map_runtime(crate::runtime::farm::get(self.config, &args))?;
        serialized_operation_result::<FarmGetResult, _>(&view)
    }
}

impl OperationService<FarmProfileUpdateRequest> for FarmOperationService<'_> {
    type Result = FarmProfileUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmProfileUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        farm_set::<FarmProfileUpdateResult>(&request, self.config, profile_field(&request)?)
    }
}

impl OperationService<FarmLocationUpdateRequest> for FarmOperationService<'_> {
    type Result = FarmLocationUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmLocationUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        farm_set::<FarmLocationUpdateResult>(&request, self.config, location_field(&request)?)
    }
}

impl OperationService<FarmFulfillmentUpdateRequest> for FarmOperationService<'_> {
    type Result = FarmFulfillmentUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmFulfillmentUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        farm_set::<FarmFulfillmentUpdateResult>(&request, self.config, FarmFieldArg::Delivery)
    }
}

impl OperationService<FarmReadinessCheckRequest> for FarmOperationService<'_> {
    type Result = FarmReadinessCheckResult;

    fn execute(
        &self,
        request: OperationRequest<FarmReadinessCheckRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmScopedArgs {
            scope: scope_input(&request)?,
        };
        let view = map_runtime(crate::runtime::farm::status(self.config, &args))?;
        serialized_operation_result::<FarmReadinessCheckResult, _>(&view)
    }
}

impl OperationService<FarmPublishRequest> for FarmOperationService<'_> {
    type Result = FarmPublishResult;

    fn execute(
        &self,
        request: OperationRequest<FarmPublishRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmPublishArgs {
            scope: scope_input(&request)?,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            signer_session_id: string_input(&request, "signer_session_id"),
            print_job: bool_input(&request, "print_job").unwrap_or(false),
            print_event: bool_input(&request, "print_event").unwrap_or(false),
        };
        if !request.context.dry_run && request.context.approval_token.is_none() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let view = crate::runtime::farm::publish(self.config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        farm_publish_result(request.operation_id(), &view)
    }
}

fn farm_set<R>(
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
    config: &RuntimeConfig,
    field: FarmFieldArg,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    let value = required_string(request, "value")?;
    let args = FarmUpdateArgs {
        scope: scope_input(request)?,
        field,
        value: vec![value.clone()],
    };
    if request.context.dry_run {
        return json_operation_result::<R>(json!({
            "state": "dry_run",
            "field": field_name(field),
            "value": value,
        }));
    }

    let view = map_runtime(crate::runtime::farm::set(config, &args))?;
    serialized_operation_result::<R, _>(&view)
}

fn profile_field(
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
) -> Result<FarmFieldArg, OperationAdapterError> {
    match string_input(request, "field").as_deref() {
        Some("name") | None => Ok(FarmFieldArg::Name),
        Some("display_name") | Some("display-name") => Ok(FarmFieldArg::DisplayName),
        Some("about") => Ok(FarmFieldArg::About),
        Some("website") => Ok(FarmFieldArg::Website),
        Some("picture") => Ok(FarmFieldArg::Picture),
        Some("banner") => Ok(FarmFieldArg::Banner),
        Some(other) => Err(invalid_input(
            request.operation_id(),
            format!("profile field `{other}` is not supported"),
        )),
    }
}

fn location_field(
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
) -> Result<FarmFieldArg, OperationAdapterError> {
    match string_input(request, "field").as_deref() {
        Some("location") | None => Ok(FarmFieldArg::Location),
        Some("city") => Ok(FarmFieldArg::City),
        Some("region") => Ok(FarmFieldArg::Region),
        Some("country") => Ok(FarmFieldArg::Country),
        Some(other) => Err(invalid_input(
            request.operation_id(),
            format!("location field `{other}` is not supported"),
        )),
    }
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn farm_publish_result(
    operation_id: &str,
    view: &FarmPublishView,
) -> Result<OperationResult<FarmPublishResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<FarmPublishResult, _>(view),
        CommandDisposition::Unconfigured => Err(OperationAdapterError::unconfigured(
            operation_id,
            view.reason
                .clone()
                .unwrap_or_else(|| "farm publish is unconfigured".to_owned()),
        )),
        CommandDisposition::ExternalUnavailable => Err(OperationAdapterError::unavailable(
            operation_id,
            view.reason
                .clone()
                .unwrap_or_else(|| "farm publish is unavailable".to_owned()),
        )),
        CommandDisposition::Unsupported => Err(OperationAdapterError::InvalidInput {
            operation_id: operation_id.to_owned(),
            message: view
                .reason
                .clone()
                .unwrap_or_else(|| "farm publish is unsupported".to_owned()),
        }),
        CommandDisposition::InternalError => Err(OperationAdapterError::Runtime(
            view.reason
                .clone()
                .unwrap_or_else(|| "farm publish failed".to_owned()),
        )),
    }
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

fn scope_input<P>(
    request: &OperationRequest<P>,
) -> Result<Option<FarmScopeArg>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    match string_input(request, "scope").as_deref() {
        Some("user") => Ok(Some(FarmScopeArg::User)),
        Some("workspace") => Ok(Some(FarmScopeArg::Workspace)),
        Some(other) => Err(invalid_input(
            request.operation_id(),
            format!("scope must be `user` or `workspace`, got `{other}`"),
        )),
        None => Ok(None),
    }
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

fn scope_name(scope: FarmScopeArg) -> &'static str {
    match scope {
        FarmScopeArg::User => "user",
        FarmScopeArg::Workspace => "workspace",
    }
}

fn field_name(field: FarmFieldArg) -> &'static str {
    match field {
        FarmFieldArg::Name => "name",
        FarmFieldArg::DisplayName => "display_name",
        FarmFieldArg::About => "about",
        FarmFieldArg::Website => "website",
        FarmFieldArg::Picture => "picture",
        FarmFieldArg::Banner => "banner",
        FarmFieldArg::Location => "location",
        FarmFieldArg::City => "city",
        FarmFieldArg::Region => "region",
        FarmFieldArg::Country => "country",
        FarmFieldArg::Delivery => "delivery",
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value};
    use tempfile::tempdir;

    use super::FarmOperationService;
    use crate::operation_adapter::{
        FarmCreateRequest, FarmGetRequest, FarmPublishRequest, FarmReadinessCheckRequest,
        OperationAdapter, OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn farm_service_reports_missing_farm_config() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let request = OperationRequest::new(OperationContext::default(), FarmGetRequest::default())
            .expect("farm get request");
        let envelope = service
            .execute(request)
            .expect("farm get result")
            .to_envelope(OperationContext::default().envelope_context("req_farm_get"))
            .expect("farm get envelope");

        assert_eq!(envelope.operation_id, "farm.get");
        assert_eq!(envelope.result["state"], "unconfigured");
        assert_eq!(envelope.result["config_present"], false);
    }

    #[test]
    fn farm_service_supports_create_and_readiness_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let mut context = OperationContext::default();
        context.dry_run = true;
        let request = OperationRequest::new(
            context.clone(),
            FarmCreateRequest::from_data(data(&[("name", "dry farm"), ("location", "earth")])),
        )
        .expect("farm create request");
        let envelope = service
            .execute(request)
            .expect("farm create result")
            .to_envelope(context.envelope_context("req_farm_create"))
            .expect("farm create envelope");

        assert_eq!(envelope.operation_id, "farm.create");
        assert_eq!(envelope.dry_run, true);
        assert_eq!(envelope.result["state"], "dry_run");

        let readiness = OperationRequest::new(
            OperationContext::default(),
            FarmReadinessCheckRequest::default(),
        )
        .expect("farm readiness request");
        let readiness_envelope = service
            .execute(readiness)
            .expect("farm readiness result")
            .to_envelope(OperationContext::default().envelope_context("req_farm_ready"))
            .expect("farm readiness envelope");
        assert_eq!(readiness_envelope.operation_id, "farm.readiness.check");
        assert_eq!(readiness_envelope.result["state"], "unconfigured");
    }

    #[test]
    fn farm_publish_requires_approval_token_unless_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let request =
            OperationRequest::new(OperationContext::default(), FarmPublishRequest::default())
                .expect("farm publish request");
        let error = service.execute(request).expect_err("approval required");
        assert!(format!("{error}").contains("approval_token"));
        assert_eq!(error.to_output_error().code, "approval_required");
        assert_eq!(error.to_output_error().exit_code, 6);
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
