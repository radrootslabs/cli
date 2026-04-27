#![allow(dead_code)]

use std::fmt::Debug;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::operation_registry::{OPERATION_REGISTRY, OperationSpec, get_operation};
use crate::output_contract::{
    CliExitCode, EnvelopeContext, NextAction, OUTPUT_SCHEMA_VERSION, OutputEnvelope, OutputError,
    OutputWarning,
};
use crate::target_cli::{TargetCliArgs, TargetOutputFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutputFormat {
    Human,
    Json,
    Ndjson,
}

impl Default for OperationOutputFormat {
    fn default() -> Self {
        Self::Human
    }
}

impl From<TargetOutputFormat> for OperationOutputFormat {
    fn from(format: TargetOutputFormat) -> Self {
        match format {
            TargetOutputFormat::Human => Self::Human,
            TargetOutputFormat::Json => Self::Json,
            TargetOutputFormat::Ndjson => Self::Ndjson,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationNetworkMode {
    Default,
    Offline,
    Online,
}

impl Default for OperationNetworkMode {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationInputMode {
    PromptingAllowed,
    NoInput,
}

impl Default for OperationInputMode {
    fn default() -> Self {
        Self::PromptingAllowed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationContext {
    pub output_format: OperationOutputFormat,
    pub account_id: Option<String>,
    pub farm_id: Option<String>,
    pub profile: Option<String>,
    pub signer_session_id: Option<String>,
    pub relays: Vec<String>,
    pub network_mode: OperationNetworkMode,
    pub dry_run: bool,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    pub approval_token: Option<String>,
    pub input_mode: OperationInputMode,
    pub quiet: bool,
    pub verbose: bool,
    pub trace: bool,
    pub color: bool,
}

impl OperationContext {
    pub fn from_target_args(args: &TargetCliArgs) -> Self {
        Self {
            output_format: OperationOutputFormat::from(args.format),
            account_id: args.account_id.clone(),
            farm_id: args.farm_id.clone(),
            profile: args.profile.clone(),
            signer_session_id: args.signer_session_id.clone(),
            relays: args.relay.clone(),
            network_mode: if args.offline {
                OperationNetworkMode::Offline
            } else if args.online {
                OperationNetworkMode::Online
            } else {
                OperationNetworkMode::Default
            },
            dry_run: args.dry_run,
            idempotency_key: args.idempotency_key.clone(),
            correlation_id: args.correlation_id.clone(),
            approval_token: args.approval_token.clone(),
            input_mode: if args.no_input {
                OperationInputMode::NoInput
            } else {
                OperationInputMode::PromptingAllowed
            },
            quiet: args.quiet,
            verbose: args.verbose,
            trace: args.trace,
            color: !args.no_color,
        }
    }

    pub fn envelope_context(&self, request_id: impl Into<String>) -> EnvelopeContext {
        let mut context = EnvelopeContext::new(request_id, self.dry_run);
        context.correlation_id = self.correlation_id.clone();
        context.idempotency_key = self.idempotency_key.clone();
        context
    }
}

pub type OperationData = Map<String, Value>;

pub trait OperationRequestPayload: Debug + Clone + PartialEq + 'static {
    const OPERATION_ID: &'static str;
    const REQUEST_TYPE: &'static str;
}

pub trait OperationRequestData: OperationRequestPayload {
    fn input(&self) -> &OperationData;
}

pub trait OperationResultPayload: Debug + Clone + PartialEq + Serialize + 'static {
    const OPERATION_ID: &'static str;
    const RESULT_TYPE: &'static str;
}

pub trait OperationResultData: OperationResultPayload + Sized {
    fn from_data(data: OperationData) -> Self;

    fn from_value(value: Value) -> Self {
        Self::from_data(value_to_data(value))
    }

    fn from_serializable<T: Serialize>(value: &T) -> Result<Self, OperationAdapterError> {
        Ok(Self::from_value(serde_json::to_value(value).map_err(
            |error| OperationAdapterError::Serialization(error.to_string()),
        )?))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationRequest<P: OperationRequestPayload> {
    pub spec: &'static OperationSpec,
    pub context: OperationContext,
    pub payload: P,
}

impl<P: OperationRequestPayload> OperationRequest<P> {
    pub fn new(context: OperationContext, payload: P) -> Result<Self, OperationAdapterError> {
        let spec = get_operation(P::OPERATION_ID)
            .ok_or_else(|| OperationAdapterError::UnknownOperation(P::OPERATION_ID.to_owned()))?;
        if spec.rust_request != P::REQUEST_TYPE {
            return Err(OperationAdapterError::RequestTypeMismatch {
                operation_id: P::OPERATION_ID.to_owned(),
                registry_request: spec.rust_request.to_owned(),
                adapter_request: P::REQUEST_TYPE.to_owned(),
            });
        }
        Ok(Self {
            spec,
            context,
            payload,
        })
    }

    pub fn operation_id(&self) -> &'static str {
        P::OPERATION_ID
    }

    pub fn request_type_name(&self) -> &'static str {
        P::REQUEST_TYPE
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationResult<P: OperationResultPayload> {
    pub spec: &'static OperationSpec,
    pub payload: P,
    pub warnings: Vec<OutputWarning>,
    pub next_actions: Vec<NextAction>,
}

impl<P: OperationResultPayload> OperationResult<P> {
    pub fn new(payload: P) -> Result<Self, OperationAdapterError> {
        let spec = get_operation(P::OPERATION_ID)
            .ok_or_else(|| OperationAdapterError::UnknownOperation(P::OPERATION_ID.to_owned()))?;
        if spec.rust_result != P::RESULT_TYPE {
            return Err(OperationAdapterError::ResultTypeMismatch {
                operation_id: P::OPERATION_ID.to_owned(),
                registry_result: spec.rust_result.to_owned(),
                adapter_result: P::RESULT_TYPE.to_owned(),
            });
        }
        Ok(Self {
            spec,
            payload,
            warnings: Vec::new(),
            next_actions: Vec::new(),
        })
    }

    pub fn operation_id(&self) -> &'static str {
        P::OPERATION_ID
    }

    pub fn result_type_name(&self) -> &'static str {
        P::RESULT_TYPE
    }

    pub fn to_envelope(
        &self,
        context: EnvelopeContext,
    ) -> Result<OutputEnvelope, OperationAdapterError> {
        Ok(OutputEnvelope {
            schema_version: OUTPUT_SCHEMA_VERSION,
            operation_id: self.operation_id().to_owned(),
            kind: self.operation_id().to_owned(),
            request_id: context.request_id,
            correlation_id: context.correlation_id,
            idempotency_key: context.idempotency_key,
            dry_run: context.dry_run,
            actor: context.actor,
            result: serde_json::to_value(&self.payload)
                .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?,
            warnings: self.warnings.clone(),
            errors: Vec::new(),
            next_actions: self.next_actions.clone(),
        })
    }
}

pub trait OperationService<P: OperationRequestPayload> {
    type Result: OperationResultPayload;

    fn execute(
        &self,
        request: OperationRequest<P>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError>;
}

#[derive(Debug, Clone)]
pub struct OperationAdapter<S> {
    service: S,
}

impl<S> OperationAdapter<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn execute<P>(
        &self,
        request: OperationRequest<P>,
    ) -> Result<OperationResult<<S as OperationService<P>>::Result>, OperationAdapterError>
    where
        P: OperationRequestPayload,
        S: OperationService<P>,
    {
        self.service.execute(request)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OperationAdapterError {
    #[error("unknown operation `{0}`")]
    UnknownOperation(String),
    #[error(
        "operation `{operation_id}` registry request `{registry_request}` does not match adapter request `{adapter_request}`"
    )]
    RequestTypeMismatch {
        operation_id: String,
        registry_request: String,
        adapter_request: String,
    },
    #[error(
        "operation `{operation_id}` registry result `{registry_result}` does not match adapter result `{adapter_result}`"
    )]
    ResultTypeMismatch {
        operation_id: String,
        registry_result: String,
        adapter_result: String,
    },
    #[error("failed to serialize operation result: {0}")]
    Serialization(String),
    #[error("invalid operation input for `{operation_id}`: {message}")]
    InvalidInput {
        operation_id: String,
        message: String,
    },
    #[error("approval required for `{operation_id}`: {message}")]
    ApprovalRequired {
        operation_id: String,
        message: String,
    },
    #[error("operation runtime error: {0}")]
    Runtime(String),
}

impl OperationAdapterError {
    pub fn approval_required(operation_id: &str) -> Self {
        Self::ApprovalRequired {
            operation_id: operation_id.to_owned(),
            message: "missing required `approval_token` input".to_owned(),
        }
    }

    pub fn to_output_error(&self) -> OutputError {
        match self {
            Self::ApprovalRequired { message, .. } => OutputError::new(
                "approval_required",
                message.clone(),
                CliExitCode::ApprovalRequiredOrDenied,
            ),
            Self::InvalidInput { message, .. } => {
                OutputError::new("invalid_input", message.clone(), CliExitCode::InvalidInput)
            }
            Self::UnknownOperation(operation_id) => OutputError::new(
                "unknown_operation",
                format!("unknown operation `{operation_id}`"),
                CliExitCode::InvalidInput,
            ),
            Self::RequestTypeMismatch { .. } | Self::ResultTypeMismatch { .. } => OutputError::new(
                "contract_mismatch",
                self.to_string(),
                CliExitCode::InternalError,
            ),
            Self::Serialization(message) => OutputError::new(
                "serialization_failed",
                message.clone(),
                CliExitCode::InternalError,
            ),
            Self::Runtime(message) => {
                OutputError::new("runtime_error", message.clone(), CliExitCode::InternalError)
            }
        }
    }
}

macro_rules! mvp_operation_contracts {
    ($( $variant:ident => ($request:ident, $result:ident, $operation_id:literal) ),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum MvpOperationRequest {
            $( $variant(OperationRequest<$request>), )+
        }

        impl MvpOperationRequest {
            pub fn from_target_args(args: &TargetCliArgs) -> Result<Self, OperationAdapterError> {
                Self::from_operation_id(args.command.operation_id(), OperationContext::from_target_args(args))
            }

            pub fn from_operation_id(
                operation_id: &'static str,
                context: OperationContext,
            ) -> Result<Self, OperationAdapterError> {
                match operation_id {
                    $( $operation_id => Ok(Self::$variant(OperationRequest::new(context, $request::default())?)), )+
                    _ => Err(OperationAdapterError::UnknownOperation(operation_id.to_owned())),
                }
            }

            pub fn operation_id(&self) -> &'static str {
                match self {
                    $( Self::$variant(request) => request.operation_id(), )+
                }
            }

            pub fn spec(&self) -> &'static OperationSpec {
                match self {
                    $( Self::$variant(request) => request.spec, )+
                }
            }

            pub fn context(&self) -> &OperationContext {
                match self {
                    $( Self::$variant(request) => &request.context, )+
                }
            }

            pub fn request_type_name(&self) -> &'static str {
                match self {
                    $( Self::$variant(request) => request.request_type_name(), )+
                }
            }

            pub fn request_type_for_operation(operation_id: &str) -> Option<&'static str> {
                match operation_id {
                    $( $operation_id => Some(stringify!($request)), )+
                    _ => None,
                }
            }
        }

        #[derive(Debug, Clone, PartialEq)]
        pub enum MvpOperationResult {
            $( $variant(OperationResult<$result>), )+
        }

        impl MvpOperationResult {
            pub fn operation_id(&self) -> &'static str {
                match self {
                    $( Self::$variant(result) => result.operation_id(), )+
                }
            }

            pub fn result_type_name(&self) -> &'static str {
                match self {
                    $( Self::$variant(result) => result.result_type_name(), )+
                }
            }

            pub fn result_type_for_operation(operation_id: &str) -> Option<&'static str> {
                match operation_id {
                    $( $operation_id => Some(stringify!($result)), )+
                    _ => None,
                }
            }
        }

        $(
            #[derive(Debug, Default, Clone, PartialEq, Serialize)]
            pub struct $request {
                #[serde(flatten)]
                pub input: OperationData,
            }

            impl $request {
                pub fn from_data(input: OperationData) -> Self {
                    Self { input }
                }
            }

            impl OperationRequestPayload for $request {
                const OPERATION_ID: &'static str = $operation_id;
                const REQUEST_TYPE: &'static str = stringify!($request);
            }

            impl OperationRequestData for $request {
                fn input(&self) -> &OperationData {
                    &self.input
                }
            }

            #[derive(Debug, Default, Clone, PartialEq, Serialize)]
            pub struct $result {
                #[serde(flatten)]
                pub data: OperationData,
            }

            impl $result {
                pub fn from_data(data: OperationData) -> Self {
                    Self { data }
                }

                pub fn from_value(value: Value) -> Self {
                    Self {
                        data: value_to_data(value),
                    }
                }

                pub fn from_serializable<T: Serialize>(
                    value: &T,
                ) -> Result<Self, OperationAdapterError> {
                    Ok(Self::from_value(
                        serde_json::to_value(value)
                            .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?,
                    ))
                }
            }

            impl OperationResultPayload for $result {
                const OPERATION_ID: &'static str = $operation_id;
                const RESULT_TYPE: &'static str = stringify!($result);
            }

            impl OperationResultData for $result {
                fn from_data(data: OperationData) -> Self {
                    Self { data }
                }
            }
        )+
    };
}

