use std::path::PathBuf;

use radroots_transport::RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE;
use serde::Serialize;
use serde_json::{Value, json};

use crate::ops::{
    AccountCreateRequest, AccountCreateResult, AccountImportRequest, AccountImportResult,
    AccountListRequest, AccountListResult, AccountRemoveRequest, AccountRemoveResult,
    AccountSelectRequest, AccountSelectResult, HealthInspectRequest, HealthInspectResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, ProfileInspectRequest,
    ProfileInspectResult, ProfileResetRequest, ProfileResetResult, StoreBackupRequest,
    StoreBackupResult, StoreInspectRequest, StoreInspectResult, StoreRestoreRequest,
    StoreRestoreResult,
};
use crate::out::envelope::OutputWarning;
use crate::runtime::RuntimeError;
use crate::runtime::account::{
    AccountResolution, AccountRuntimeFailure, account_resolution_view, account_summary_view,
    create_default_account, import_public_identity, preview_account_removal,
    preview_public_identity_import, remove_account, resolve_account_resolution,
    resolve_account_selector, secret_backend_status, select_account, snapshot,
};
use crate::runtime::config::{RuntimeConfig, SignerBackend, TransportProfileKind};
use crate::runtime::logging::LoggingState;
use crate::runtime::sdk::CliSdkAdapterError;
use crate::runtime::signer::resolve_signer_status;
use crate::view::runtime::{
    CommandDisposition, LocalBackupView, LocalRestoreView, PublishProviderRuntimeView,
    PublishRelayRuntimeView, PublishRuntimeView,
};

pub struct CoreOperationService<'a> {
    config: &'a RuntimeConfig,
    logging: &'a LoggingState,
}

impl<'a> CoreOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig, logging: &'a LoggingState) -> Self {
        Self { config, logging }
    }
}

impl OperationService<ProfileResetRequest> for CoreOperationService<'_> {
    type Result = ProfileResetResult;

    fn execute(
        &self,
        request: OperationRequest<ProfileResetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            let local = map_runtime(crate::runtime::store::init_preflight(self.config))?;
            return json_operation_result::<ProfileResetResult>(json!({
                "state": local.state,
                "profile": self.config.paths.profile,
                "local": local,
            }));
        }

        let local = map_runtime(crate::runtime::store::init(self.config))?;
        json_operation_result::<ProfileResetResult>(json!({
            "state": local.state,
            "profile": self.config.paths.profile,
            "local": local,
        }))
    }
}

impl OperationService<ProfileInspectRequest> for CoreOperationService<'_> {
    type Result = ProfileInspectResult;

    fn execute(
        &self,
        _request: OperationRequest<ProfileInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        json_operation_result::<ProfileInspectResult>(json!({
            "profile": self.config.paths.profile,
            "profile_source": self.config.paths.profile_source,
            "root_source": self.config.paths.root_source,
            "app_namespace": self.config.paths.app_namespace,
            "workspace_config_path": self.config.paths.workspace_config_path.as_ref().map(|path| path.display().to_string()),
            "app_config_path": self.config.paths.app_config_path.display().to_string(),
            "app_data_root": self.config.paths.app_data_root.display().to_string(),
            "shared_cache_root": self.config.paths.shared_cache_root.display().to_string(),
            "app_logs_root": self.config.paths.app_logs_root.display().to_string(),
            "local_root": self.config.local.root.display().to_string(),
            "replica_store_path": self.config.local.replica_store_path.display().to_string(),
        }))
    }
}

