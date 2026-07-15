use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::global::{
    RecordLookupArgs, TradeCancelArgs, TradeDecisionArg, TradeDecisionArgs, TradeRequestArgs,
};
use crate::ops::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, TradeAcceptRequest, TradeAcceptResult,
    TradeCancelRequest, TradeCancelResult, TradeDeclineRequest, TradeDeclineResult,
    TradeGetRequest, TradeGetResult, TradeListRequest, TradeListResult, TradeRequestRequest,
    TradeRequestResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::view::runtime::{
    CommandDisposition, OrderCancellationView, OrderDecisionView, OrderSubmitView,
};

pub struct TradeOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> TradeOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<TradeRequestRequest> for TradeOperationService<'_> {
    type Result = TradeRequestResult;

    fn execute(
        &self,
        request: OperationRequest<TradeRequestRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let key = required_trade_key(&request)?;
        let args = TradeRequestArgs {
            key,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            confirm_public_note: bool_input(&request, "confirm_public_note").unwrap_or(false),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::submit(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        submit_result::<TradeRequestResult>(request.operation_id(), &view)
    }
}

impl OperationService<TradeGetRequest> for TradeOperationService<'_> {
    type Result = TradeGetResult;

    fn execute(
        &self,
        request: OperationRequest<TradeGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = RecordLookupArgs {
            key: required_trade_key(&request)?,
        };
        let view = map_runtime(crate::runtime::order::get(self.config, &args))?;
        serialized_target_result::<TradeGetResult, _>(&view)
    }
}

impl OperationService<TradeListRequest> for TradeOperationService<'_> {
    type Result = TradeListResult;

    fn execute(
        &self,
        _request: OperationRequest<TradeListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::order::list(self.config))?;
        serialized_target_result::<TradeListResult, _>(&view)
    }
}

