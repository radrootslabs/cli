#![forbid(unsafe_code)]

mod cli;
mod domain;
mod operation_adapter;
mod operation_basket;
mod operation_core;
mod operation_farm;
mod operation_listing;
mod operation_market;
mod operation_order;
mod operation_registry;
mod operation_runtime;
mod output_contract;
mod runtime;
mod target_cli;

use std::io::Write;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{CliArgs, Command, ConfigArgs, ConfigCommand, OutputFormatArg};
use crate::operation_adapter::{
    OperationAdapter, OperationAdapterError, OperationNetworkMode, OperationOutputFormat,
    OperationRequest, OperationRequestPayload, OperationResultPayload, OperationService,
    TargetOperationRequest,
};
use crate::operation_basket::BasketOperationService;
use crate::operation_core::CoreOperationService;
use crate::operation_farm::FarmOperationService;
use crate::operation_listing::ListingOperationService;
use crate::operation_market::MarketOperationService;
use crate::operation_order::OrderOperationService;
use crate::operation_runtime::RuntimeOperationService;
use crate::output_contract::OutputEnvelope;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::initialize_logging;
use crate::target_cli::{TargetCliArgs, TargetOutputFormat};

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
    debug_assert!(operation_registry::registry_linkage_is_valid());
    debug_assert!(operation_adapter::adapter_registry_linkage_is_valid());
    let args = TargetCliArgs::parse();
    let config = RuntimeConfig::from_system(&config_args_from_target(&args)?)?;
    let logging = initialize_logging(&config.logging)?;
    let request =
        TargetOperationRequest::from_target_args(&args).map_err(operation_config_error)?;
    let envelope = match validate_request_contract(&request, &config) {
        Ok(()) => execute_request(request, &config, &logging),
        Err(error) => failure_envelope(&request, error),
    };
    render_envelope(&envelope, args.format)?;
    Ok(envelope_exit_code(&envelope))
}

fn config_args_from_target(args: &TargetCliArgs) -> Result<CliArgs, runtime::RuntimeError> {
    Ok(CliArgs {
        output_format: Some(match args.format {
            TargetOutputFormat::Human => OutputFormatArg::Human,
            TargetOutputFormat::Json => OutputFormatArg::Json,
            TargetOutputFormat::Ndjson => OutputFormatArg::Ndjson,
        }),
        json: false,
        ndjson: false,
        env_file: None,
        quiet: args.quiet,
        verbose: args.verbose,
        trace: args.trace,
        dry_run: args.dry_run,
        no_color: args.no_color,
        no_input: args.no_input,
        yes: false,
        log_filter: None,
        log_dir: None,
        log_stdout: false,
        no_log_stdout: false,
        account: args.account_id.clone(),
        identity_path: None,
        signer: None,
        relay: args.relay.clone(),
        myc_executable: None,
        myc_status_timeout_ms: None,
        hyf_enabled: false,
        no_hyf_enabled: false,
        hyf_executable: None,
        command: Command::Config(ConfigArgs {
            command: ConfigCommand::Show,
        }),
    })
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
        TargetOperationRequest::RuntimeStatusGet(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::RuntimeStart(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::RuntimeStop(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::RuntimeRestart(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::RuntimeLogWatch(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::RuntimeConfigGet(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::JobGet(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::JobList(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::JobWatch(request) => {
            execute_with(RuntimeOperationService::new(config), request)
        }
        TargetOperationRequest::FarmCreate(request) => {
            execute_with(FarmOperationService::new(config), request)
        }
        TargetOperationRequest::FarmGet(request) => {
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
        TargetOperationRequest::ListingUpdate(request) => {
            execute_with(ListingOperationService::new(config), request)
        }
        TargetOperationRequest::ListingValidate(request) => {
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
        TargetOperationRequest::OrderEventList(request) => {
            execute_with(OrderOperationService::new(config), request)
        }
        TargetOperationRequest::OrderEventWatch(request) => {
            execute_with(OrderOperationService::new(config), request)
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
        .envelope_context(format!("req_{}", operation_id.replace('.', "_")));
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
    validate_network_contract(request, config)?;
    Ok(())
}

fn validate_network_contract(
    request: &TargetOperationRequest,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError> {
    let spec = request.spec();
    let external = external_network_operation(spec.operation_id);
    match request.context().network_mode {
        OperationNetworkMode::Default => Ok(()),
        OperationNetworkMode::Offline => {
            if external && !request.context().dry_run {
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
            if external && !request.context().dry_run && config.relay.urls.is_empty() {
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

fn external_network_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "sync.pull"
            | "sync.push"
            | "sync.watch"
            | "market.refresh"
            | "farm.publish"
            | "listing.publish"
            | "listing.archive"
            | "order.submit"
            | "order.event.watch"
            | "job.watch"
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
            .envelope_context(format!("req_{}", request.operation_id().replace('.', "_"))),
    )
}

fn render_envelope(
    envelope: &OutputEnvelope,
    format: TargetOutputFormat,
) -> Result<(), runtime::RuntimeError> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    match format {
        TargetOutputFormat::Human | TargetOutputFormat::Json => {
            serde_json::to_writer_pretty(&mut handle, envelope)?;
        }
        TargetOutputFormat::Ndjson => {
            serde_json::to_writer(&mut handle, envelope)?;
        }
    }
    writeln!(handle)?;
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
