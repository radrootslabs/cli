use std::fmt::Debug;

use serde::Serialize;
use serde_json::{Map, Value};

use super::context::OperationContext;
use super::error::OperationAdapterError;
use crate::cli::TargetCliArgs;
use crate::out::envelope::{
    EnvelopeContext, NextAction, OutputEnvelope, OutputWarning, next_actions_from_result_value,
};
use crate::registry::{OPERATION_REGISTRY, OperationSpec, get_operation};

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
        let result = serde_json::to_value(&self.payload)
            .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?;
        let next_actions = if self.next_actions.is_empty() {
            next_actions_from_result(&result)
        } else {
            self.next_actions.clone()
        };
        let mut envelope = OutputEnvelope::success(self.operation_id(), result, context);
        envelope.warnings = self.warnings.clone();
        envelope.next_actions = next_actions;
        Ok(envelope)
    }
}

fn next_actions_from_result(result: &Value) -> Vec<NextAction> {
    next_actions_from_result_value(result)
}

macro_rules! target_operation_contracts {
    ($( $variant:ident => ($request:ident, $result:ident, $operation_id:literal) ),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum TargetOperationRequest {
            $( $variant(OperationRequest<$request>), )+
        }

        impl TargetOperationRequest {
            pub fn from_target_args(args: &TargetCliArgs) -> Result<Self, OperationAdapterError> {
                Self::from_operation_id_with_input(
                    crate::cli::input::operation_id_from_target(args),
                    OperationContext::from_target_args(args),
                    crate::cli::input::target_operation_input(&args.command),
                )
            }

            pub fn from_operation_id(
                operation_id: &'static str,
                context: OperationContext,
            ) -> Result<Self, OperationAdapterError> {
                Self::from_operation_id_with_input(operation_id, context, OperationData::new())
            }

            fn from_operation_id_with_input(
                operation_id: &'static str,
                context: OperationContext,
                input: OperationData,
            ) -> Result<Self, OperationAdapterError> {
                match operation_id {
                    $( $operation_id => Ok(Self::$variant(OperationRequest::new(context, $request::from_data(input))?)), )+
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
        pub enum TargetOperationResult {
            $( $variant(OperationResult<$result>), )+
        }

        impl TargetOperationResult {
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

target_operation_contracts! {
    WorkspaceInit => (WorkspaceInitRequest, WorkspaceInitResult, "workspace.init"),
    WorkspaceGet => (WorkspaceGetRequest, WorkspaceGetResult, "workspace.get"),
    HealthStatusGet => (HealthStatusGetRequest, HealthStatusGetResult, "health.status.get"),
    HealthCheckRun => (HealthCheckRunRequest, HealthCheckRunResult, "health.check.run"),
    ConfigGet => (ConfigGetRequest, ConfigGetResult, "config.get"),
    AccountCreate => (AccountCreateRequest, AccountCreateResult, "account.create"),
    AccountImport => (AccountImportRequest, AccountImportResult, "account.import"),
    AccountAttachSecret => (AccountAttachSecretRequest, AccountAttachSecretResult, "account.attach_secret"),
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
    FarmCreate => (FarmCreateRequest, FarmCreateResult, "farm.create"),
    FarmGet => (FarmGetRequest, FarmGetResult, "farm.get"),
    FarmRebind => (FarmRebindRequest, FarmRebindResult, "farm.rebind"),
    FarmProfileUpdate => (FarmProfileUpdateRequest, FarmProfileUpdateResult, "farm.profile.update"),
    FarmLocationUpdate => (FarmLocationUpdateRequest, FarmLocationUpdateResult, "farm.location.update"),
    FarmFulfillmentUpdate => (FarmFulfillmentUpdateRequest, FarmFulfillmentUpdateResult, "farm.fulfillment.update"),
    FarmReadinessCheck => (FarmReadinessCheckRequest, FarmReadinessCheckResult, "farm.readiness.check"),
    FarmPublish => (FarmPublishRequest, FarmPublishResult, "farm.publish"),
    ListingCreate => (ListingCreateRequest, ListingCreateResult, "listing.create"),
    ListingGet => (ListingGetRequest, ListingGetResult, "listing.get"),
    ListingList => (ListingListRequest, ListingListResult, "listing.list"),
    ListingAppList => (ListingAppListRequest, ListingAppListResult, "listing.app.list"),
    ListingAppExport => (ListingAppExportRequest, ListingAppExportResult, "listing.app.export"),
    ListingUpdate => (ListingUpdateRequest, ListingUpdateResult, "listing.update"),
    ListingValidate => (ListingValidateRequest, ListingValidateResult, "listing.validate"),
    ListingRebind => (ListingRebindRequest, ListingRebindResult, "listing.rebind"),
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
    BasketAdjustmentAdd => (BasketAdjustmentAddRequest, BasketAdjustmentAddResult, "basket.adjustment.add"),
    BasketAdjustmentRemove => (BasketAdjustmentRemoveRequest, BasketAdjustmentRemoveResult, "basket.adjustment.remove"),
    BasketValidate => (BasketValidateRequest, BasketValidateResult, "basket.validate"),
    BasketQuoteCreate => (BasketQuoteCreateRequest, BasketQuoteCreateResult, "basket.quote.create"),
    OrderSubmit => (OrderSubmitRequest, OrderSubmitResult, "order.submit"),
    OrderGet => (OrderGetRequest, OrderGetResult, "order.get"),
    OrderList => (OrderListRequest, OrderListResult, "order.list"),
    OrderAppList => (OrderAppListRequest, OrderAppListResult, "order.app.list"),
    OrderAppExport => (OrderAppExportRequest, OrderAppExportResult, "order.app.export"),
    OrderRebind => (OrderRebindRequest, OrderRebindResult, "order.rebind"),
    OrderAccept => (OrderAcceptRequest, OrderAcceptResult, "order.accept"),
    OrderDecline => (OrderDeclineRequest, OrderDeclineResult, "order.decline"),
    OrderCancel => (OrderCancelRequest, OrderCancelResult, "order.cancel"),
    OrderRevisionPropose => (OrderRevisionProposeRequest, OrderRevisionProposeResult, "order.revision.propose"),
    OrderRevisionAccept => (OrderRevisionAcceptRequest, OrderRevisionAcceptResult, "order.revision.accept"),
    OrderRevisionDecline => (OrderRevisionDeclineRequest, OrderRevisionDeclineResult, "order.revision.decline"),
    OrderFulfillmentUpdate => (OrderFulfillmentUpdateRequest, OrderFulfillmentUpdateResult, "order.fulfillment.update"),
    OrderReceiptRecord => (OrderReceiptRecordRequest, OrderReceiptRecordResult, "order.receipt.record"),
    OrderPaymentRecord => (OrderPaymentRecordRequest, OrderPaymentRecordResult, "order.payment.record"),
    OrderSettlementAccept => (OrderSettlementAcceptRequest, OrderSettlementAcceptResult, "order.settlement.accept"),
    OrderSettlementReject => (OrderSettlementRejectRequest, OrderSettlementRejectResult, "order.settlement.reject"),
    OrderStatusGet => (OrderStatusGetRequest, OrderStatusGetResult, "order.status.get"),
    OrderEventList => (OrderEventListRequest, OrderEventListResult, "order.event.list"),
    OrderEventWatch => (OrderEventWatchRequest, OrderEventWatchResult, "order.event.watch"),
    ValidationReceiptGet => (ValidationReceiptGetRequest, ValidationReceiptGetResult, "validation.receipt.get"),
    ValidationReceiptList => (ValidationReceiptListRequest, ValidationReceiptListResult, "validation.receipt.list"),
    ValidationReceiptVerify => (ValidationReceiptVerifyRequest, ValidationReceiptVerifyResult, "validation.receipt.verify"),
}

pub fn adapter_registry_linkage_is_valid() -> bool {
    OPERATION_REGISTRY.iter().all(|operation| {
        TargetOperationRequest::request_type_for_operation(operation.operation_id)
            == Some(operation.rust_request)
            && TargetOperationResult::result_type_for_operation(operation.operation_id)
                == Some(operation.rust_result)
    })
}