impl OperationService<TradeAcceptRequest> for TradeOperationService<'_> {
    type Result = TradeAcceptResult;

    fn execute(
        &self,
        request: OperationRequest<TradeAcceptRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradeDecisionArgs {
            key: required_trade_key(&request)?,
            decision: TradeDecisionArg::Accept,
            reason: None,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            confirm_public_note: false,
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::decide(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        decision_result::<TradeAcceptResult>(request.operation_id(), &view)
    }
}

impl OperationService<TradeDeclineRequest> for TradeOperationService<'_> {
    type Result = TradeDeclineResult;

    fn execute(
        &self,
        request: OperationRequest<TradeDeclineRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let reason = string_input(&request, "reason")
            .map(|reason| reason.trim().to_owned())
            .filter(|reason| !reason.is_empty())
            .ok_or_else(|| {
                invalid_input(
                    request.operation_id(),
                    "missing required `reason` input".to_owned(),
                )
            })?;

        let args = TradeDecisionArgs {
            key: required_trade_key(&request)?,
            decision: TradeDecisionArg::Decline,
            reason: Some(reason),
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            confirm_public_note: bool_input(&request, "confirm_public_note").unwrap_or(false),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::decide(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        decision_result::<TradeDeclineResult>(request.operation_id(), &view)
    }
}

impl OperationService<TradeCancelRequest> for TradeOperationService<'_> {
    type Result = TradeCancelResult;

    fn execute(
        &self,
        request: OperationRequest<TradeCancelRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let reason = string_input(&request, "reason")
            .map(|reason| reason.trim().to_owned())
            .filter(|reason| !reason.is_empty())
            .ok_or_else(|| {
                invalid_input(
                    request.operation_id(),
                    "missing required `reason` input".to_owned(),
                )
            })?;

        let args = TradeCancelArgs {
            key: required_trade_key(&request)?,
            reason,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            confirm_public_note: bool_input(&request, "confirm_public_note").unwrap_or(false),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::cancel(&config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        cancellation_result::<TradeCancelResult>(request.operation_id(), &view)
    }
}

fn decision_result<R>(
    operation_id: &str,
    view: &OrderDecisionView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_target_result::<R, _>(view),
        CommandDisposition::ValidationFailed => {
            let message = view.reason.clone().unwrap_or_else(|| {
                format!(
                    "order decision failed validation with state `{}`",
                    view.state
                )
            });
            Err(OperationAdapterError::validation_failed_with_detail(
                operation_id,
                message,
                order_decision_error_detail(view),
            ))
        }
        disposition => {
            let message = view
                .reason
                .clone()
                .unwrap_or_else(|| format!("order decision finished with state `{}`", view.state));
            if disposition == CommandDisposition::ExternalUnavailable {
                let detail = order_decision_error_detail(view);
                if !view.failed_transport_targets.is_empty()
                    && view.attempted_transport_endpoints.is_empty()
                {
                    Err(OperationAdapterError::network_unavailable_with_detail(
                        operation_id,
                        message,
                        detail,
                    ))
                } else {
                    Err(OperationAdapterError::operation_unavailable_with_detail(
                        operation_id,
                        message,
                        detail,
                    ))
                }
            } else if disposition == CommandDisposition::Unconfigured {
                Err(OperationAdapterError::operation_unavailable_with_detail(
                    operation_id,
                    message,
                    order_decision_error_detail(view),
                ))
            } else if disposition == CommandDisposition::NotFound {
                Err(OperationAdapterError::not_found_with_detail(
                    operation_id,
                    message,
                    order_decision_error_detail(view),
                ))
            } else {
                Err(OperationAdapterError::from_command_disposition(
                    operation_id,
                    disposition,
                    message,
                ))
            }
        }
    }
}

fn order_decision_error_detail(view: &OrderDecisionView) -> Value {
    json!({
        "state": &view.state,
        "trade_id": &view.order_id,
        "locator": &view.locator,
        "listing_addr": &view.listing_addr,
        "listing_event_id": &view.listing_event_id,
        "request_event_id": &view.request_event_id,
        "root_event_id": &view.root_event_id,
        "prev_event_id": &view.prev_event_id,
        "event_id": &view.event_id,
        "event_kind": view.event_kind,
        "inventory": &view.inventory,
        "buyer_pubkey": &view.buyer_pubkey,
        "seller_pubkey": &view.seller_pubkey,
        "decision": &view.decision,
        "dry_run": view.dry_run,
        "target_transport_endpoints": &view.target_transport_endpoints,
        "attempted_transport_endpoints": &view.attempted_transport_endpoints,
        "accepted_transport_endpoints": &view.accepted_transport_endpoints,
        "failed_transport_targets": &view.failed_transport_targets,
        "fetched_count": view.fetched_count,
        "decoded_count": view.decoded_count,
        "skipped_count": view.skipped_count,
        "idempotency_key": &view.idempotency_key,
        "signer_mode": &view.signer_mode,
        "issues": &view.issues,
        "actions": &view.actions,
    })
}

fn cancellation_result<R>(
    operation_id: &str,
    view: &OrderCancellationView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_target_result::<R, _>(view),
        CommandDisposition::ValidationFailed => {
            let message = view.reason.clone().unwrap_or_else(|| {
                format!("order cancel failed validation with state `{}`", view.state)
            });
            Err(OperationAdapterError::validation_failed_with_detail(
                operation_id,
                message,
                order_cancellation_error_detail(view),
            ))
        }
        disposition => {
            let message = view
                .reason
                .clone()
                .unwrap_or_else(|| format!("order cancel finished with state `{}`", view.state));
            if disposition == CommandDisposition::ExternalUnavailable {
                let detail = order_cancellation_error_detail(view);
                if !view.failed_transport_targets.is_empty()
                    && view.attempted_transport_endpoints.is_empty()
                {
                    Err(OperationAdapterError::network_unavailable_with_detail(
                        operation_id,
                        message,
                        detail,
                    ))
                } else {
                    Err(OperationAdapterError::operation_unavailable_with_detail(
                        operation_id,
                        message,
                        detail,
                    ))
                }
            } else if disposition == CommandDisposition::Unconfigured {
                Err(OperationAdapterError::operation_unavailable_with_detail(
                    operation_id,
                    message,
                    order_cancellation_error_detail(view),
                ))
            } else {
                Err(OperationAdapterError::from_command_disposition(
                    operation_id,
                    disposition,
                    message,
                ))
            }
        }
    }
}

fn order_cancellation_error_detail(view: &OrderCancellationView) -> Value {
    json!({
        "state": &view.state,
        "trade_id": &view.order_id,
        "locator": &view.locator,
        "listing_addr": &view.listing_addr,
        "request_event_id": &view.request_event_id,
        "decision_event_id": &view.decision_event_id,
        "root_event_id": &view.root_event_id,
        "prev_event_id": &view.prev_event_id,
        "event_id": &view.event_id,
        "event_kind": view.event_kind,
        "buyer_pubkey": &view.buyer_pubkey,
        "seller_pubkey": &view.seller_pubkey,
        "cancellation_reason": &view.cancellation_reason,
        "dry_run": view.dry_run,
        "target_transport_endpoints": &view.target_transport_endpoints,
        "attempted_transport_endpoints": &view.attempted_transport_endpoints,
        "accepted_transport_endpoints": &view.accepted_transport_endpoints,
        "failed_transport_targets": &view.failed_transport_targets,
        "fetched_count": view.fetched_count,
        "decoded_count": view.decoded_count,
        "skipped_count": view.skipped_count,
        "idempotency_key": &view.idempotency_key,
        "signer_mode": &view.signer_mode,
        "issues": &view.issues,
        "actions": &view.actions,
    })
}

fn serialized_target_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn submit_result<R>(
    operation_id: &str,
    view: &OrderSubmitView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_target_result::<R, _>(view),
        disposition => {
            let message = view
                .reason
                .clone()
                .unwrap_or_else(|| format!("order submit finished with state `{}`", view.state));
            let detail = order_submit_error_detail(view);
            match disposition {
                CommandDisposition::NotFound => Err(OperationAdapterError::not_found_with_detail(
                    operation_id,
                    message,
                    detail,
                )),
                CommandDisposition::ValidationFailed => {
                    Err(OperationAdapterError::validation_failed_with_detail(
                        operation_id,
                        message,
                        detail,
                    ))
                }
                CommandDisposition::Unconfigured => {
                    Err(OperationAdapterError::operation_unavailable_with_detail(
                        operation_id,
                        message,
                        detail,
                    ))
                }
                CommandDisposition::ExternalUnavailable => {
                    if !view.failed_transport_targets.is_empty()
                        && view.attempted_transport_endpoints.is_empty()
                    {
                        Err(OperationAdapterError::network_unavailable_with_detail(
                            operation_id,
                            message,
                            detail,
                        ))
                    } else {
                        Err(OperationAdapterError::operation_unavailable_with_detail(
                            operation_id,
                            message,
                            detail,
                        ))
                    }
                }
                _ => Err(OperationAdapterError::from_command_disposition(
                    operation_id,
                    disposition,
                    message,
                )),
            }
        }
    }
}