impl OperationService<HealthInspectRequest> for CoreOperationService<'_> {
    type Result = HealthInspectResult;

    fn execute(
        &self,
        request: OperationRequest<HealthInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let store = map_sdk_adapter(
            request.operation_id(),
            crate::runtime::store::status(self.config),
        )?;
        let account = map_runtime(resolve_account_resolution(self.config))?;
        let publish = publish_runtime_view(self.config, true, &account);
        let signer = signer_health_view(self.config, &account);
        let state = health_status_state(&store.state, &publish);
        let actions = health_actions(self.config, store.state.as_str(), &account, &publish);
        json_operation_result::<HealthInspectResult>(json!({
            "state": state,
            "store": store,
            "account_resolution": account_resolution_view(&account),
            "signer": signer,
            "publish": publish,
            "logging": {
                "initialized": self.logging.initialized,
                "current_file": self.logging.current_file.as_ref().map(|path| path.display().to_string()),
            },
            "actions": actions,
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
                },
            }));
        }

        let result = map_runtime(create_default_account(self.config))?;
        json_operation_result::<AccountCreateResult>(json!({
            "state": "created",
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
            "source": crate::runtime::account::SHARED_ACCOUNT_STORE_SOURCE,
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

        let resolved_farm_config =
            map_runtime(crate::runtime::farm_config::load(self.config, None))?;
        let result = remove_account(self.config, selector.as_str()).map_err(|error| {
            OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
        })?;
        let removed_account_id = result.removed_account.record.id().to_string();
        let farm_orphan_warning =
            account_remove_farm_orphan_warning(resolved_farm_config.as_ref(), &removed_account_id);
        let mut result_value = json!({
            "state": "removed",
            "removed_account": account_summary_view(&result.removed_account),
            "default_cleared": result.default_cleared,
            "remaining_account_count": result.remaining_account_count,
        });
        if let Some(warning) = farm_orphan_warning.as_ref() {
            result_value["warnings"] = json!([warning.result_value()]);
            result_value["actions"] = json!(warning.actions.clone());
        }
        let mut operation_result = json_operation_result::<AccountRemoveResult>(result_value)?;
        if let Some(warning) = farm_orphan_warning {
            operation_result.warnings.push(warning.output_warning());
        }
        Ok(operation_result)
    }
}

impl OperationService<AccountSelectRequest> for CoreOperationService<'_> {
    type Result = AccountSelectResult;

    fn execute(
        &self,
        request: OperationRequest<AccountSelectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let selector = required_string(&request, "selector")?;
        if request.context.dry_run {
            let account =
                resolve_account_selector(self.config, selector.as_str()).map_err(|error| {
                    OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
                })?;
            return json_operation_result::<AccountSelectResult>(json!({
                "state": "dry_run",
                "account": account_summary_view(&account),
            }));
        }

        let account = select_account(self.config, selector.as_str()).map_err(|error| {
            OperationAdapterError::unconfigured(request.operation_id(), error.to_string())
        })?;
        json_operation_result::<AccountSelectResult>(json!({
            "state": "default",
            "account": account_summary_view(&account),
        }))
    }
}

impl OperationService<StoreInspectRequest> for CoreOperationService<'_> {
    type Result = StoreInspectResult;

    fn execute(
        &self,
        request: OperationRequest<StoreInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_sdk_adapter(
            request.operation_id(),
            crate::runtime::store::status(self.config),
        )?;
        serialized_operation_result::<StoreInspectResult, _>(&view)
    }
}

impl OperationService<StoreBackupRequest> for CoreOperationService<'_> {
    type Result = StoreBackupResult;

    fn execute(
        &self,
        request: OperationRequest<StoreBackupRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let output = optional_path(&request, "output")
            .unwrap_or_else(|| self.config.local.backups_dir.join("sdk-store-backup"));
        if request.context.dry_run {
            let view = map_sdk_adapter(
                request.operation_id(),
                crate::runtime::store::backup_preflight(self.config, output.as_path()),
            )?;
            return local_backup_result(request.operation_id(), &view);
        }

        let view = map_sdk_adapter(
            request.operation_id(),
            crate::runtime::store::backup(self.config, output.as_path()),
        )?;
        local_backup_result(request.operation_id(), &view)
    }
}

impl OperationService<StoreRestoreRequest> for CoreOperationService<'_> {
    type Result = StoreRestoreResult;

    fn execute(
        &self,
        request: OperationRequest<StoreRestoreRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let source = required_path(&request, "source")?;
        let destination = optional_path(&request, "destination");
        let overwrite = bool_input(&request, "overwrite").unwrap_or(false);

        let view = map_sdk_adapter(
            request.operation_id(),
            crate::runtime::store::restore(
                self.config,
                source.as_path(),
                destination.as_deref(),
                overwrite,
                request.context.dry_run,
            ),
        )?;
        local_restore_result(request.operation_id(), &view)
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

fn map_sdk_adapter<T>(
    operation_id: &str,
    result: Result<T, CliSdkAdapterError>,
) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::sdk_adapter_failure(operation_id, error))
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
) -> Result<OperationResult<StoreBackupResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<StoreBackupResult, _>(view),
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

fn local_restore_result(
    operation_id: &str,
    view: &LocalRestoreView,
) -> Result<OperationResult<StoreRestoreResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<StoreRestoreResult, _>(view),
        disposition => Err(OperationAdapterError::from_command_disposition(
            operation_id,
            disposition,
            view.reason.clone().unwrap_or_else(|| match disposition {
                CommandDisposition::Success => "store restore succeeded".to_owned(),
                CommandDisposition::NotFound => "store restore source was not found".to_owned(),
                CommandDisposition::ValidationFailed => {
                    "store restore validation failed".to_owned()
                }
                CommandDisposition::Unconfigured => "store restore is unconfigured".to_owned(),
                CommandDisposition::ExternalUnavailable => {
                    "store restore is unavailable".to_owned()
                }
                CommandDisposition::Unsupported => "store restore is unsupported".to_owned(),
                CommandDisposition::InternalError => "store restore failed".to_owned(),
            }),
        )),
    }
}