fn value_to_data(value: Value) -> OperationData {
    match value {
        Value::Object(map) => map,
        other => {
            let mut map = OperationData::new();
            map.insert("value".to_owned(), other);
            map
        }
    }
}

mvp_operation_contracts! {
    WorkspaceInit => (WorkspaceInitRequest, WorkspaceInitResult, "workspace.init"),
    WorkspaceGet => (WorkspaceGetRequest, WorkspaceGetResult, "workspace.get"),
    HealthStatusGet => (HealthStatusGetRequest, HealthStatusGetResult, "health.status.get"),
    HealthCheckRun => (HealthCheckRunRequest, HealthCheckRunResult, "health.check.run"),
    ConfigGet => (ConfigGetRequest, ConfigGetResult, "config.get"),
    AccountCreate => (AccountCreateRequest, AccountCreateResult, "account.create"),
    AccountImport => (AccountImportRequest, AccountImportResult, "account.import"),
    AccountGet => (AccountGetRequest, AccountGetResult, "account.get"),
    AccountList => (AccountListRequest, AccountListResult, "account.list"),
    AccountRemove => (AccountRemoveRequest, AccountRemoveResult, "account.remove"),
    AccountSelectionGet => (AccountSelectionGetRequest, AccountSelectionGetResult, "account.selection.get"),
    AccountSelectionUpdate => (AccountSelectionUpdateRequest, AccountSelectionUpdateResult, "account.selection.update"),
    AccountSelectionClear => (AccountSelectionClearRequest, AccountSelectionClearResult, "account.selection.clear"),
    SignerStatusGet => (SignerStatusGetRequest, SignerStatusGetResult, "signer.status.get"),
    RelayList => (RelayListRequest, RelayListResult, "relay.list"),
    StoreInit => (StoreInitRequest, StoreInitResult, "store.init"),
    StoreStatusGet => (StoreStatusGetRequest, StoreStatusGetResult, "store.status.get"),
    StoreExport => (StoreExportRequest, StoreExportResult, "store.export"),
    StoreBackupCreate => (StoreBackupCreateRequest, StoreBackupCreateResult, "store.backup.create"),
    SyncStatusGet => (SyncStatusGetRequest, SyncStatusGetResult, "sync.status.get"),
    SyncPull => (SyncPullRequest, SyncPullResult, "sync.pull"),
    SyncPush => (SyncPushRequest, SyncPushResult, "sync.push"),
    SyncWatch => (SyncWatchRequest, SyncWatchResult, "sync.watch"),
    RuntimeStatusGet => (RuntimeStatusGetRequest, RuntimeStatusGetResult, "runtime.status.get"),
    RuntimeStart => (RuntimeStartRequest, RuntimeStartResult, "runtime.start"),
    RuntimeStop => (RuntimeStopRequest, RuntimeStopResult, "runtime.stop"),
    RuntimeRestart => (RuntimeRestartRequest, RuntimeRestartResult, "runtime.restart"),
    RuntimeLogWatch => (RuntimeLogWatchRequest, RuntimeLogWatchResult, "runtime.log.watch"),
    RuntimeConfigGet => (RuntimeConfigGetRequest, RuntimeConfigGetResult, "runtime.config.get"),
    JobGet => (JobGetRequest, JobGetResult, "job.get"),
    JobList => (JobListRequest, JobListResult, "job.list"),
    JobWatch => (JobWatchRequest, JobWatchResult, "job.watch"),
    FarmCreate => (FarmCreateRequest, FarmCreateResult, "farm.create"),
    FarmGet => (FarmGetRequest, FarmGetResult, "farm.get"),
    FarmProfileUpdate => (FarmProfileUpdateRequest, FarmProfileUpdateResult, "farm.profile.update"),
    FarmLocationUpdate => (FarmLocationUpdateRequest, FarmLocationUpdateResult, "farm.location.update"),
    FarmFulfillmentUpdate => (FarmFulfillmentUpdateRequest, FarmFulfillmentUpdateResult, "farm.fulfillment.update"),
    FarmReadinessCheck => (FarmReadinessCheckRequest, FarmReadinessCheckResult, "farm.readiness.check"),
    FarmPublish => (FarmPublishRequest, FarmPublishResult, "farm.publish"),
    ListingCreate => (ListingCreateRequest, ListingCreateResult, "listing.create"),
    ListingGet => (ListingGetRequest, ListingGetResult, "listing.get"),
    ListingList => (ListingListRequest, ListingListResult, "listing.list"),
    ListingUpdate => (ListingUpdateRequest, ListingUpdateResult, "listing.update"),
    ListingValidate => (ListingValidateRequest, ListingValidateResult, "listing.validate"),
    ListingPublish => (ListingPublishRequest, ListingPublishResult, "listing.publish"),
    ListingArchive => (ListingArchiveRequest, ListingArchiveResult, "listing.archive"),
    MarketRefresh => (MarketRefreshRequest, MarketRefreshResult, "market.refresh"),
    MarketProductSearch => (MarketProductSearchRequest, MarketProductSearchResult, "market.product.search"),
    MarketListingGet => (MarketListingGetRequest, MarketListingGetResult, "market.listing.get"),
    BasketCreate => (BasketCreateRequest, BasketCreateResult, "basket.create"),
    BasketGet => (BasketGetRequest, BasketGetResult, "basket.get"),
    BasketList => (BasketListRequest, BasketListResult, "basket.list"),
    BasketItemAdd => (BasketItemAddRequest, BasketItemAddResult, "basket.item.add"),
    BasketItemUpdate => (BasketItemUpdateRequest, BasketItemUpdateResult, "basket.item.update"),
    BasketItemRemove => (BasketItemRemoveRequest, BasketItemRemoveResult, "basket.item.remove"),
    BasketValidate => (BasketValidateRequest, BasketValidateResult, "basket.validate"),
    BasketQuoteCreate => (BasketQuoteCreateRequest, BasketQuoteCreateResult, "basket.quote.create"),
    OrderSubmit => (OrderSubmitRequest, OrderSubmitResult, "order.submit"),
    OrderGet => (OrderGetRequest, OrderGetResult, "order.get"),
    OrderList => (OrderListRequest, OrderListResult, "order.list"),
    OrderEventList => (OrderEventListRequest, OrderEventListResult, "order.event.list"),
    OrderEventWatch => (OrderEventWatchRequest, OrderEventWatchResult, "order.event.watch"),
}

