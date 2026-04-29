use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::runtime::{CommandDisposition, LocalBackupView};
use crate::operation_adapter::{
    AccountCreateRequest, AccountCreateResult, AccountGetRequest, AccountGetResult,
    AccountImportRequest, AccountImportResult, AccountListRequest, AccountListResult,
    AccountRemoveRequest, AccountRemoveResult, AccountSelectionClearRequest,
    AccountSelectionClearResult, AccountSelectionGetRequest, AccountSelectionGetResult,
    AccountSelectionUpdateRequest, AccountSelectionUpdateResult, ConfigGetRequest, ConfigGetResult,
    HealthCheckRunRequest, HealthCheckRunResult, HealthStatusGetRequest, HealthStatusGetResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, StoreBackupCreateRequest,
    StoreBackupCreateResult, StoreExportRequest, StoreExportResult, StoreInitRequest,
    StoreInitResult, StoreStatusGetRequest, StoreStatusGetResult, WorkspaceGetRequest,
    WorkspaceGetResult, WorkspaceInitRequest, WorkspaceInitResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts::{
    account_resolution_view, account_summary_view, clear_default_account,
    create_or_migrate_default_account, import_public_identity, preview_account_removal,
    preview_public_identity_import, remove_account, resolve_account_resolution,
    resolve_account_selector, secret_backend_status, select_account, snapshot,
    unresolved_account_reason,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;
use crate::runtime_args::LocalExportFormatArg;

pub struct CoreOperationService<'a> {
    config: &'a RuntimeConfig,
    logging: &'a LoggingState,
}

impl<'a> CoreOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig, logging: &'a LoggingState) -> Self {
        Self { config, logging }
    }
}

impl OperationService<WorkspaceInitRequest> for CoreOperationService<'_> {
    type Result = WorkspaceInitResult;

    fn execute(
        &self,
        request: OperationRequest<WorkspaceInitRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            let local = map_runtime(crate::runtime::local::init_preflight(self.config))?;
            return json_operation_result::<WorkspaceInitResult>(json!({
                "state": local.state,
                "profile": self.config.paths.profile,
                "local": local,
            }));
        }

        let local = map_runtime(crate::runtime::local::init(self.config))?;
        json_operation_result::<WorkspaceInitResult>(json!({
            "state": local.state,
            "profile": self.config.paths.profile,
            "local": local,
        }))
    }
}

impl OperationService<WorkspaceGetRequest> for CoreOperationService<'_> {
    type Result = WorkspaceGetResult;

    fn execute(
        &self,
        _request: OperationRequest<WorkspaceGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        json_operation_result::<WorkspaceGetResult>(json!({
            "profile": self.config.paths.profile,
            "profile_source": self.config.paths.profile_source,
            "root_source": self.config.paths.root_source,
            "app_namespace": self.config.paths.app_namespace,
            "workspace_config_path": self.config.paths.workspace_config_path.as_ref().map(|path| path.display().to_string()),
            "app_config_path": self.config.paths.app_config_path.display().to_string(),
            "app_data_root": self.config.paths.app_data_root.display().to_string(),
            "app_logs_root": self.config.paths.app_logs_root.display().to_string(),
            "local_root": self.config.local.root.display().to_string(),
            "replica_db_path": self.config.local.replica_db_path.display().to_string(),
        }))
    }
}

impl OperationService<HealthStatusGetRequest> for CoreOperationService<'_> {
    type Result = HealthStatusGetResult;

    fn execute(
        &self,
        _request: OperationRequest<HealthStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let store = map_runtime(crate::runtime::local::status(self.config))?;
        let account = map_runtime(resolve_account_resolution(self.config))?;
        json_operation_result::<HealthStatusGetResult>(json!({
            "state": if store.state == "ready" { "ready" } else { "needs_attention" },
            "store": store,
            "account_resolution": account_resolution_view(&account),
            "logging": {
                "initialized": self.logging.initialized,
                "current_file": self.logging.current_file.as_ref().map(|path| path.display().to_string()),
            },
        }))
    }
}