fn publish_runtime_view(
    config: &RuntimeConfig,
    signed_write_required: bool,
    account: &AccountResolution,
) -> PublishRuntimeView {
    let relay_ready = !config.transport.nostr_relay_urls.is_empty();
    let source = config.transport.source.as_str().to_owned();
    let relay = PublishRelayRuntimeView {
        ready: relay_ready,
        count: config.transport.nostr_relay_urls.len(),
        source: config.transport.source.as_str().to_owned(),
    };

    match config.transport.profile {
        TransportProfileKind::Nostr | TransportProfileKind::MultiTarget => {
            let (state, executable, reason) =
                nostr_publish_readiness(config, relay_ready, signed_write_required, account);
            PublishRuntimeView {
                transport: config.transport.profile.as_str().to_owned(),
                source,
                transport_family: config.transport.profile.transport_family().to_owned(),
                state: state.to_owned(),
                executable,
                reason: reason.clone(),
                signed_write_required,
                relay,
                provider: PublishProviderRuntimeView {
                    provider_runtime_id: config.transport.profile.as_str().to_owned(),
                    state: state.to_owned(),
                    source: config.transport.source.as_str().to_owned(),
                    reason,
                },
            }
        }
        TransportProfileKind::LocalOnly => PublishRuntimeView {
            transport: config.transport.profile.as_str().to_owned(),
            source,
            transport_family: config.transport.profile.transport_family().to_owned(),
            state: "local_only".to_owned(),
            executable: false,
            reason: Some(
                "local_only transport profile does not perform network publish".to_owned(),
            ),
            signed_write_required,
            relay,
            provider: PublishProviderRuntimeView {
                provider_runtime_id: "local_only".to_owned(),
                state: "local_only".to_owned(),
                source: config.transport.source.as_str().to_owned(),
                reason: Some("local_only transport profile writes only to local state".to_owned()),
            },
        },
        TransportProfileKind::Reticulum => PublishRuntimeView {
            transport: config.transport.profile.as_str().to_owned(),
            source,
            transport_family: config.transport.profile.transport_family().to_owned(),
            state: "unavailable".to_owned(),
            executable: false,
            reason: Some(RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE.to_owned()),
            signed_write_required,
            relay,
            provider: PublishProviderRuntimeView {
                provider_runtime_id: "reticulum".to_owned(),
                state: "unavailable".to_owned(),
                source: config.transport.source.as_str().to_owned(),
                reason: Some(RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE.to_owned()),
            },
        },
    }
}

fn nostr_publish_readiness(
    config: &RuntimeConfig,
    relay_ready: bool,
    signed_write_required: bool,
    account: &AccountResolution,
) -> (&'static str, bool, Option<String>) {
    if !relay_ready {
        return (
            "unconfigured",
            false,
            Some(
                "Nostr transport profile requires at least one configured Nostr relay for writes"
                    .to_owned(),
            ),
        );
    }

    if !signed_write_required {
        return ("ready", true, None);
    }

    if matches!(config.signer.backend, SignerBackend::Myc) {
        let signer = resolve_signer_status(config);
        return if signer.state == "ready" {
            ("ready", true, None)
        } else {
            ("unconfigured", false, signer.reason)
        };
    }

    let Some(resolved_account) = account.resolved_account.as_ref() else {
        return (
            "unconfigured",
            false,
            Some(
                "Nostr transport profile requires a selected or default write-capable local account for signed writes"
                    .to_owned(),
            ),
        );
    };

    if !resolved_account.write_capable {
        return (
            "unconfigured",
            false,
            Some(AccountRuntimeFailure::watch_only(&resolved_account.record.id()).to_string()),
        );
    }

    ("ready", true, None)
}

fn signer_health_view(config: &RuntimeConfig, account: &AccountResolution) -> Value {
    match config.signer.backend {
        SignerBackend::Local => {
            let write_capable = account
                .resolved_account
                .as_ref()
                .map(|account| account.write_capable)
                .unwrap_or(false);
            json!({
                "state": if write_capable { "ready" } else { "unconfigured" },
                "backend": config.signer.backend.as_str(),
                "write_capable_account": write_capable,
                "reason": if write_capable {
                    Value::Null
                } else {
                    json!("local signer requires a selected or default write-capable local account")
                },
            })
        }
        SignerBackend::Myc => {
            let signer = resolve_signer_status(config);
            json!({
                "state": signer.state,
                "backend": config.signer.backend.as_str(),
                "write_capable_account": signer.reason.is_none(),
                "reason": signer.reason,
                "binding_state": signer.binding.state,
            })
        }
    }
}

