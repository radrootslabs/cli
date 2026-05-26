#![allow(dead_code)]

mod context;
mod contract;
mod error;
mod service;

pub use context::*;
pub use contract::*;
pub use error::OperationAdapterError;
pub use service::*;

#[cfg(test)]
mod tests {
    use std::io;

    use clap::Parser;
    use serde_json::{Value, json};

    use super::{
        OperationAdapter, OperationAdapterError, OperationContext, OperationInputMode,
        OperationNetworkMode, OperationOutputFormat, OperationRequest, OperationResult,
        OperationService, TargetOperationRequest, WorkspaceGetRequest, WorkspaceGetResult,
        adapter_registry_linkage_is_valid,
    };
    use crate::cli::TargetCliArgs;
    use crate::registry::OPERATION_REGISTRY;
    use crate::runtime::RuntimeError;
    use crate::runtime::accounts::AccountRuntimeFailure;

    #[test]
    fn adapter_binds_every_registry_entry() {
        assert!(adapter_registry_linkage_is_valid());

        for operation in OPERATION_REGISTRY {
            let parsed = TargetCliArgs::try_parse_from(operation.cli_path.split_whitespace())
                .unwrap_or_else(|error| {
                    panic!("{} failed to parse: {error}", operation.cli_path);
                });
            let request = TargetOperationRequest::from_target_args(&parsed)
                .expect("operation request from target args");

            assert_eq!(request.operation_id(), operation.operation_id);
            assert_eq!(request.spec().mcp_tool, operation.mcp_tool);
            assert_eq!(request.request_type_name(), operation.rust_request);
            assert_eq!(
                TargetOperationRequest::request_type_for_operation(operation.operation_id),
                Some(operation.rust_request)
            );
        }
    }