impl OperationService<HealthCheckRunRequest> for CoreOperationService<'_> {
    type Result = HealthCheckRunResult;

    fn execute(
        &self,
        _request: OperationRequest<HealthCheckRunRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let store = map_runtime(crate::runtime::local::status(self.config))?;
        let account = map_runtime(resolve_account_resolution(self.config))?;
        let account_reason = if account.resolved_account.is_some() {
            None
        } else {
            Some(map_runtime(unresolved_account_reason(self.config))?)
        };
        json_operation_result::<HealthCheckRunResult>(json!({
            "state": if store.state == "ready" && account.resolved_account.is_some() { "ready" } else { "needs_attention" },
            "checks": {
                "workspace": {
                    "state": "ready",
                    "profile": self.config.paths.profile,
                },
                "store": {
                    "state": store.state,
                    "reason": store.reason,
                },
                "account": {
                    "state": if account.resolved_account.is_some() { "ready" } else { "unconfigured" },
                    "reason": account_reason,
                },
            },
        }))
    }
}

impl OperationService<ConfigGetRequest> for CoreOperationService<'_> {
    type Result = ConfigGetResult;

    fn execute(
        &self,
        _request: OperationRequest<ConfigGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        json_operation_result::<ConfigGetResult>(json!({
            "output": {
                "format": self.config.output.format.as_str(),
                "verbosity": self.config.output.verbosity.as_str(),
                "color": self.config.output.color,
                "dry_run": self.config.output.dry_run,
            },
            "interaction": {
                "input_enabled": self.config.interaction.input_enabled,
                "prompts_allowed": self.config.interaction.prompts_allowed,
                "confirmations_allowed": self.config.interaction.confirmations_allowed,
            },
            "paths": {
                "profile": self.config.paths.profile,
                "app_config_path": self.config.paths.app_config_path.display().to_string(),
                "workspace_config_path": self.config.paths.workspace_config_path.as_ref().map(|path| path.display().to_string()),
                "app_data_root": self.config.paths.app_data_root.display().to_string(),
                "app_logs_root": self.config.paths.app_logs_root.display().to_string(),
            },
            "account": {
                "selector": self.config.account.selector,
                "store_path": self.config.account.store_path.display().to_string(),
                "secrets_dir": self.config.account.secrets_dir.display().to_string(),
            },
            "relay": {
                "count": self.config.relay.urls.len(),
                "urls": self.config.relay.urls,
                "source": self.config.relay.source.as_str(),
            },
            "local": {
                "root": self.config.local.root.display().to_string(),
                "replica_db_path": self.config.local.replica_db_path.display().to_string(),
                "backups_dir": self.config.local.backups_dir.display().to_string(),
                "exports_dir": self.config.local.exports_dir.display().to_string(),
            },
        }))
    }
}

impl OperationService<AccountCreateRequest> for CoreOperationService<'_> {
    type Result = AccountCreateResult;

    fn execute(
        &self,
        request: OperationRequest<AccountCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            let secret_backend = secret_backend_status(self.config);
            if secret_backend.state != "ready" {
                return Err(OperationAdapterError::OperationUnavailable {
                    operation_id: request.operation_id().to_owned(),
                    message: secret_backend
                        .reason
                        .unwrap_or_else(|| "account secret backend is not available".to_owned()),
                });
            }
            return json_operation_result::<AccountCreateResult>(json!({
                "state": "dry_run",
                "store_path": self.config.account.store_path.display().to_string(),
                "secrets_dir": self.config.account.secrets_dir.display().to_string(),
                "secret_backend": {
                    "state": secret_backend.state,
                    "active_backend": secret_backend.active_backend,
                    "used_fallback": secret_backend.used_fallback,
                },
            }));
        }

        let result = map_runtime(create_or_migrate_default_account(self.config))?;
        json_operation_result::<AccountCreateResult>(json!({
            "state": match result.mode {
                crate::runtime::accounts::AccountCreateMode::Created => "created",
                crate::runtime::accounts::AccountCreateMode::Migrated => "migrated",
            },
            "account": account_summary_view(&result.account),
        }))
    }
}

impl OperationService<AccountImportRequest> for CoreOperationService<'_> {
    type Result = AccountImportResult;

    fn execute(
        &self,
        request: OperationRequest<AccountImportRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let path = required_path(&request, "path")?;
        let make_default = bool_input(&request, "default").unwrap_or(false);
        if request.context.dry_run {
            let account = map_expected_runtime(
                request.operation_id(),
                preview_public_identity_import(self.config, path.as_path(), make_default),
            )?;
            return json_operation_result::<AccountImportResult>(json!({
                "state": "dry_run",
                "path": path.display().to_string(),
                "default": make_default,
                "account": account_summary_view(&account),
            }));
        }
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let account = map_expected_runtime(
            request.operation_id(),
            import_public_identity(self.config, path.as_path(), make_default),
        )?;
        json_operation_result::<AccountImportResult>(json!({
            "state": "imported",
            "account": account_summary_view(&account),
        }))
    }
}

