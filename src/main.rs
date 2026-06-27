#![forbid(unsafe_code)]

mod cli;
mod ops;
mod out;
mod registry;
mod runtime;
mod view;

use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;

use crate::cli::input::runtime_invocation_args_from_target;
use crate::cli::{TargetCliArgs, TargetOutputFormat};
use crate::ops::exec::{
    BasketOperationService, CoreOperationService, FarmOperationService, ListingOperationService,
    MarketOperationService, RuntimeOperationService, TradeOperationService,
    ValidationOperationService,
};
use crate::ops::{
    OperationAdapter, OperationAdapterError, OperationNetworkMode, OperationOutputFormat,
    OperationRequest, OperationRequestPayload, OperationResultPayload, OperationService,
    TargetOperationRequest,
};
use crate::out::envelope::{CliExitCode, OutputEnvelope, OutputError};
use crate::out::terminal::registry::terminal_renderer_registry;
use crate::out::terminal::renderer::{
    TerminalColorPolicy, TerminalRenderContext, TerminalVerbosity, render_terminal_document,
};
use crate::out::terminal::renderers::common::generic_terminal_document;
use crate::registry::{NetworkRequirement, network_requirement, requires_local_signer_mode};
use crate::runtime::config::{
    OutputFormat as RuntimeOutputFormat, RuntimeConfig, SignerBackend, Verbosity,
};
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
    let mut request =
        TargetOperationRequest::from_target_args(&args).map_err(operation_config_error)?;
    let pre_runtime_render_config =
        render_config_from_target_args(&args, args.format.unwrap_or(TargetOutputFormat::Terminal));
    if let Err(error) = validate_pre_runtime_request_contract(&request) {
        let envelope = failure_envelope(&request, error);
        render_envelope(&envelope, &pre_runtime_render_config)?;
        return Ok(envelope_exit_code(&envelope));
    }
    let config = match RuntimeConfig::from_system(&runtime_invocation_args_from_target(&args)) {
        Ok(config) => config,
        Err(error) => {
            let envelope = runtime_config_failure_envelope(&request, error.into());
            render_envelope(&envelope, &pre_runtime_render_config)?;
            return Ok(envelope_exit_code(&envelope));
        }
    };
    request.set_output_format(OperationOutputFormat::from(config.output.format));
    let runtime_render_config = render_config_from_runtime(&config);
    let logging = match initialize_logging(&config.logging) {
        Ok(logging) => logging,
        Err(error) => {
            let envelope = runtime_config_failure_envelope(&request, error.into());
            render_envelope(&envelope, &runtime_render_config)?;
            return Ok(envelope_exit_code(&envelope));
        }
    };
    let envelope = match validate_request_contract(&request, &config) {
        Ok(()) => execute_request(request, &config, &logging),
        Err(error) => failure_envelope(&request, error),
    };
    render_envelope(&envelope, &runtime_render_config)?;
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
        TargetOperationRequest::FarmLocationSet(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmLocationGet(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmLocationClear(request) => {
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
        TargetOperationRequest::TradeSubmit(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeGet(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeList(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeAppList(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeAppExport(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeRebind(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeAccept(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeDecline(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeCancel(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeRevisionPropose(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeRevisionAccept(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeRevisionDecline(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeStatusGet(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeEventList(request) => {
            execute_with(TradeOperationService::new(config), request)
        }
        TargetOperationRequest::TradeEventWatch(request) => {
            execute_with(TradeOperationService::new(config), request)
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

fn runtime_config_failure_envelope(
    request: &TargetOperationRequest,
    error: runtime::RuntimeError,
) -> OutputEnvelope {
    OutputEnvelope::failure(
        request.operation_id(),
        OutputError::new(
            "invalid_input",
            error.to_string(),
            CliExitCode::InvalidInput,
        ),
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

#[derive(Debug, Clone)]
struct EnvelopeRenderConfig {
    format: RenderOutputFormat,
    terminal: TerminalRenderContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderOutputFormat {
    Terminal,
    Json,
    Ndjson,
}

fn render_config_from_target_args(
    args: &TargetCliArgs,
    format: TargetOutputFormat,
) -> EnvelopeRenderConfig {
    EnvelopeRenderConfig {
        format: match format {
            TargetOutputFormat::Terminal => RenderOutputFormat::Terminal,
            TargetOutputFormat::Json => RenderOutputFormat::Json,
            TargetOutputFormat::Ndjson => RenderOutputFormat::Ndjson,
        },
        terminal: TerminalRenderContext {
            verbosity: terminal_verbosity_from_flags(args.quiet, args.verbose, args.trace),
            color: terminal_color_policy(!args.no_color),
            width: 80,
            stdout_is_tty: std::io::stdout().is_terminal(),
            stderr_is_tty: std::io::stderr().is_terminal(),
            dry_run: args.dry_run,
        },
    }
}

fn render_config_from_runtime(config: &RuntimeConfig) -> EnvelopeRenderConfig {
    EnvelopeRenderConfig {
        format: match config.output.format {
            RuntimeOutputFormat::Terminal => RenderOutputFormat::Terminal,
            RuntimeOutputFormat::Json => RenderOutputFormat::Json,
            RuntimeOutputFormat::Ndjson => RenderOutputFormat::Ndjson,
        },
        terminal: TerminalRenderContext {
            verbosity: terminal_verbosity_from_runtime(config.output.verbosity),
            color: terminal_color_policy(config.output.color),
            width: 80,
            stdout_is_tty: config.interaction.stdout_tty,
            stderr_is_tty: std::io::stderr().is_terminal(),
            dry_run: config.output.dry_run,
        },
    }
}

fn terminal_verbosity_from_flags(quiet: bool, verbose: bool, trace: bool) -> TerminalVerbosity {
    if trace {
        TerminalVerbosity::Trace
    } else if verbose {
        TerminalVerbosity::Verbose
    } else if quiet {
        TerminalVerbosity::Quiet
    } else {
        TerminalVerbosity::Normal
    }
}

fn terminal_verbosity_from_runtime(verbosity: Verbosity) -> TerminalVerbosity {
    match verbosity {
        Verbosity::Quiet => TerminalVerbosity::Quiet,
        Verbosity::Normal => TerminalVerbosity::Normal,
        Verbosity::Verbose => TerminalVerbosity::Verbose,
        Verbosity::Trace => TerminalVerbosity::Trace,
    }
}

fn terminal_color_policy(color: bool) -> TerminalColorPolicy {
    if color {
        TerminalColorPolicy::Auto
    } else {
        TerminalColorPolicy::Never
    }
}

fn render_envelope(
    envelope: &OutputEnvelope,
    config: &EnvelopeRenderConfig,
) -> Result<(), runtime::RuntimeError> {
    match config.format {
        RenderOutputFormat::Terminal => render_terminal_envelope(envelope, &config.terminal),
        RenderOutputFormat::Json => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            serde_json::to_writer_pretty(&mut handle, envelope)?;
            writeln!(handle)?;
            Ok(())
        }
        RenderOutputFormat::Ndjson => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            for frame in envelope.to_ndjson_frames() {
                serde_json::to_writer(&mut handle, &frame)?;
                writeln!(handle)?;
            }
            Ok(())
        }
    }
}

fn render_terminal_envelope(
    envelope: &OutputEnvelope,
    cx: &TerminalRenderContext,
) -> Result<(), runtime::RuntimeError> {
    let registry = terminal_renderer_registry();
    let document = registry
        .get(envelope.operation_id.as_str())
        .map(|renderer| renderer.render(envelope, cx))
        .unwrap_or_else(|| generic_terminal_document(envelope));
    let rendered = render_terminal_document(&document, cx);
    if envelope.errors.is_empty() {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        writeln!(handle, "{rendered}")?;
    } else {
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        writeln!(handle, "{rendered}")?;
    }
    Ok(())
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
