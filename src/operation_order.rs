use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::runtime::{
    CommandDisposition, OrderDecisionView, OrderStatusView, OrderSubmitView,
};
use crate::operation_adapter::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, OrderAcceptRequest, OrderAcceptResult,
    OrderDeclineRequest, OrderDeclineResult, OrderEventListRequest, OrderEventListResult,
    OrderEventWatchRequest, OrderEventWatchResult, OrderGetRequest, OrderGetResult,
    OrderListRequest, OrderListResult, OrderStatusGetRequest, OrderStatusGetResult,
    OrderSubmitRequest, OrderSubmitResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime_args::{
    OrderDecisionArg, OrderDecisionArgs, OrderStatusArgs, OrderSubmitArgs, OrderWatchArgs,
    RecordLookupArgs,
};

pub struct OrderOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> OrderOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<OrderSubmitRequest> for OrderOperationService<'_> {
    type Result = OrderSubmitResult;

    fn execute(
        &self,
        request: OperationRequest<OrderSubmitRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let key = required_order_key(&request)?;
        let args = OrderSubmitArgs {
            key,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::submit(&config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        if request.context.dry_run && view.state == "unconfigured" && !view.issues.is_empty() {
            serialized_target_result::<OrderSubmitResult, _>(&view)
        } else {
            submit_result::<OrderSubmitResult>(request.operation_id(), &view)
        }
    }
}

impl OperationService<OrderGetRequest> for OrderOperationService<'_> {
    type Result = OrderGetResult;

    fn execute(
        &self,
        request: OperationRequest<OrderGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = RecordLookupArgs {
            key: required_order_key(&request)?,
        };
        let view = map_runtime(crate::runtime::order::get(self.config, &args))?;
        serialized_target_result::<OrderGetResult, _>(&view)
    }
}

impl OperationService<OrderListRequest> for OrderOperationService<'_> {
    type Result = OrderListResult;

    fn execute(
        &self,
        _request: OperationRequest<OrderListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::order::list(self.config))?;
        serialized_target_result::<OrderListResult, _>(&view)
    }
}

impl OperationService<OrderAcceptRequest> for OrderOperationService<'_> {
    type Result = OrderAcceptResult;

    fn execute(
        &self,
        request: OperationRequest<OrderAcceptRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let args = OrderDecisionArgs {
            key: required_order_key(&request)?,
            decision: OrderDecisionArg::Accept,
            reason: None,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::decide(&config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        decision_result::<OrderAcceptResult>(request.operation_id(), &view)
    }
}

impl OperationService<OrderDeclineRequest> for OrderOperationService<'_> {
    type Result = OrderDeclineResult;

    fn execute(
        &self,
        request: OperationRequest<OrderDeclineRequest>,
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
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let args = OrderDecisionArgs {
            key: required_order_key(&request)?,
            decision: OrderDecisionArg::Decline,
            reason: Some(reason),
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = crate::runtime::order::decide(&config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        decision_result::<OrderDeclineResult>(request.operation_id(), &view)
    }
}

impl OperationService<OrderStatusGetRequest> for OrderOperationService<'_> {
    type Result = OrderStatusGetResult;

    fn execute(
        &self,
        request: OperationRequest<OrderStatusGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = OrderStatusArgs {
            key: required_order_key(&request)?,
        };
        let view = crate::runtime::order::status(self.config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        status_result::<OrderStatusGetResult>(request.operation_id(), &view)
    }
}

impl OperationService<OrderEventListRequest> for OrderOperationService<'_> {
    type Result = OrderEventListResult;

    fn execute(
        &self,
        request: OperationRequest<OrderEventListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let order_id = string_input(&request, "order_id");
        let view =
            crate::runtime::order::history(self.config, order_id.as_deref()).map_err(|error| {
                OperationAdapterError::runtime_failure(request.operation_id(), error)
            })?;
        history_result::<OrderEventListResult>(request.operation_id(), &view)
    }
}

impl OperationService<OrderEventWatchRequest> for OrderOperationService<'_> {
    type Result = OrderEventWatchResult;

    fn execute(
        &self,
        request: OperationRequest<OrderEventWatchRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = OrderWatchArgs {
            key: required_order_key(&request)?,
            frames: usize_input(&request, "frames").or(Some(1)),
            interval_ms: u64_input(&request, "interval_ms").unwrap_or(1_000),
        };
        let view = map_runtime(crate::runtime::order::watch(self.config, &args))?;
        serialized_target_result::<OrderEventWatchResult, _>(&view)
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
                if !view.failed_relays.is_empty() && view.connected_relays.is_empty() {
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
        "order_id": &view.order_id,
        "listing_addr": &view.listing_addr,
        "listing_event_id": &view.listing_event_id,
        "request_event_id": &view.request_event_id,
        "root_event_id": &view.root_event_id,
        "prev_event_id": &view.prev_event_id,
        "event_id": &view.event_id,
        "event_kind": view.event_kind,
        "buyer_pubkey": &view.buyer_pubkey,
        "seller_pubkey": &view.seller_pubkey,
        "decision": &view.decision,
        "dry_run": view.dry_run,
        "target_relays": &view.target_relays,
        "connected_relays": &view.connected_relays,
        "acknowledged_relays": &view.acknowledged_relays,
        "failed_relays": &view.failed_relays,
        "fetched_count": view.fetched_count,
        "decoded_count": view.decoded_count,
        "skipped_count": view.skipped_count,
        "idempotency_key": &view.idempotency_key,
        "signer_mode": &view.signer_mode,
        "issues": &view.issues,
        "actions": &view.actions,
    })
}

