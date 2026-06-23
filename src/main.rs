#![forbid(unsafe_code)]

mod cli;
mod ops;
mod out;
mod registry;
mod runtime;
mod view;

use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde_json::Value;

use crate::cli::input::runtime_invocation_args_from_target;
use crate::cli::{TargetCliArgs, TargetOutputFormat};
use crate::ops::exec::{
    BasketOperationService, CoreOperationService, FarmOperationService, ListingOperationService,
    MarketOperationService, OrderOperationService, RuntimeOperationService,
    ValidationOperationService,
};
use crate::ops::{
    OperationAdapter, OperationAdapterError, OperationNetworkMode, OperationOutputFormat,
    OperationRequest, OperationRequestPayload, OperationResultPayload, OperationService,
    TargetOperationRequest,
};
use crate::out::envelope::OutputEnvelope;
use crate::registry::{NetworkRequirement, network_requirement, requires_local_signer_mode};
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use crate::runtime::logging::initialize_logging;

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            error.exit_code()
        }
    }
}

fn run() -> Result<ExitCode, runtime::RuntimeError> {
    debug_assert!(registry::registry_linkage_is_valid());
    debug_assert!(ops::adapter_registry_linkage_is_valid());
    let args = TargetCliArgs::parse();
    let request =
        TargetOperationRequest::from_target_args(&args).map_err(operation_config_error)?;
    if let Err(error) = validate_pre_runtime_request_contract(&request) {
        let envelope = failure_envelope(&request, error);
        render_envelope(&envelope, args.format)?;
        return Ok(envelope_exit_code(&envelope));
    }
    let config = RuntimeConfig::from_system(&runtime_invocation_args_from_target(&args))?;
    let logging = initialize_logging(&config.logging)?;
    let envelope = match validate_request_contract(&request, &config) {
        Ok(()) => execute_request(request, &config, &logging),
        Err(error) => failure_envelope(&request, error),
    };
    render_envelope(&envelope, args.format)?;
    Ok(envelope_exit_code(&envelope))
}