fn order_submit_error_detail(view: &OrderSubmitView) -> Value {
    json!({
        "state": &view.state,
        "source": &view.source,
        "trade_id": &view.order_id,
        "locator": &view.locator,
        "file": &view.file,
        "listing_lookup": &view.listing_lookup,
        "listing_addr": &view.listing_addr,
        "listing_event_id": &view.listing_event_id,
        "listing_relays": &view.listing_relays,
        "buyer_account_id": &view.buyer_account_id,
        "buyer_pubkey": &view.buyer_pubkey,
        "seller_pubkey": &view.seller_pubkey,
        "event_id": &view.event_id,
        "event_kind": view.event_kind,
        "dry_run": view.dry_run,
        "deduplicated": view.deduplicated,
        "target_transport_endpoints": &view.target_transport_endpoints,
        "attempted_transport_endpoints": &view.attempted_transport_endpoints,
        "accepted_transport_endpoints": &view.accepted_transport_endpoints,
        "failed_transport_targets": &view.failed_transport_targets,
        "idempotency_key": &view.idempotency_key,
        "signer_mode": &view.signer_mode,
        "issues": &view.issues,
        "actions": &view.actions,
    })
}

fn required_trade_key<P>(request: &OperationRequest<P>) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, "trade_id").ok_or_else(|| {
        invalid_input(
            request.operation_id(),
            "missing required `trade_id` input".to_owned(),
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

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
}

fn invalid_input(operation_id: &str, message: String) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message,
    }
}