fn status_result<R>(
    operation_id: &str,
    view: &OrderStatusView,
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
                .unwrap_or_else(|| format!("order status finished with state `{}`", view.state));
            Err(OperationAdapterError::from_command_disposition(
                operation_id,
                disposition,
                message,
            ))
        }
    }
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
            if disposition == CommandDisposition::Unconfigured && !view.issues.is_empty() {
                Err(OperationAdapterError::operation_unavailable_with_detail(
                    operation_id,
                    message,
                    json!({
                        "state": &view.state,
                        "order_id": &view.order_id,
                        "file": &view.file,
                        "listing_addr": &view.listing_addr,
                        "listing_event_id": &view.listing_event_id,
                        "issues": &view.issues,
                        "actions": &view.actions,
                    }),
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

fn history_result<R>(
    operation_id: &str,
    view: &crate::domain::runtime::OrderHistoryView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_target_result::<R, _>(view),
        disposition => {
            let message = view.reason.clone().unwrap_or_else(|| {
                format!("order event list finished with state `{}`", view.state)
            });
            if disposition == CommandDisposition::ExternalUnavailable {
                Err(OperationAdapterError::network_unavailable_with_detail(
                    operation_id,
                    message,
                    json!({
                        "state": &view.state,
                        "seller_pubkey": &view.seller_pubkey,
                        "target_relays": &view.target_relays,
                        "connected_relays": &view.connected_relays,
                        "failed_relays": &view.failed_relays,
                        "fetched_count": view.fetched_count,
                        "decoded_count": view.decoded_count,
                        "skipped_count": view.skipped_count,
                    }),
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

fn required_order_key<P>(request: &OperationRequest<P>) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, "order_id")
        .or_else(|| string_input(request, "key"))
        .ok_or_else(|| {
            invalid_input(
                request.operation_id(),
                "missing required `order_id` input".to_owned(),
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

fn usize_input<P>(request: &OperationRequest<P>, key: &str) -> Option<usize>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn u64_input<P>(request: &OperationRequest<P>, key: &str) -> Option<u64>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_u64)
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value};
    use tempfile::tempdir;

    use super::{OrderOperationService, decision_result};
    use crate::domain::runtime::OrderDecisionView;
    use crate::operation_adapter::{
        OperationAdapter, OperationContext, OperationData, OperationRequest, OrderAcceptRequest,
        OrderAcceptResult, OrderDeclineRequest, OrderDeclineResult, OrderEventListRequest,
        OrderEventWatchRequest, OrderGetRequest, OrderListRequest, OrderStatusGetRequest,
        OrderSubmitRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn order_service_get_and_list_preserve_order_truth() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let get = OperationRequest::new(
            OperationContext::default(),
            OrderGetRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order get request");
        let get_envelope = service
            .execute(get)
            .expect("order get result")
            .to_envelope(OperationContext::default().envelope_context("req_order_get"))
            .expect("order get envelope");

        assert_eq!(get_envelope.operation_id, "order.get");
        assert_eq!(get_envelope.result["state"], "missing");
        assert_eq!(get_envelope.result["actions"][0], "radroots order list");
        assert_eq!(get_envelope.result["actions"][1], "radroots basket create");

        let list = OperationRequest::new(OperationContext::default(), OrderListRequest::default())
            .expect("order list request");
        let list_envelope = service
            .execute(list)
            .expect("order list result")
            .to_envelope(OperationContext::default().envelope_context("req_order_list"))
            .expect("order list envelope");
        assert_eq!(list_envelope.operation_id, "order.list");
        assert_eq!(list_envelope.result["state"], "empty");
        assert_eq!(list_envelope.result["actions"][0], "radroots basket create");
    }

    #[test]
    fn order_submit_requires_approval_token() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let submit = OperationRequest::new(
            OperationContext::default(),
            OrderSubmitRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order submit request");
        let error = service.execute(submit).expect_err("approval required");

        assert!(format!("{error}").contains("approval_token"));
        assert_eq!(error.to_output_error().code, "approval_required");
        assert_eq!(error.to_output_error().exit_code, 6);
    }

    #[test]
    fn order_submit_with_approval_returns_not_found_for_missing_order() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let mut context = OperationContext::default();
        context.approval_token = Some("approve_test".to_owned());
        let submit = OperationRequest::new(
            context.clone(),
            OrderSubmitRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order submit request");
        let error = service.execute(submit).expect_err("missing order error");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "not_found");
        assert_eq!(output_error.exit_code, 4);
        assert!(output_error.message.contains("ord_missing"));
    }

    #[test]
    fn order_accept_requires_approval_token() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let accept = OperationRequest::new(
            OperationContext::default(),
            OrderAcceptRequest::from_data(data(&[("order_id", "ord_pending")])),
        )
        .expect("order accept request");
        let error = service.execute(accept).expect_err("approval required");

        assert_eq!(error.to_output_error().code, "approval_required");
    }

    #[test]
    fn order_decision_already_decided_maps_to_validation_failure() {
        let view = already_decided_view();
        let error = match decision_result::<OrderAcceptResult>("order.accept", &view) {
            Ok(_) => panic!("already decided view should fail validation"),
            Err(error) => error,
        };
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "validation_failed");
        assert_eq!(output_error.exit_code, 10);
        let detail = output_error.detail.expect("validation detail");
        assert_eq!(detail["state"], "already_decided");
        assert_eq!(detail["operation_id"], "order.accept");
        assert_eq!(detail["listing_event_id"], "l".repeat(64));
        assert_eq!(detail["event_id"], "d".repeat(64));
        assert_eq!(detail["event_kind"], 3423);
        assert_eq!(detail["idempotency_key"], "idem_test");
        assert_eq!(detail["signer_mode"], "local");
        assert_eq!(detail["actions"][0], "radroots order status get ord_test");
    }

    #[test]
    fn order_decision_invalid_maps_to_validation_failure() {
        let view = invalid_decision_view();
        let error = match decision_result::<OrderDeclineResult>("order.decline", &view) {
            Ok(_) => panic!("invalid view should fail validation"),
            Err(error) => error,
        };
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "validation_failed");
        assert_eq!(output_error.exit_code, 10);
        assert_eq!(
            output_error.message,
            "active order events for `ord_test` failed reducer validation"
        );
        let detail = output_error.detail.expect("validation detail");
        assert_eq!(detail["state"], "invalid");
        assert_eq!(detail["operation_id"], "order.decline");
        assert_eq!(detail["listing_event_id"], "l".repeat(64));
        assert_eq!(detail["event_id"], Value::Null);
        assert_eq!(detail["event_kind"], Value::Null);
        assert_eq!(detail["idempotency_key"], "idem_test");
        assert_eq!(detail["signer_mode"], "local");
    }

    #[test]
    fn order_decline_requires_reason_before_approval() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let decline = OperationRequest::new(
            OperationContext::default(),
            OrderDeclineRequest::from_data(data(&[("order_id", "ord_pending")])),
        )
        .expect("order decline request");
        let error = service.execute(decline).expect_err("reason required");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "invalid_input");
        assert!(output_error.message.contains("reason"));
    }

    #[test]
    fn order_decline_rejects_blank_reason_before_approval() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let decline = OperationRequest::new(
            OperationContext::default(),
            OrderDeclineRequest::from_data(data(&[("order_id", "ord_pending"), ("reason", " ")])),
        )
        .expect("order decline request");
        let error = service.execute(decline).expect_err("reason required");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "invalid_input");
        assert!(output_error.message.contains("reason"));
    }

    #[test]
    fn order_status_get_requires_relay_configuration() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let status = OperationRequest::new(
            OperationContext::default(),
            OrderStatusGetRequest::from_data(data(&[("order_id", "ord_pending")])),
        )
        .expect("order status request");
        let error = service.execute(status).expect_err("status unconfigured");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "operation_unavailable");
        assert!(output_error.message.contains("configured relay"));
    }

    #[test]
    fn order_event_list_requires_relay_configuration() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            OrderEventListRequest::default(),
        )
        .expect("order event list request");
        let envelope = service
            .execute(request)
            .expect_err("order event list unconfigured")
            .to_output_error();

        assert_eq!(envelope.code, "operation_unavailable");
        assert_eq!(envelope.exit_code, 3);
        assert!(envelope.message.contains("configured relay"));
    }

    #[test]
    fn order_event_watch_reports_missing_order_with_target_actions() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            OrderEventWatchRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order event watch request");
        let envelope = service
            .execute(request)
            .expect("order event watch result")
            .to_envelope(OperationContext::default().envelope_context("req_order_watch"))
            .expect("order event watch envelope");

        assert_eq!(envelope.operation_id, "order.event.watch");
        assert_eq!(envelope.result["state"], "missing");
        assert_eq!(envelope.result["actions"][0], "radroots order list");
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

    fn already_decided_view() -> OrderDecisionView {
        OrderDecisionView {
            state: "already_decided".to_owned(),
            source: "test".to_owned(),
            order_id: "ord_test".to_owned(),
            listing_addr: Some("30402:seller:listing".to_owned()),
            buyer_pubkey: Some("b".repeat(64)),
            seller_pubkey: Some("s".repeat(64)),
            decision: "accepted".to_owned(),
            request_event_id: Some("r".repeat(64)),
            listing_event_id: Some("l".repeat(64)),
            root_event_id: Some("r".repeat(64)),
            prev_event_id: Some("r".repeat(64)),
            event_id: Some("d".repeat(64)),
            event_kind: Some(3423),
            dry_run: false,
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            fetched_count: 2,
            decoded_count: 2,
            skipped_count: 0,
            idempotency_key: Some("idem_test".to_owned()),
            signer_mode: Some("local".to_owned()),
            reason: Some(
                "order accept refused because order `ord_test` already has a visible `accepted` seller decision"
                    .to_owned(),
            ),
            issues: Vec::new(),
            actions: vec!["radroots order status get ord_test".to_owned()],
        }
    }

    fn invalid_decision_view() -> OrderDecisionView {
        let mut view = already_decided_view();
        view.state = "invalid".to_owned();
        view.decision = "declined".to_owned();
        view.event_id = None;
        view.event_kind = None;
        view.reason =
            Some("active order events for `ord_test` failed reducer validation".to_owned());
        view
    }
}