    #[test]
    fn adapter_context_carries_target_global_scope() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "--format",
            "json",
            "--account-id",
            "acct_test",
            "--relay",
            "wss://relay.one",
            "--online",
            "--dry-run",
            "--idempotency-key",
            "idem_test",
            "--correlation-id",
            "corr_test",
            "--approval-token",
            "approval_test",
            "--no-input",
            "--quiet",
            "--verbose",
            "--trace",
            "--no-color",
            "workspace",
            "get",
        ])
        .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let context = request.context();

        assert_eq!(context.output_format, OperationOutputFormat::Json);
        assert_eq!(context.account_id.as_deref(), Some("acct_test"));
        assert_eq!(context.relays, vec!["wss://relay.one".to_owned()]);
        assert_eq!(context.network_mode, OperationNetworkMode::Online);
        assert!(context.dry_run);
        assert_eq!(context.idempotency_key.as_deref(), Some("idem_test"));
        assert_eq!(context.correlation_id.as_deref(), Some("corr_test"));
        assert_eq!(context.approval_token.as_deref(), Some("approval_test"));
        assert_eq!(context.input_mode, OperationInputMode::NoInput);
        assert!(context.quiet);
        assert!(context.verbose);
        assert!(context.trace);
        assert!(!context.color);

        let envelope_context = context.envelope_context("req_test");
        let actor = envelope_context.actor.expect("account actor");
        assert_eq!(actor.account_id, "acct_test");
        assert_eq!(actor.role, "account");
    }

    #[test]
    fn adapter_maps_account_attach_secret_input() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "account",
            "attach-secret",
            "acct_test",
            "identity.json",
            "--default",
        ])
        .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let TargetOperationRequest::AccountAttachSecret(request) = request else {
            panic!("expected account attach-secret request")
        };

        assert_eq!(request.operation_id(), "account.attach_secret");
        assert_eq!(
            request
                .payload
                .input
                .get("selector")
                .and_then(Value::as_str),
            Some("acct_test")
        );
        assert_eq!(
            request.payload.input.get("path").and_then(Value::as_str),
            Some("identity.json")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("default")
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn adapter_maps_farm_rebind_selector() {
        let parsed = TargetCliArgs::try_parse_from(["radroots", "farm", "rebind", "acct_test"])
            .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let TargetOperationRequest::FarmRebind(request) = request else {
            panic!("expected farm rebind request")
        };

        assert_eq!(request.operation_id(), "farm.rebind");
        assert_eq!(
            request
                .payload
                .input
                .get("selector")
                .and_then(Value::as_str),
            Some("acct_test")
        );
    }

    #[test]
    fn adapter_maps_listing_rebind_inputs() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "listing",
            "rebind",
            "listing.toml",
            "acct_test",
            "--farm-d-tag",
            "AAAAAAAAAAAAAAAAAAAAAw",
        ])
        .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let TargetOperationRequest::ListingRebind(request) = request else {
            panic!("expected listing rebind request")
        };

        assert_eq!(request.operation_id(), "listing.rebind");
        assert_eq!(
            request.payload.input.get("file").and_then(Value::as_str),
            Some("listing.toml")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("selector")
                .and_then(Value::as_str),
            Some("acct_test")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("farm_d_tag")
                .and_then(Value::as_str),
            Some("AAAAAAAAAAAAAAAAAAAAAw")
        );
    }

    #[test]
    fn adapter_maps_order_rebind_inputs() {
        let parsed =
            TargetCliArgs::try_parse_from(["radroots", "order", "rebind", "ord_test", "acct_test"])
                .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let TargetOperationRequest::OrderRebind(request) = request else {
            panic!("expected order rebind request")
        };

        assert_eq!(request.operation_id(), "order.rebind");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("selector")
                .and_then(Value::as_str),
            Some("acct_test")
        );
    }

    #[test]
    fn adapter_maps_order_fulfillment_update_input() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "fulfillment",
            "update",
            "ord_test",
            "--state",
            "seller_cancelled",
        ])
        .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let TargetOperationRequest::OrderFulfillmentUpdate(request) = request else {
            panic!("expected order fulfillment update request")
        };

        assert_eq!(request.operation_id(), "order.fulfillment.update");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request.payload.input.get("state").and_then(Value::as_str),
            Some("seller_cancelled")
        );
    }

    #[test]
    fn adapter_maps_order_lifecycle_inputs() {
        let revision = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "revision",
            "propose",
            "ord_test",
            "--reason",
            "update count",
            "--bin-id",
            "bin-1",
            "--bin-count",
            "3",
            "--adjustment-id",
            "adj-weather",
            "--adjustment-effect",
            "increase",
            "--adjustment-amount",
            "1.25",
            "--adjustment-currency",
            "USD",
            "--adjustment-reason",
            "weather delay",
        ])
        .expect("target args parse");
        let request =
            TargetOperationRequest::from_target_args(&revision).expect("operation request");
        let TargetOperationRequest::OrderRevisionPropose(request) = request else {
            panic!("expected order revision propose request")
        };
        assert_eq!(request.operation_id(), "order.revision.propose");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request.payload.input.get("reason").and_then(Value::as_str),
            Some("update count")
        );
        assert_eq!(
            request.payload.input.get("bin_id").and_then(Value::as_str),
            Some("bin-1")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("bin_count")
                .and_then(Value::as_u64),
            Some(3)
        );
        assert_eq!(
            request
                .payload
                .input
                .get("adjustment_id")
                .and_then(Value::as_str),
            Some("adj-weather")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("adjustment_effect")
                .and_then(Value::as_str),
            Some("increase")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("adjustment_amount")
                .and_then(Value::as_str),
            Some("1.25")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("adjustment_currency")
                .and_then(Value::as_str),
            Some("USD")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("adjustment_reason")
                .and_then(Value::as_str),
            Some("weather delay")
        );

        let revision_accept = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "revision",
            "accept",
            "ord_test",
            "--revision-id",
            "rev_test",
        ])
        .expect("target args parse");
        let request =
            TargetOperationRequest::from_target_args(&revision_accept).expect("operation request");
        let TargetOperationRequest::OrderRevisionAccept(request) = request else {
            panic!("expected order revision accept request")
        };
        assert_eq!(request.operation_id(), "order.revision.accept");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("revision_id")
                .and_then(Value::as_str),
            Some("rev_test")
        );

        let revision_decline = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "revision",
            "decline",
            "ord_test",
            "--revision-id",
            "rev_test",
            "--reason",
            "keep original order",
        ])
        .expect("target args parse");
        let request =
            TargetOperationRequest::from_target_args(&revision_decline).expect("operation request");
        let TargetOperationRequest::OrderRevisionDecline(request) = request else {
            panic!("expected order revision decline request")
        };
        assert_eq!(request.operation_id(), "order.revision.decline");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("revision_id")
                .and_then(Value::as_str),
            Some("rev_test")
        );
        assert_eq!(
            request.payload.input.get("reason").and_then(Value::as_str),
            Some("keep original order")
        );

        let cancel = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "cancel",
            "ord_test",
            "--reason",
            "changed plans",
        ])
        .expect("target args parse");
        let request = TargetOperationRequest::from_target_args(&cancel).expect("operation request");
        let TargetOperationRequest::OrderCancel(request) = request else {
            panic!("expected order cancel request")
        };
        assert_eq!(request.operation_id(), "order.cancel");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request.payload.input.get("reason").and_then(Value::as_str),
            Some("changed plans")
        );

        let receipt = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "receipt",
            "record",
            "ord_test",
            "--issue",
            "damaged items",
        ])
        .expect("target args parse");
        let request =
            TargetOperationRequest::from_target_args(&receipt).expect("operation request");
        let TargetOperationRequest::OrderReceiptRecord(request) = request else {
            panic!("expected order receipt record request")
        };
        assert_eq!(request.operation_id(), "order.receipt.record");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request.payload.input.get("issue").and_then(Value::as_str),
            Some("damaged items")
        );

        let payment = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "payment",
            "record",
            "ord_test",
            "--amount",
            "12",
            "--currency",
            "USD",
            "--method",
            "manual_transfer",
            "--reference",
            "memo-1",
            "--paid-at",
            "1777666000",
        ])
        .expect("target args parse");
        let request =
            TargetOperationRequest::from_target_args(&payment).expect("operation request");
        let TargetOperationRequest::OrderPaymentRecord(request) = request else {
            panic!("expected order payment record request")
        };
        assert_eq!(request.operation_id(), "order.payment.record");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request.payload.input.get("amount").and_then(Value::as_str),
            Some("12")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("currency")
                .and_then(Value::as_str),
            Some("USD")
        );
        assert_eq!(
            request.payload.input.get("method").and_then(Value::as_str),
            Some("manual_transfer")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("reference")
                .and_then(Value::as_str),
            Some("memo-1")
        );
        assert_eq!(
            request.payload.input.get("paid_at").and_then(Value::as_u64),
            Some(1_777_666_000)
        );

        let settlement = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "settlement",
            "reject",
            "ord_test",
            "--payment-event-id",
            "pay_event",
            "--reason",
            "reference mismatch",
        ])
        .expect("target args parse");
        let request =
            TargetOperationRequest::from_target_args(&settlement).expect("operation request");
        let TargetOperationRequest::OrderSettlementReject(request) = request else {
            panic!("expected order settlement reject request")
        };
        assert_eq!(request.operation_id(), "order.settlement.reject");
        assert_eq!(
            request
                .payload
                .input
                .get("order_id")
                .and_then(Value::as_str),
            Some("ord_test")
        );
        assert_eq!(
            request
                .payload
                .input
                .get("payment_event_id")
                .and_then(Value::as_str),
            Some("pay_event")
        );
        assert_eq!(
            request.payload.input.get("reason").and_then(Value::as_str),
            Some("reference mismatch")
        );
    }

    #[test]
    fn typed_service_boundary_returns_enveloped_result() {
        struct WorkspaceService;

        impl OperationService<WorkspaceGetRequest> for WorkspaceService {
            type Result = WorkspaceGetResult;

            fn execute(
                &self,
                request: OperationRequest<WorkspaceGetRequest>,
            ) -> Result<OperationResult<Self::Result>, super::OperationAdapterError> {
                assert_eq!(request.operation_id(), "workspace.get");
                OperationResult::new(WorkspaceGetResult::default())
            }
        }

        let adapter = OperationAdapter::new(WorkspaceService);
        let context = OperationContext::default();
        let request = OperationRequest::new(context.clone(), WorkspaceGetRequest::default())
            .expect("typed request");
        let result = adapter.execute(request).expect("typed result");
        let envelope = result
            .to_envelope(context.envelope_context("req_test"))
            .expect("operation envelope");

        assert_eq!(envelope.operation_id, "workspace.get");
        assert_eq!(envelope.kind, "workspace.get");
        assert_eq!(envelope.request_id, "req_test");
        assert_eq!(envelope.result, json!({}));
    }

    #[test]
    fn approval_errors_map_to_structured_exit_code() {
        let error = OperationAdapterError::approval_required("order.submit");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "approval_required");
        assert_eq!(output_error.exit_code, 6);
        assert!(output_error.message.contains("approval_token"));
    }

    #[test]
    fn not_implemented_errors_map_to_structured_exit_code() {
        let error = OperationAdapterError::not_implemented(
            "order.payment.record",
            "coming soon".to_owned(),
        );
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "not_implemented");
        assert_eq!(output_error.exit_code, 3);
        assert_eq!(
            output_error.detail.expect("detail")["operation_id"],
            "order.payment.record"
        );
    }

    #[test]
    fn runtime_failures_map_to_specific_machine_codes() {
        let cases = [
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "no selected account for seller write".to_owned(),
                ),
                "account_unresolved",
                "account",
                5,
            ),
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "resolved account `a` is watch_only and cannot sign because it is not secret-backed"
                        .to_owned(),
                ),
                "account_watch_only",
                "account",
                7,
            ),
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "account mismatch: resolved account pubkey `b` cannot sign listing seller_pubkey `a`"
                        .to_owned(),
                ),
                "account_mismatch",
                "account",
                5,
            ),
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "signer.remote_nip46 binding is missing".to_owned(),
                ),
                "signer_unconfigured",
                "signer",
                7,
            ),
            (
                OperationAdapterError::unavailable(
                    "listing.publish",
                    "radrootsd bridge is unavailable".to_owned(),
                ),
                "provider_unavailable",
                "provider",
                3,
            ),
            (
                OperationAdapterError::SignerModeDeferred {
                    operation_id: "signer.status.get".to_owned(),
                    message: "signer mode `myc` is deferred".to_owned(),
                },
                "signer_mode_deferred",
                "signer",
                7,
            ),
            (
                OperationAdapterError::unconfigured(
                    "basket.quote.create",
                    "quote engine not ready".to_owned(),
                ),
                "operation_unavailable",
                "operation",
                3,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "listing.publish",
                    RuntimeError::Io(io::Error::new(io::ErrorKind::NotFound, "missing draft")),
                ),
                "not_found",
                "resource",
                4,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "listing.validate",
                    RuntimeError::Config("invalid listing draft listing.toml".to_owned()),
                ),
                "validation_failed",
                "validation",
                10,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "listing.archive",
                    RuntimeError::Account(AccountRuntimeFailure::mismatch(
                        "account mismatch: resolved account pubkey `b` cannot sign listing seller_pubkey `a`",
                    )),
                ),
                "account_mismatch",
                "account",
                5,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "farm.publish",
                    RuntimeError::Network("direct relay connection failed".to_owned()),
                ),
                "network_unavailable",
                "network",
                8,
            ),
        ];

        for (error, code, class, exit_code) in cases {
            let output = error.to_output_error();
            assert_eq!(output.code, code);
            assert_eq!(output.exit_code, exit_code);
            assert_eq!(
                output.detail.expect("detail")["class"],
                serde_json::Value::String(class.to_owned())
            );
        }
    }
}