impl OperationService<AccountGetRequest> for CoreOperationService<'_> {
    type Result = AccountGetResult;

    fn execute(
        &self,
        request: OperationRequest<AccountGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let scoped;
        let config = if let Some(selector) = string_input(&request, "selector") {
            scoped = selected_config(self.config, selector);
            &scoped
        } else {
            self.config
        };
        let resolution = resolve_account_resolution(config).map_err(|error| {
            OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
        })?;
        let reason = if resolution.resolved_account.is_some() {
            None
        } else {
            Some(map_runtime(unresolved_account_reason(config))?)
        };
        json_operation_result::<AccountGetResult>(json!({
            "state": if resolution.resolved_account.is_some() { "ready" } else { "unconfigured" },
            "reason": reason,
            "account_resolution": account_resolution_view(&resolution),
        }))
    }
}

impl OperationService<AccountListRequest> for CoreOperationService<'_> {
    type Result = AccountListResult;

    fn execute(
        &self,
        _request: OperationRequest<AccountListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let snapshot = map_runtime(snapshot(self.config))?;
        let accounts = snapshot
            .accounts
            .iter()
            .map(account_summary_view)
            .collect::<Vec<_>>();
        json_operation_result::<AccountListResult>(json!({
            "source": crate::runtime::accounts::SHARED_ACCOUNT_STORE_SOURCE,
            "count": accounts.len(),
            "accounts": accounts,
        }))
    }
}

impl OperationService<AccountRemoveRequest> for CoreOperationService<'_> {
    type Result = AccountRemoveResult;

    fn execute(
        &self,
        request: OperationRequest<AccountRemoveRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let selector = required_string(&request, "selector")?;
        if request.context.dry_run {
            let preview =
                preview_account_removal(self.config, selector.as_str()).map_err(|error| {
                    OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
                })?;
            return json_operation_result::<AccountRemoveResult>(json!({
                "state": "dry_run",
                "removed_account": account_summary_view(&preview.account),
                "default_would_clear": preview.default_would_clear,
                "remaining_account_count": preview.remaining_account_count,
            }));
        }
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let result = remove_account(self.config, selector.as_str()).map_err(|error| {
            OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
        })?;
        json_operation_result::<AccountRemoveResult>(json!({
            "state": "removed",
            "removed_account": account_summary_view(&result.removed_account),
            "default_cleared": result.default_cleared,
            "remaining_account_count": result.remaining_account_count,
        }))
    }
}

impl OperationService<AccountSelectionGetRequest> for CoreOperationService<'_> {
    type Result = AccountSelectionGetResult;

    fn execute(
        &self,
        _request: OperationRequest<AccountSelectionGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let resolution = map_runtime(resolve_account_resolution(self.config))?;
        json_operation_result::<AccountSelectionGetResult>(json!({
            "account_resolution": account_resolution_view(&resolution),
        }))
    }
}

impl OperationService<AccountSelectionUpdateRequest> for CoreOperationService<'_> {
    type Result = AccountSelectionUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<AccountSelectionUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let selector = required_string(&request, "selector")?;
        if request.context.dry_run {
            let account =
                resolve_account_selector(self.config, selector.as_str()).map_err(|error| {
                    OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
                })?;
            return json_operation_result::<AccountSelectionUpdateResult>(json!({
                "state": "dry_run",
                "account": account_summary_view(&account),
            }));
        }

        let account = select_account(self.config, selector.as_str()).map_err(|error| {
            OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
        })?;
        json_operation_result::<AccountSelectionUpdateResult>(json!({
            "state": "default",
            "account": account_summary_view(&account),
        }))
    }
}