fn execute_request(
    request: TargetOperationRequest,
    config: &RuntimeConfig,
    logging: &runtime::logging::LoggingState,
) -> OutputEnvelope {
    match request {
        TargetOperationRequest::WorkspaceInit(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::WorkspaceGet(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::HealthStatusGet(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::HealthCheckRun(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::ConfigGet(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountCreate(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountImport(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountAttachSecret(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountGet(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountList(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountRemove(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountSelectionGet(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountSelectionUpdate(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::AccountSelectionClear(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::StoreInit(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::StoreStatusGet(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::StoreExport(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::StoreBackupCreate(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::StoreBackupRestore(request) => {
            execute_with(CoreOperationService::new(config, logging), request)
        }
        TargetOperationRequest::SignerStatusGet(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::RelayList(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::SyncStatusGet(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::SyncPull(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::SyncPush(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::SyncWatch(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::FarmCreate(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmGet(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmRebind(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmProfileUpdate(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmLocationUpdate(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmFulfillmentUpdate(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmReadinessCheck(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmPublish(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::ListingCreate(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingGet(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingList(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingAppList(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingAppExport(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingUpdate(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingValidate(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingRebind(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingPublish(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingArchive(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::MarketRefresh(request) => {
            execute_with(MarketOperationService::new(config), request)
        }
        TargetOperationRequest::MarketProductSearch(request) => {
            execute_with(MarketOperationService::new(config), request)
        }
        TargetOperationRequest::MarketListingGet(request) => {
            execute_with(MarketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketCreate(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketGet(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketList(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketItemAdd(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketItemUpdate(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketItemRemove(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketAdjustmentAdd(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketAdjustmentRemove(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketValidate(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::BasketQuoteCreate(request) => {
            execute_with(BasketOperationService::new(config), request)
        }
        TargetOperationRequest::OrderSubmit(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderGet(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderList(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderAppList(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderAppExport(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderRebind(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderAccept(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderDecline(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderCancel(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderRevisionPropose(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderRevisionAccept(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderRevisionDecline(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderStatusGet(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderEventList(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderEventWatch(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::ValidationReceiptGet(request) => {
            execute_with(ValidationOperationService::new(config), request)
        }
        TargetOperationRequest::ValidationReceiptList(request) => {
            execute_with(ValidationOperationService::new(config), request)
        }
        TargetOperationRequest::ValidationReceiptVerify(request) => {
            execute_with(ValidationOperationService::new(config), request)
        }
    }
}

fn execute_with<S, P>(service: S, request: OperationRequest<P>) -> OutputEnvelope
where
    S: OperationService<P>,
    P: OperationRequestPayload,
    S::Result: OperationResultPayload,
{
    let operation_id = request.operation_id().to_owned();
    let envelope_context = request
        .context
        .envelope_context(next_request_id(&operation_id));
    match OperationAdapter::new(service)
        .execute(request)
        .and_then(|result| result.to_envelope(envelope_context.clone()))
    {
        Ok(envelope) => envelope,
        Err(error) => {
            OutputEnvelope::failure(operation_id, error.to_output_error(), envelope_context)
        }
    }
}

fn validate_request_contract(
    request: &TargetOperationRequest,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError> {
    validate_pre_runtime_request_contract(request)?;
    validate_publish_transport_contract(request, config)?;
    validate_signer_mode_contract(request, config)?;
    validate_network_contract(request, config)?;
    Ok(())
}

fn validate_pre_runtime_request_contract(
    request: &TargetOperationRequest,
) -> Result<(), OperationAdapterError> {
    let spec = request.spec();
    if matches!(
        request.context().output_format,
        OperationOutputFormat::Ndjson
    ) && !spec.supports_ndjson
    {
        return Err(OperationAdapterError::InvalidInput {
            operation_id: spec.operation_id.to_owned(),
            message: format!("`{}` does not support --format ndjson", spec.cli_path),
        });
    }
    if request.context().dry_run && !spec.supports_dry_run {
        return Err(OperationAdapterError::InvalidInput {
            operation_id: spec.operation_id.to_owned(),
            message: format!("`{}` does not support --dry-run", spec.cli_path),
        });
    }
    Ok(())
}

fn validate_signer_mode_contract(
    request: &TargetOperationRequest,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError> {
    let spec = request.spec();
    if matches!(config.signer.backend, SignerBackend::Myc)
        && requires_local_signer_mode_for_publish_transport(spec.operation_id, config)
    {
        return Err(OperationAdapterError::SignerModeDeferred {
            operation_id: spec.operation_id.to_owned(),
            message: format!(
                "`{}` cannot run with signer mode `myc`; use signer mode `local`",
                spec.cli_path
            ),
        });
    }
    Ok(())
}

fn validate_network_contract(
    request: &TargetOperationRequest,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError> {
    let spec = request.spec();
    let requirement = network_requirement(spec.operation_id);
    match request.context().network_mode {
        OperationNetworkMode::Default => Ok(()),
        OperationNetworkMode::Offline => {
            if allows_offline_local_mutation(spec.operation_id) {
                return Ok(());
            }
            if let NetworkRequirement::External {
                dry_run_requires_network,
            } = requirement
                && (!request.context().dry_run || dry_run_requires_network)
            {
                return Err(OperationAdapterError::OfflineForbidden {
                    operation_id: spec.operation_id.to_owned(),
                    message: format!(
                        "`{}` requires relay, provider, or workflow network access",
                        spec.cli_path
                    ),
                });
            }
            Ok(())
        }
        OperationNetworkMode::Online => {
            if let NetworkRequirement::External {
                dry_run_requires_network,
            } = requirement
                && (!request.context().dry_run || dry_run_requires_network)
                && requires_pre_runtime_relay_target(spec.operation_id)
                && config.relay.urls.is_empty()
            {
                return Err(OperationAdapterError::NetworkUnavailable {
                    operation_id: spec.operation_id.to_owned(),
                    message: format!(
                        "`{}` requires at least one configured relay for online execution",
                        spec.cli_path
                    ),
                });
            }
            Ok(())
        }
    }
}

fn requires_local_signer_mode_for_publish_transport(
    operation_id: &str,
    config: &RuntimeConfig,
) -> bool {
    let _ = config;
    requires_local_signer_mode(operation_id)
}

fn requires_pre_runtime_relay_target(operation_id: &str) -> bool {
    !is_publish_transport_routed_operation(operation_id)
}

fn allows_offline_local_mutation(operation_id: &str) -> bool {
    matches!(operation_id, "listing.publish")
}

fn validate_publish_transport_contract(
    request: &TargetOperationRequest,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError> {
    let _ = request;
    let _ = config;
    Ok(())
}

fn is_publish_transport_routed_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "farm.publish" | "listing.publish" | "listing.update" | "listing.archive"
    )
}

fn failure_envelope(
    request: &TargetOperationRequest,
    error: OperationAdapterError,
) -> OutputEnvelope {
    OutputEnvelope::failure(
        request.operation_id(),
        error.to_output_error(),
        request
            .context()
            .envelope_context(next_request_id(request.operation_id())),
    )
}

fn next_request_id(operation_id: &str) -> String {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!(
        "req_{}_{}_{}_{}",
        operation_id.replace('.', "_"),
        std::process::id(),
        timestamp,
        sequence
    )
}

fn render_envelope(
    envelope: &OutputEnvelope,
    format: TargetOutputFormat,
) -> Result<(), runtime::RuntimeError> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    match format {
        TargetOutputFormat::Human => {
            render_human_envelope(&mut handle, envelope)?;
        }
        TargetOutputFormat::Json => {
            serde_json::to_writer_pretty(&mut handle, envelope)?;
        }
        TargetOutputFormat::Ndjson => {
            for frame in envelope.to_ndjson_frames() {
                serde_json::to_writer(&mut handle, &frame)?;
                writeln!(handle)?;
            }
            return Ok(());
        }
    }
    writeln!(handle)?;
    Ok(())
}

fn render_human_envelope(
    handle: &mut impl Write,
    envelope: &OutputEnvelope,
) -> Result<(), runtime::RuntimeError> {
    writeln!(
        handle,
        "{}: {}",
        envelope.operation_id,
        human_envelope_status(envelope)
    )?;
    writeln!(handle, "request_id: {}", envelope.request_id)?;
    if let Some(error) = envelope.errors.first() {
        writeln!(handle, "error: {}", error.code)?;
        writeln!(handle, "message: {}", error.message)?;
    }
    let display = human_display_source(envelope);
    if !envelope.errors.is_empty()
        && let Some(state) = human_state(display)
    {
        writeln!(handle, "state: {state}")?;
    }
    if let Some(mode) = human_publish_transport(display) {
        writeln!(handle, "publish_transport: {mode}")?;
    }
    if let Some(state) = human_publish_state(display) {
        writeln!(handle, "publish_state: {state}")?;
    }
    if let Some(state) = human_proof_state(display) {
        writeln!(handle, "proof_state: {state}")?;
    }
    if let Some(system) = human_proof_system(display) {
        writeln!(handle, "proof_system: {system}")?;
    }
    if let Some(verified) = human_cryptographic_proof_verified(display) {
        writeln!(handle, "cryptographic_proof_verified: {verified}")?;
    }
    if let Some(reason) = human_reason(display) {
        writeln!(handle, "reason: {reason}")?;
    }
    let actions = human_actions(envelope, display);
    if !actions.is_empty() {
        writeln!(handle, "next:")?;
        for action in actions {
            writeln!(handle, "- {action}")?;
        }
    }
    Ok(())
}

fn human_display_source(envelope: &OutputEnvelope) -> &Value {
    if !envelope.result.is_null() {
        return &envelope.result;
    }
    envelope
        .errors
        .first()
        .and_then(|error| error.detail.as_ref())
        .unwrap_or(&envelope.result)
}

fn human_state(result: &Value) -> Option<&str> {
    human_string_path(result, &["state"])
}

fn human_publish_transport(result: &Value) -> Option<&str> {
    human_string_path(result, &["publish", "mode"])
        .or_else(|| human_string_path(result, &["checks", "publish", "mode"]))
        .or_else(|| human_string_path(result, &["publish_transport"]))
}

fn human_publish_state(result: &Value) -> Option<&str> {
    human_string_path(result, &["publish", "state"])
        .or_else(|| human_string_path(result, &["checks", "publish", "state"]))
        .or_else(|| human_string_path(result, &["publish_state"]))
}

fn human_proof_state(result: &Value) -> Option<&str> {
    human_string_path(result, &["proof_verification", "state"])
        .or_else(|| human_string_path(result, &["proof_verification_state"]))
}

fn human_proof_system(result: &Value) -> Option<&str> {
    human_string_path(result, &["proof_verification", "proof_system"])
        .or_else(|| human_string_path(result, &["receipt", "proof", "system"]))
        .or_else(|| human_string_path(result, &["proof_system"]))
}

fn human_cryptographic_proof_verified(result: &Value) -> Option<bool> {
    human_bool_path(
        result,
        &["proof_verification", "cryptographic_proof_verified"],
    )
}

fn human_reason(result: &Value) -> Option<&str> {
    human_string_path(result, &["reason"])
        .or_else(|| human_string_path(result, &["publish", "reason"]))
        .or_else(|| human_string_path(result, &["checks", "publish", "reason"]))
        .or_else(|| human_string_path(result, &["store", "reason"]))
        .or_else(|| human_string_path(result, &["checks", "store", "reason"]))
        .or_else(|| human_string_path(result, &["checks", "account", "reason"]))
}

fn human_actions(envelope: &OutputEnvelope, display: &Value) -> Vec<String> {
    let mut actions = display
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if actions.is_empty() {
        actions = envelope
            .next_actions
            .iter()
            .map(|action| {
                action
                    .command
                    .clone()
                    .or_else(|| action.description.clone())
                    .unwrap_or_else(|| action.label.clone())
            })
            .collect();
    }
    actions.into_iter().fold(Vec::new(), |mut unique, action| {
        if !unique.contains(&action) {
            unique.push(action);
        }
        unique
    })
}

fn human_string_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().filter(|value| !value.trim().is_empty())
}

fn human_bool_path(value: &Value, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_bool()
}

fn human_envelope_status(envelope: &OutputEnvelope) -> &str {
    if !envelope.errors.is_empty() {
        return "error";
    }
    if let Some(state) = envelope
        .result
        .get("state")
        .and_then(|value| value.as_str())
    {
        return state;
    }
    if envelope.dry_run {
        return "dry_run";
    }
    "ok"
}

fn envelope_exit_code(envelope: &OutputEnvelope) -> ExitCode {
    envelope
        .errors
        .first()
        .map(|error| ExitCode::from(error.exit_code))
        .unwrap_or_else(|| ExitCode::from(0))
}

fn operation_config_error(error: OperationAdapterError) -> runtime::RuntimeError {
    runtime::RuntimeError::Config(error.to_string())
}