pub fn adapter_registry_linkage_is_valid() -> bool {
    OPERATION_REGISTRY.iter().all(|operation| {
        MvpOperationRequest::request_type_for_operation(operation.operation_id)
            == Some(operation.rust_request)
            && MvpOperationResult::result_type_for_operation(operation.operation_id)
                == Some(operation.rust_result)
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::json;

    use super::{
        MvpOperationRequest, OperationAdapter, OperationAdapterError, OperationContext,
        OperationInputMode, OperationNetworkMode, OperationOutputFormat, OperationRequest,
        OperationResult, OperationService, WorkspaceGetRequest, WorkspaceGetResult,
        adapter_registry_linkage_is_valid,
    };
    use crate::operation_registry::OPERATION_REGISTRY;
    use crate::target_cli::TargetCliArgs;

    #[test]
    fn adapter_binds_every_registry_entry() {
        assert!(adapter_registry_linkage_is_valid());

        for operation in OPERATION_REGISTRY {
            let parsed = TargetCliArgs::try_parse_from(operation.cli_path.split_whitespace())
                .unwrap_or_else(|error| {
                    panic!("{} failed to parse: {error}", operation.cli_path);
                });
            let request = MvpOperationRequest::from_target_args(&parsed)
                .expect("operation request from target args");

            assert_eq!(request.operation_id(), operation.operation_id);
            assert_eq!(request.spec().mcp_tool, operation.mcp_tool);
            assert_eq!(request.request_type_name(), operation.rust_request);
            assert_eq!(
                MvpOperationRequest::request_type_for_operation(operation.operation_id),
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
            "--farm-id",
            "farm_test",
            "--profile",
            "repo_local",
            "--signer-session-id",
            "sess_test",
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

        let request = MvpOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let context = request.context();

        assert_eq!(context.output_format, OperationOutputFormat::Json);
        assert_eq!(context.account_id.as_deref(), Some("acct_test"));
        assert_eq!(context.farm_id.as_deref(), Some("farm_test"));
        assert_eq!(context.profile.as_deref(), Some("repo_local"));
        assert_eq!(context.signer_session_id.as_deref(), Some("sess_test"));
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
}