impl OperationService<AccountSelectionClearRequest> for CoreOperationService<'_> {
    type Result = AccountSelectionClearResult;

    fn execute(
        &self,
        request: OperationRequest<AccountSelectionClearRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            let resolution = map_runtime(resolve_account_resolution(self.config))?;
            let account_snapshot = map_runtime(snapshot(self.config))?;
            return json_operation_result::<AccountSelectionClearResult>(json!({
                "state": "dry_run",
                "cleared_account": resolution.default_account.as_ref().map(account_summary_view),
                "remaining_account_count": account_snapshot.accounts.len(),
            }));
        }

        let result = map_runtime(clear_default_account(self.config))?;
        json_operation_result::<AccountSelectionClearResult>(json!({
            "state": if result.cleared_account.is_some() { "cleared" } else { "already_clear" },
            "cleared_account": result.cleared_account.as_ref().map(account_summary_view),
            "remaining_account_count": result.remaining_account_count,
        }))
    }
}

impl OperationService<StoreInitRequest> for CoreOperationService<'_> {
    type Result = StoreInitResult;

    fn execute(
        &self,
        request: OperationRequest<StoreInitRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            let view = map_runtime(crate::runtime::local::init_preflight(self.config))?;
            return serialized_operation_result::<StoreInitResult, _>(&view);
        }

        let view = map_runtime(crate::runtime::local::init(self.config))?;
        serialized_operation_result::<StoreInitResult, _>(&view)
    }
}

impl OperationService<StoreStatusGetRequest> for CoreOperationService<'_> {
    type Result = StoreStatusGetResult;

    fn execute(
        &self,
        _request: OperationRequest<StoreStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::local::status(self.config))?;
        serialized_operation_result::<StoreStatusGetResult, _>(&view)
    }
}

impl OperationService<StoreExportRequest> for CoreOperationService<'_> {
    type Result = StoreExportResult;

    fn execute(
        &self,
        request: OperationRequest<StoreExportRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let output = optional_path(&request, "output")
            .unwrap_or_else(|| self.config.local.exports_dir.join("store-export.json"));
        let format = match string_input(&request, "format").as_deref() {
            Some("ndjson") => LocalExportFormatArg::Ndjson,
            Some("json") | None => LocalExportFormatArg::Json,
            Some(other) => {
                return Err(invalid_input(
                    request.operation_id(),
                    format!("format must be `json` or `ndjson`, got `{other}`"),
                ));
            }
        };
        if request.context.dry_run {
            return Err(invalid_input(
                request.operation_id(),
                "`radroots store export` does not support --dry-run".to_owned(),
            ));
        }

        let view = map_runtime(crate::runtime::local::export(
            self.config,
            format,
            output.as_path(),
        ))?;
        serialized_operation_result::<StoreExportResult, _>(&view)
    }
}

impl OperationService<StoreBackupCreateRequest> for CoreOperationService<'_> {
    type Result = StoreBackupCreateResult;

    fn execute(
        &self,
        request: OperationRequest<StoreBackupCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let output = optional_path(&request, "output")
            .unwrap_or_else(|| self.config.local.backups_dir.join("store-backup.json"));
        if request.context.dry_run {
            let view = map_expected_runtime(
                request.operation_id(),
                crate::runtime::local::backup_preflight(self.config, output.as_path()),
            )?;
            return local_backup_result(request.operation_id(), &view);
        }

        let view = map_expected_runtime(
            request.operation_id(),
            crate::runtime::local::backup(self.config, output.as_path()),
        )?;
        local_backup_result(request.operation_id(), &view)
    }
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

fn map_expected_runtime<T>(
    operation_id: &str,
    result: Result<T, RuntimeError>,
) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::runtime_failure(operation_id, error))
}

fn local_backup_result(
    operation_id: &str,
    view: &LocalBackupView,
) -> Result<OperationResult<StoreBackupCreateResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => {
            serialized_operation_result::<StoreBackupCreateResult, _>(view)
        }
        disposition => Err(OperationAdapterError::from_command_disposition(
            operation_id,
            disposition,
            view.reason.clone().unwrap_or_else(|| match disposition {
                CommandDisposition::Success => "store backup succeeded".to_owned(),
                CommandDisposition::NotFound => "store backup target was not found".to_owned(),
                CommandDisposition::ValidationFailed => "store backup validation failed".to_owned(),
                CommandDisposition::Unconfigured => "store backup is unconfigured".to_owned(),
                CommandDisposition::ExternalUnavailable => "store backup is unavailable".to_owned(),
                CommandDisposition::Unsupported => "store backup is unsupported".to_owned(),
                CommandDisposition::InternalError => "store backup failed".to_owned(),
            }),
        )),
    }
}