fn health_status_state(store_state: &str, publish: &PublishRuntimeView) -> &'static str {
    if store_state == "ready" && publish_runtime_ready(publish) {
        "ready"
    } else {
        "needs_attention"
    }
}

fn publish_runtime_ready(publish: &PublishRuntimeView) -> bool {
    !publish.signed_write_required || publish.executable
}

fn health_actions(
    config: &RuntimeConfig,
    store_state: &str,
    account: &AccountResolution,
    publish: &PublishRuntimeView,
) -> Vec<String> {
    let mut actions = Vec::new();
    if store_state != "ready" {
        push_unique(&mut actions, "radroots store inspect");
    }
    if let Some(resolved) = account.resolved_account.as_ref() {
        if !resolved.write_capable {
            push_unique(&mut actions, "radroots account create");
        }
    } else {
        push_unique(&mut actions, "radroots account create");
    }
    for action in publish_recovery_actions(config, account, publish) {
        push_unique(&mut actions, action);
    }
    actions
}

#[derive(Debug, Clone)]
struct AccountRemoveFarmOrphanWarning {
    message: String,
    subject_account_id: String,
    farm_config_scope: String,
    farm_config_path: String,
    actions: Vec<String>,
}

impl AccountRemoveFarmOrphanWarning {
    const CODE: &'static str = "farm_bound_seller_orphaned";

    fn result_value(&self) -> Value {
        json!({
            "code": Self::CODE,
            "message": self.message.clone(),
            "subject_account_id": self.subject_account_id.clone(),
            "farm_config": {
                "scope": self.farm_config_scope.clone(),
                "path": self.farm_config_path.clone(),
            },
            "actions": self.actions.clone(),
        })
    }

    fn output_warning(&self) -> OutputWarning {
        OutputWarning {
            code: Self::CODE.to_owned(),
            message: self.message.clone(),
        }
    }
}

fn account_remove_farm_orphan_warning(
    resolved: Option<&crate::runtime::farm_config::ResolvedFarmConfig>,
    removed_account_id: &str,
) -> Option<AccountRemoveFarmOrphanWarning> {
    let resolved = resolved?;
    if resolved.document.selection.account != removed_account_id {
        return None;
    }
    Some(AccountRemoveFarmOrphanWarning {
        message: format!(
            "removed account `{removed_account_id}` is still bound as the farm seller account"
        ),
        subject_account_id: removed_account_id.to_owned(),
        farm_config_scope: resolved.scope.as_str().to_owned(),
        farm_config_path: resolved.path.display().to_string(),
        actions: crate::runtime::farm::farm_bound_seller_recovery_actions("<selector>"),
    })
}

fn publish_recovery_actions(
    config: &RuntimeConfig,
    account: &AccountResolution,
    publish: &PublishRuntimeView,
) -> Vec<String> {
    if publish.state == "ready" {
        return Vec::new();
    }

    let mut actions = Vec::new();
    match config.transport.profile {
        TransportProfileKind::Nostr | TransportProfileKind::MultiTarget => {
            if config.transport.nostr_relay_urls.is_empty() {
                push_unique(
                    &mut actions,
                    "radroots transport config update --kind nostr --nostr-relay wss://relay.example.com",
                );
            }
            if publish.signed_write_required {
                if matches!(config.signer.backend, SignerBackend::Myc) {
                    push_unique(&mut actions, "radroots signer status");
                } else if let Some(resolved) = account.resolved_account.as_ref() {
                    if !resolved.write_capable {
                        push_unique(&mut actions, "radroots account create");
                    }
                } else {
                    push_unique(&mut actions, "radroots account create");
                }
            }
        }
        TransportProfileKind::LocalOnly => {
            push_unique(
                &mut actions,
                "radroots transport config update --kind nostr --nostr-relay wss://relay.example.com",
            );
        }
        TransportProfileKind::Reticulum => {
            push_unique(
                &mut actions,
                "radroots transport config update --kind nostr --nostr-relay wss://relay.example.com",
            );
        }
    }
    actions
}

fn push_unique(actions: &mut Vec<String>, action: impl Into<String>) {
    let action = action.into();
    if !actions.contains(&action) {
        actions.push(action);
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