fn selected_config(config: &RuntimeConfig, selector: String) -> RuntimeConfig {
    let mut config = config.clone();
    config.account.selector = Some(selector);
    config
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

    use super::CoreOperationService;
    use crate::operation_adapter::{
        AccountCreateRequest, AccountImportRequest, AccountListRequest, AccountRemoveRequest,
        OperationAdapter, OperationContext, OperationData, OperationRequest, StoreStatusGetRequest,
        WorkspaceGetRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime::logging::LoggingState;

    #[test]
    fn core_service_envelopes_workspace_get() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let logging = LoggingState {
            initialized: true,
            current_file: None,
        };
        let service = OperationAdapter::new(CoreOperationService::new(&config, &logging));
        let request =
            OperationRequest::new(OperationContext::default(), WorkspaceGetRequest::default())
                .expect("workspace request");
        let result = service.execute(request).expect("workspace result");
        let envelope = result
            .to_envelope(OperationContext::default().envelope_context("req_workspace"))
            .expect("workspace envelope");

        assert_eq!(envelope.operation_id, "workspace.get");
        assert_eq!(envelope.kind, "workspace.get");
        assert_eq!(envelope.request_id, "req_workspace");
        assert_eq!(envelope.result["profile"], "interactive_user");
        assert_eq!(
            envelope.result["replica_db_path"],
            config.local.replica_db_path.display().to_string()
        );
    }

    #[test]
    fn core_service_backs_store_status() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let logging = LoggingState {
            initialized: false,
            current_file: None,
        };
        let service = OperationAdapter::new(CoreOperationService::new(&config, &logging));
        let request = OperationRequest::new(
            OperationContext::default(),
            StoreStatusGetRequest::default(),
        )
        .expect("store status request");
        let result = service.execute(request).expect("store status result");
        let envelope = result
            .to_envelope(OperationContext::default().envelope_context("req_store"))
            .expect("store status envelope");

        assert_eq!(envelope.operation_id, "store.status.get");
        assert_eq!(envelope.result["state"], "unconfigured");
        assert_eq!(envelope.result["replica_db"], "missing");
    }

    #[test]
    fn core_service_backs_account_create_and_list() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let logging = LoggingState {
            initialized: false,
            current_file: None,
        };
        let service = OperationAdapter::new(CoreOperationService::new(&config, &logging));
        let create =
            OperationRequest::new(OperationContext::default(), AccountCreateRequest::default())
                .expect("account create request");
        let create_result = service.execute(create).expect("account create result");
        let create_envelope = create_result
            .to_envelope(OperationContext::default().envelope_context("req_create"))
            .expect("account create envelope");

        assert_eq!(create_envelope.operation_id, "account.create");
        assert_eq!(create_envelope.result["state"], "created");
        assert!(create_envelope.result["account"]["id"].is_string());

        let list =
            OperationRequest::new(OperationContext::default(), AccountListRequest::default())
                .expect("account list request");
        let list_result = service.execute(list).expect("account list result");
        let list_envelope = list_result
            .to_envelope(OperationContext::default().envelope_context("req_list"))
            .expect("account list envelope");

        assert_eq!(list_envelope.operation_id, "account.list");
        assert_eq!(list_envelope.result["count"], 1);
        assert_eq!(list_envelope.result["accounts"][0]["is_default"], true);
    }

    #[test]
    fn core_required_account_approvals_return_approval_error() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let logging = LoggingState {
            initialized: false,
            current_file: None,
        };
        let service = OperationAdapter::new(CoreOperationService::new(&config, &logging));
        let import = OperationRequest::new(
            OperationContext::default(),
            AccountImportRequest::from_data(data(&[("path", "account.json")])),
        )
        .expect("account import request");
        let import_error = service.execute(import).expect_err("approval required");
        assert_eq!(import_error.to_output_error().code, "approval_required");
        assert_eq!(import_error.to_output_error().exit_code, 6);

        let remove = OperationRequest::new(
            OperationContext::default(),
            AccountRemoveRequest::from_data(data(&[("selector", "acct_test")])),
        )
        .expect("account remove request");
        let remove_error = service.execute(remove).expect_err("approval required");
        assert_eq!(remove_error.to_output_error().code, "approval_required");
        assert_eq!(remove_error.to_output_error().exit_code, 6);
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
