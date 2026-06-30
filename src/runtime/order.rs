#![allow(dead_code)]

mod sdk_status;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_authority::RadrootsActorContext;
use radroots_core::{
    RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreDiscount, RadrootsCoreDiscountScope,
    RadrootsCoreDiscountThreshold, RadrootsCoreDiscountValue, RadrootsCoreMoney, RadrootsCoreUnit,
    convert_unit_decimal,
};
use radroots_events::contract::RadrootsActorRole;
use radroots_events::ids::{
    RadrootsEconomicsDigest, RadrootsEventId, RadrootsInventoryBinId, RadrootsListingAddress,
    RadrootsOrderId, RadrootsOrderQuoteId, RadrootsOrderRevisionId, RadrootsPublicKey,
};
use radroots_events::kinds::{
    KIND_LISTING, KIND_ORDER_CANCELLATION, KIND_ORDER_DECISION, KIND_ORDER_REQUEST,
    KIND_ORDER_REVISION_DECISION, KIND_ORDER_REVISION_PROPOSAL,
};
use radroots_events::order::{
    RadrootsOrderCancellation, RadrootsOrderDecision, RadrootsOrderEconomicActor,
    RadrootsOrderEconomicEffect, RadrootsOrderEconomicItem, RadrootsOrderEconomicLine,
    RadrootsOrderEconomicLineKind, RadrootsOrderEconomics, RadrootsOrderEventType,
    RadrootsOrderInventoryCommitment, RadrootsOrderItem, RadrootsOrderPricingBasis,
    RadrootsOrderRequest, RadrootsOrderRevisionDecision, RadrootsOrderRevisionOutcome,
    RadrootsOrderRevisionProposal,
};
use radroots_events::{RadrootsNostrEvent as SdkRadrootsNostrEvent, RadrootsNostrEventPtr};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::order::{
    order_cancellation_from_event, order_envelope_from_event, order_event_context_from_tags,
    order_request_from_event, order_revision_decision_from_event,
    order_revision_proposal_from_event,
};
use radroots_local_events::{
    BUYER_ORDER_REQUEST_LOCAL_WORK_RECORD_KIND, LocalEventRecord, LocalRecordFamily,
    LocalRecordStatus, PublishOutboxStatus, RelayDeliveryEvidence, RelayDeliveryState,
    SourceRuntime, normalize_relay_urls, validate_supported_buyer_order_request_local_work_payload,
};
use radroots_nostr::prelude::{
    RadrootsNostrEvent, RadrootsNostrFilter, radroots_event_from_nostr, radroots_nostr_filter_tag,
    radroots_nostr_kind,
};
use radroots_replica_db::{
    ReplicaSql, ReplicaTradeProductSummaryRow, nostr_event_head, trade_product,
};
use radroots_replica_db_schema::nostr_event_head::{
    INostrEventHeadFindOne, INostrEventHeadFindOneArgs, NostrEventHeadQueryBindValues,
};
use radroots_replica_db_schema::trade_product::{
    ITradeProductFieldsFilter, ITradeProductFindMany, TradeProduct,
};
use radroots_sdk::{
    AckPolicy, PrivacyPreflightConfirmation, ProductSensitivityField, PublishMode,
    PushOutboxEventReceipt, PushOutboxEventState, PushOutboxReceipt, PushOutboxRelayOutcomeKind,
    RelayResolutionPolicy, SdkMutationState, TradeAcceptRequest, TradeCancelRequest,
    TradeCancellationPlan, TradeCancellationReceipt, TradeDecisionPlan, TradeDecisionReceipt,
    TradeDeclineRequest, TradeEvidenceIngestRequest, TradeMutationOutcome, TradeProposeRequest,
    TradeRevisionDecisionPlan, TradeRevisionDecisionReceipt, TradeRevisionDecisionRequest,
    TradeRevisionProposalPlan, TradeRevisionProposalReceipt, TradeRevisionProposalRequest,
    TradeStatusReceipt, TradeStatusRequest, TradeSubmitPlan, TradeSubmitReceipt,
    TradeWorkflowEnqueueReceipt,
};
use radroots_sql_core::SqliteExecutor;
use radroots_trade::identity::RadrootsTradeLocator;
use radroots_trade::order::{
    RadrootsOrderCancellationRecord, RadrootsOrderDecisionRecord, RadrootsOrderRequestRecord,
    RadrootsOrderRevisionDecisionRecord, RadrootsOrderRevisionProposalRecord,
    canonicalize_order_request_for_signer,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::global::{
    OrderDraftCreateArgs, RecordLookupArgs, TradeAppRecordExportArgs, TradeCancelArgs,
    TradeDecisionArg, TradeDecisionArgs, TradeRebindArgs, TradeRevisionDecisionArg,
    TradeRevisionDecisionArgs, TradeRevisionProposeArgs, TradeStatusArgs, TradeSubmitArgs,
};
use crate::runtime::RuntimeError;
use crate::runtime::account;
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayFetchError, DirectRelayFetchReceipt, fetch_events_from_relays,
};
use crate::runtime::local_events::{
    get_shared_record, list_shared_records_before, list_shared_records_latest,
    shared_local_events_db_path,
};
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession};
use crate::runtime::signer::ActorWriteBindingError;
use crate::runtime::sync::{
    RelayIngestScope, freshness_for_scope, freshness_requires_refresh, market_refresh,
    relay_provenance_relays_for_scope,
};
use crate::view::runtime::{
    OrderAppRecordExportView, OrderAppRecordListView, OrderAppRecordSummaryView,
    OrderCancellationView, OrderDecisionView, OrderDraftItemView, OrderEventListEntryView,
    OrderEventListView, OrderGetView, OrderIssueView, OrderListView, OrderNewView, OrderRebindView,
    OrderRevisionDecisionView, OrderRevisionProposalView, OrderStatusView, OrderSubmitView,
    OrderSummaryView, OrderTradeLocatorView, RelayFailureView,
};

use self::sdk_status::sdk_order_status_view;

const ORDER_DRAFT_KIND: &str = "order_draft_v1";
const ORDER_SOURCE: &str = "local trade drafts · local first";
const ORDER_APP_RECORD_SOURCE: &str = "app-authored shared local trade records";
const ORDER_SUBMIT_SOURCE: &str = "SDK trade submit · local key";
const ORDER_DECISION_SOURCE: &str = "SDK trade decision · local key";
const ORDER_REVISION_PROPOSAL_SOURCE: &str = "SDK trade revision proposal · local key";
const ORDER_REVISION_DECISION_SOURCE: &str = "SDK trade revision decision · local key";
const ORDER_CANCELLATION_SOURCE: &str = "SDK trade cancellation · local key";
const ORDER_EVENT_LIST_SOURCE: &str = "direct Nostr relay fetch · selected seller identity";
const ORDER_STATUS_SDK_SOURCE: &str = "SDK local trade projection";
const ORDER_EVENT_LIST_RELAY_ACTION: &str =
    "radroots --relay wss://relay.example.com trade event list";
const ORDER_BUYER_ACTOR_SOURCE_RESOLVED_ACCOUNT: &str = "resolved_account";
const ORDER_BUYER_ACTOR_SOURCE_REBIND: &str = "order_rebind";
const ORDER_APP_RECORD_LIST_LIMIT: u32 = 500;
const ORDER_ACTOR_CONTEXT_ORDER_DRAFT: &str = "order_draft";
const ORDER_ACTOR_CONTEXT_RESOLVED_ACCOUNT: &str = "resolved_account";
const ORDER_ACTOR_CONTEXT_NETWORK_ONLY: &str = "network_only";
const ORDER_ACTOR_CONTEXT_SDK_LOCAL: &str = "sdk_local_projection";
const ORDERS_DIR: &str = "orders/drafts";
const APP_ORDER_ALREADY_SUBMITTED_ISSUE: &str = "app_order_already_submitted";
const APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE: &str = "app_order_signed_evidence_conflict";

static ORDER_COUNTER: AtomicU64 = AtomicU64::new(0);

fn order_locator_view_from_key(key: &str) -> OrderTradeLocatorView {
    OrderTradeLocatorView {
        trade_id: key.to_owned(),
        root_event_id: None,
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
    }
}

fn order_locator_view_from_locator(locator: &RadrootsTradeLocator) -> OrderTradeLocatorView {
    OrderTradeLocatorView {
        trade_id: locator.trade_id.as_str().to_owned(),
        root_event_id: locator.root_event_id.as_ref().map(ToString::to_string),
        listing_addr: locator.listing_addr.as_ref().map(ToString::to_string),
        buyer_pubkey: locator.buyer_pubkey.as_ref().map(ToString::to_string),
        seller_pubkey: locator.seller_pubkey.as_ref().map(ToString::to_string),
    }
}

fn order_locator_view_from_status(status: &OrderStatusView) -> OrderTradeLocatorView {
    OrderTradeLocatorView {
        trade_id: status.order_id.clone(),
        root_event_id: status.request_event_id.clone(),
        listing_addr: status.listing_addr.clone(),
        buyer_pubkey: status.buyer_pubkey.clone(),
        seller_pubkey: status.seller_pubkey.clone(),
    }
}

fn trade_locator_from_key(key: &str) -> Result<RadrootsTradeLocator, CliSdkAdapterError> {
    Ok(TradeStatusRequest::parse(key)?.locator)
}

fn trade_publish_mode(config: &RuntimeConfig) -> PublishMode {
    if config.output.dry_run {
        PublishMode::DryRun
    } else {
        PublishMode::EnqueueAndPublish
    }
}

fn trade_ack_policy(mode: PublishMode) -> Result<AckPolicy, RuntimeError> {
    Ok(match mode {
        PublishMode::DryRun | PublishMode::EnqueueOnly => AckPolicy::NoWait,
        PublishMode::EnqueueAndPublish => AckPolicy::AtLeastOneRelay,
        _ => {
            return Err(RuntimeError::Config(
                "unsupported SDK publish mode for CLI trade workflow".to_owned(),
            ));
        }
    })
}

fn trade_relay_resolution_policy() -> RelayResolutionPolicy {
    RelayResolutionPolicy::configured_relays()
}

fn trade_privacy_confirmation() -> PrivacyPreflightConfirmation {
    PrivacyPreflightConfirmation::new().confirm(ProductSensitivityField::PublicButSensitiveNotes)
}

fn sdk_trade_actor(
    account: &account::AccountRecordView,
    role: RadrootsActorRole,
    operation: &str,
) -> Result<RadrootsActorContext, RuntimeError> {
    RadrootsActorContext::local_account(
        account.record.public_identity.public_key_hex.as_str(),
        account.record.account_id.to_string(),
        [role],
    )
    .map_err(|error| RuntimeError::Config(format!("invalid trade {operation} SDK actor: {error}")))
}

fn connect_sdk_for_trade_actor(
    config: &RuntimeConfig,
    account: &account::AccountRecordView,
    actor_label: &str,
) -> Result<CliSdkSession, CliSdkAdapterError> {
    CliSdkSession::connect_for_actor(
        config,
        Some(account.record.account_id.as_str()),
        account.record.public_identity.public_key_hex.as_str(),
        actor_label,
    )
}

fn protocol_order_id(value: &str, field: &str) -> Result<RadrootsOrderId, RuntimeError> {
    value
        .parse()
        .map_err(|error| RuntimeError::Config(format!("{field} is not a valid order id: {error}")))
}

fn protocol_listing_addr(value: &str, field: &str) -> Result<RadrootsListingAddress, RuntimeError> {
    value.parse().map_err(|error| {
        RuntimeError::Config(format!("{field} is not a valid listing address: {error}"))
    })
}

fn protocol_revision_id(value: &str, field: &str) -> Result<RadrootsOrderRevisionId, RuntimeError> {
    value.parse().map_err(|error| {
        RuntimeError::Config(format!("{field} is not a valid trade revision id: {error}"))
    })
}

fn protocol_quote_id(value: &str, field: &str) -> Result<RadrootsOrderQuoteId, RuntimeError> {
    value.parse().map_err(|error| {
        RuntimeError::Config(format!("{field} is not a valid order quote id: {error}"))
    })
}

fn protocol_inventory_bin_id(
    value: &str,
    field: &str,
) -> Result<RadrootsInventoryBinId, RuntimeError> {
    value.parse().map_err(|error| {
        RuntimeError::Config(format!("{field} is not a valid inventory bin id: {error}"))
    })
}

fn protocol_economics_digest(
    value: &str,
    field: &str,
) -> Result<RadrootsEconomicsDigest, RuntimeError> {
    value.parse().map_err(|error| {
        RuntimeError::Config(format!("{field} is not a valid economics digest: {error}"))
    })
}

fn protocol_event_id(value: &str, field: &str) -> Result<RadrootsEventId, RuntimeError> {
    value
        .parse()
        .map_err(|error| RuntimeError::Config(format!("{field} is not a valid event id: {error}")))
}

fn protocol_pubkey(value: &str, field: &str) -> Result<RadrootsPublicKey, RuntimeError> {
    value
        .parse()
        .map_err(|error| RuntimeError::Config(format!("{field} is not a valid pubkey: {error}")))
}

fn required_order_context_event_id(
    event_id: Option<RadrootsEventId>,
    tag: &'static str,
    message: &'static str,
) -> Result<RadrootsEventId, RuntimeError> {
    event_id.ok_or_else(|| RuntimeError::Config(format!("{message} is missing {tag}")))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraftDocument {
    version: u32,
    kind: String,
    order: OrderDraft,
    buyer_actor: OrderDraftBuyerActor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    listing_lookup: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraft {
    order_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    listing_addr: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    listing_event_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    listing_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    buyer_pubkey: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    seller_pubkey: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<OrderDraftItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    economics: Option<RadrootsOrderEconomics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraftItem {
    bin_id: String,
    bin_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraftBuyerActor {
    account_id: String,
    pubkey: String,
    source: String,
}

#[derive(Debug, Clone)]
struct LoadedOrderDraft {
    file: PathBuf,
    updated_at_unix: u64,
    document: OrderDraftDocument,
}

#[derive(Debug, Clone)]
struct LoadedAppOrderRecord {
    record: LocalEventRecord,
    loaded: LoadedOrderDraft,
    source_issues: Vec<OrderIssueView>,
}

#[derive(Debug, Clone)]
struct AppOrderRecordListEntry {
    record: LocalEventRecord,
    superseded_count: usize,
}

#[derive(Debug, Clone)]
struct ResolvedOrderListing {
    listing_addr: String,
    listing_event_id: String,
    listing_relays: Vec<String>,
    seller_pubkey: String,
    economics_product: Option<ResolvedOrderEconomicsProduct>,
}

#[derive(Debug, Clone)]
struct ResolvedOrderEconomicsProduct {
    qty_amt_exact: Option<String>,
    qty_unit: String,
    price_amt_exact: Option<String>,
    price_currency: String,
    price_qty_amt_exact: Option<String>,
    price_qty_unit: String,
    primary_bin_id: Option<String>,
    verified_primary_bin_id: Option<String>,
    notes: Option<String>,
}

impl ResolvedOrderEconomicsProduct {
    fn from_summary(row: &ReplicaTradeProductSummaryRow) -> Self {
        Self {
            qty_amt_exact: row.qty_amt_exact.clone(),
            qty_unit: row.qty_unit.clone(),
            price_amt_exact: row.price_amt_exact.clone(),
            price_currency: row.price_currency.clone(),
            price_qty_amt_exact: row.price_qty_amt_exact.clone(),
            price_qty_unit: row.price_qty_unit.clone(),
            primary_bin_id: row.primary_bin_id.clone(),
            verified_primary_bin_id: row.verified_primary_bin_id.clone(),
            notes: row.notes.clone(),
        }
    }

    fn from_product(row: TradeProduct) -> Self {
        Self {
            qty_amt_exact: row.qty_amt_exact,
            qty_unit: row.qty_unit,
            price_amt_exact: row.price_amt_exact,
            price_currency: row.price_currency,
            price_qty_amt_exact: row.price_qty_amt_exact,
            price_qty_unit: row.price_qty_unit,
            primary_bin_id: row.primary_bin_id,
            verified_primary_bin_id: row.verified_primary_bin_id,
            notes: row.notes,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ResolvedTradeProductNotes {
    #[serde(default)]
    listing_discounts: Vec<RadrootsCoreDiscount>,
}

#[derive(Debug, Clone)]
struct ResolvedSellerOrderRequest {
    request_event: SdkRadrootsNostrEvent,
    request_event_id: RadrootsEventId,
    listing_event_id: Option<String>,
    order_id: RadrootsOrderId,
    listing_addr: RadrootsListingAddress,
    buyer_pubkey: RadrootsPublicKey,
    seller_pubkey: RadrootsPublicKey,
    items: Vec<RadrootsOrderItem>,
    economics: RadrootsOrderEconomics,
}

#[derive(Debug, Clone)]
struct ResolvedOrderSubmitRequest {
    request_event_id: String,
    listing_event_id: Option<String>,
    payload: RadrootsOrderRequest,
}

#[derive(Debug, Clone)]
struct OrderRebindExistingRequestCheck {
    state: String,
    event_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct OrderDraftStatusActorContext {
    source: &'static str,
    buyer_pubkey: Option<String>,
    seller_pubkey: Option<String>,
    selected_account_pubkey: Option<String>,
}

#[derive(Debug, Clone)]
struct OrderEventListActorContext {
    source: &'static str,
    seller_pubkey: String,
}

#[derive(Debug, Clone)]
struct OrderBoundBuyerWriteContext {
    loaded: LoadedOrderDraft,
    account: account::AccountRecordView,
}

#[derive(Debug, Clone)]
struct OrderBuyerWriteActorContext {
    bound: Option<OrderBoundBuyerWriteContext>,
    selected_pubkey: String,
    status_buyer_pubkey: Option<String>,
    status_seller_pubkey: Option<String>,
    status_context_source: &'static str,
}

#[derive(Debug, Clone)]
struct SellerOrderRequestResolution {
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
    fetched_count: usize,
    decoded_count: usize,
    skipped_count: usize,
    requests: Vec<ResolvedSellerOrderRequest>,
    candidate_issues: Vec<OrderIssueView>,
}

pub fn scaffold(
    config: &RuntimeConfig,
    args: &OrderDraftCreateArgs,
) -> Result<OrderNewView, RuntimeError> {
    validate_scaffold_args(args)?;

    let listing_lookup = normalize_optional(args.listing.as_deref());
    let explicit_listing_addr = normalize_optional(args.listing_addr.as_deref());
    let resolved_listing = resolve_order_listing(
        config,
        listing_lookup.as_deref(),
        explicit_listing_addr.as_deref(),
    )?;

    let buyer_actor = resolve_initial_buyer_actor(config)?;
    let buyer_pubkey = buyer_actor.pubkey.clone();

    let listing_addr = resolved_listing
        .as_ref()
        .map(|listing| listing.listing_addr.clone())
        .unwrap_or_default();
    let seller_pubkey = resolved_listing
        .as_ref()
        .map(|listing| listing.seller_pubkey.clone())
        .unwrap_or_default();
    let listing_event_id = resolved_listing
        .as_ref()
        .map(|listing| listing.listing_event_id.clone())
        .unwrap_or_default();
    let listing_relays = resolved_listing
        .as_ref()
        .map(|listing| listing.listing_relays.clone())
        .unwrap_or_default();

    let items = match normalize_optional(args.bin_id.as_deref()) {
        Some(bin_id) => vec![OrderDraftItem {
            bin_id,
            bin_count: args.bin_count.unwrap_or(1),
        }],
        None => Vec::new(),
    };

    let order_id = next_order_id();
    let economics = order_economics_from_resolved_listing(
        order_id.as_str(),
        resolved_listing.as_ref(),
        items.as_slice(),
        args.adjustments.as_slice(),
    )?;
    let drafts_dir = drafts_dir(config);
    fs::create_dir_all(&drafts_dir)?;
    let file = drafts_dir.join(format!("{order_id}.toml"));

    let document = OrderDraftDocument {
        version: 1,
        kind: ORDER_DRAFT_KIND.to_owned(),
        order: OrderDraft {
            order_id: order_id.clone(),
            listing_addr,
            listing_event_id,
            listing_relays,
            buyer_pubkey,
            seller_pubkey,
            items,
            economics,
        },
        buyer_actor,
        listing_lookup,
    };
    save_draft(file.as_path(), &document)?;

    let mut view: OrderNewView = view_from_loaded(
        config,
        LoadedOrderDraft {
            file,
            updated_at_unix: now_unix(),
            document,
        },
    )?
    .into();
    view.actions
        .insert(0, format!("radroots trade get {}", view.order_id));

    Ok(view)
}

pub fn scaffold_preflight(
    config: &RuntimeConfig,
    args: &OrderDraftCreateArgs,
) -> Result<OrderNewView, RuntimeError> {
    validate_scaffold_args(args)?;

    let listing_lookup = normalize_optional(args.listing.as_deref());
    let explicit_listing_addr = normalize_optional(args.listing_addr.as_deref());
    let resolved_listing = resolve_order_listing(
        config,
        listing_lookup.as_deref(),
        explicit_listing_addr.as_deref(),
    )?;

    let buyer_actor = resolve_initial_buyer_actor(config)?;
    let buyer_pubkey = buyer_actor.pubkey.clone();

    let listing_addr = resolved_listing
        .as_ref()
        .map(|listing| listing.listing_addr.clone())
        .unwrap_or_default();
    let seller_pubkey = resolved_listing
        .as_ref()
        .map(|listing| listing.seller_pubkey.clone())
        .unwrap_or_default();
    let listing_event_id = resolved_listing
        .as_ref()
        .map(|listing| listing.listing_event_id.clone())
        .unwrap_or_default();
    let listing_relays = resolved_listing
        .as_ref()
        .map(|listing| listing.listing_relays.clone())
        .unwrap_or_default();

    let items = match normalize_optional(args.bin_id.as_deref()) {
        Some(bin_id) => vec![OrderDraftItem {
            bin_id,
            bin_count: args.bin_count.unwrap_or(1),
        }],
        None => Vec::new(),
    };

    let order_id = next_order_id();
    let economics = order_economics_from_resolved_listing(
        order_id.as_str(),
        resolved_listing.as_ref(),
        items.as_slice(),
        args.adjustments.as_slice(),
    )?;
    let file = drafts_dir(config).join(format!("{order_id}.toml"));
    let document = OrderDraftDocument {
        version: 1,
        kind: ORDER_DRAFT_KIND.to_owned(),
        order: OrderDraft {
            order_id: order_id.clone(),
            listing_addr,
            listing_event_id,
            listing_relays,
            buyer_pubkey,
            seller_pubkey,
            items,
            economics,
        },
        buyer_actor,
        listing_lookup,
    };

    let mut view: OrderNewView = view_from_loaded(
        config,
        LoadedOrderDraft {
            file,
            updated_at_unix: now_unix(),
            document,
        },
    )?
    .into();
    view.state = "dry_run".to_owned();
    view.actions
        .insert(0, format!("radroots trade get {}", view.order_id));

    Ok(view)
}

pub fn get(config: &RuntimeConfig, args: &RecordLookupArgs) -> Result<OrderGetView, RuntimeError> {
    let lookup = args.key.clone();
    let file = draft_lookup_path(config, lookup.as_str());
    if !file.exists() {
        if let Some(app_order) = load_app_order_record_for_lookup(config, lookup.as_str())? {
            return view_from_loaded_with_source_issues(
                config,
                app_order.loaded,
                app_order.source_issues.as_slice(),
            );
        }
        return Ok(OrderGetView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup: lookup.clone(),
            order_id: None,
            file: Some(file.display().to_string()),
            listing_lookup: None,
            listing_addr: None,
            listing_event_id: None,
            listing_relays: Vec::new(),
            buyer_account_id: None,
            buyer_pubkey: None,
            buyer_actor_source: None,
            buyer_custody: None,
            buyer_write_capable: None,
            seller_pubkey: None,
            ready_for_submit: false,
            items: Vec::new(),
            economics: None,
            updated_at_unix: None,
            job: None,
            workflow: None,
            reason: Some(format!("trade draft `{lookup}` was not found")),
            issues: Vec::new(),
            actions: vec![
                "radroots trade list".to_owned(),
                "radroots basket create".to_owned(),
            ],
        });
    }

    match load_draft(file.as_path()) {
        Ok(loaded) => view_from_loaded(config, loaded),
        Err(reason) => Ok(OrderGetView {
            state: "error".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup,
            order_id: None,
            file: Some(file.display().to_string()),
            listing_lookup: None,
            listing_addr: None,
            listing_event_id: None,
            listing_relays: Vec::new(),
            buyer_account_id: None,
            buyer_pubkey: None,
            buyer_actor_source: None,
            buyer_custody: None,
            buyer_write_capable: None,
            seller_pubkey: None,
            ready_for_submit: false,
            items: Vec::new(),
            economics: None,
            updated_at_unix: None,
            job: None,
            workflow: None,
            reason: Some(reason),
            issues: Vec::new(),
            actions: Vec::new(),
        }),
    }
}

pub fn list(config: &RuntimeConfig) -> Result<OrderListView, RuntimeError> {
    let dir = drafts_dir(config);
    let mut orders = Vec::new();
    let mut local_order_ids = HashSet::new();
    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }
            match load_draft(path.as_path()) {
                Ok(loaded) => {
                    local_order_ids.insert(loaded.document.order.order_id.clone());
                    orders.push(summary_from_loaded(config, &loaded)?);
                }
                Err(reason) => orders.push(summary_for_invalid_file(path.as_path(), reason)),
            }
        }
    }
    for entry in current_app_order_record_entries(app_order_local_records(config)?) {
        let app_order = load_app_order_record_from_record(config, entry.record.clone())?;
        if local_order_ids.contains(&app_order.loaded.document.order.order_id) {
            continue;
        }
        orders.push(summary_from_loaded_with_source_issues(
            config,
            &app_order.loaded,
            app_order.source_issues.as_slice(),
        )?);
    }

    orders.sort_by(|left, right| {
        right
            .updated_at_unix
            .cmp(&left.updated_at_unix)
            .then_with(|| left.id.cmp(&right.id))
    });

    let state = if orders.is_empty() {
        "empty"
    } else if orders.iter().any(|order| {
        order.state == "error" || (!order.ready_for_submit && order.state != "submitted")
    }) {
        "degraded"
    } else {
        "ready"
    };
    let actions = if orders.is_empty() {
        vec!["radroots basket create".to_owned()]
    } else {
        Vec::new()
    };

    Ok(OrderListView {
        state: state.to_owned(),
        source: ORDER_SOURCE.to_owned(),
        count: orders.len(),
        orders,
        actions,
    })
}

pub fn app_record_list(config: &RuntimeConfig) -> Result<OrderAppRecordListView, RuntimeError> {
    let database_path = shared_local_events_db_path(config)?;
    let mut entries = current_app_order_record_entries(app_order_local_records(config)?);
    let has_more = entries.len() > ORDER_APP_RECORD_LIST_LIMIT as usize;
    if has_more {
        entries.truncate(ORDER_APP_RECORD_LIST_LIMIT as usize);
    }
    let next_cursor = if has_more {
        entries
            .last()
            .map(|entry| (entry.record.change_seq, entry.record.seq))
    } else {
        None
    };
    let records = entries
        .iter()
        .map(|entry| app_order_record_summary(config, &entry.record, entry.superseded_count))
        .collect::<Result<Vec<_>, _>>()?;
    let state = if records.is_empty() { "empty" } else { "ready" };
    let actions = if records.is_empty() {
        vec!["place a buyer order in radroots_studio_app".to_owned()]
    } else {
        Vec::new()
    };

    Ok(OrderAppRecordListView {
        state: state.to_owned(),
        source: ORDER_APP_RECORD_SOURCE.to_owned(),
        count: records.len(),
        limit: ORDER_APP_RECORD_LIST_LIMIT,
        has_more,
        next_before_change_seq: next_cursor.map(|(change_seq, _)| change_seq),
        next_before_seq: next_cursor.map(|(_, seq)| seq),
        local_events_db: database_path.display().to_string(),
        records,
        actions,
    })
}

pub fn app_record_export(
    config: &RuntimeConfig,
    args: &TradeAppRecordExportArgs,
) -> Result<OrderAppRecordExportView, RuntimeError> {
    let Some(record) = get_shared_record(config, args.record_id.as_str())? else {
        return Ok(OrderAppRecordExportView {
            state: "missing".to_owned(),
            source: ORDER_APP_RECORD_SOURCE.to_owned(),
            record_id: args.record_id.clone(),
            dry_run: config.output.dry_run,
            file: args
                .output
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            valid: false,
            order_id: None,
            listing_addr: None,
            listing_event_id: None,
            listing_relays: Vec::new(),
            buyer_account_id: None,
            buyer_pubkey: None,
            buyer_actor_source: None,
            seller_pubkey: None,
            issues: Vec::new(),
            reason: Some(format!(
                "app-authored local trade record `{}` was not found",
                args.record_id
            )),
            actions: vec!["radroots trade app list".to_owned()],
        });
    };

    let app_order = load_app_order_record_from_record(config, record)?;
    let mut issues = source_and_document_issues(config, &app_order)?;
    if !issues.is_empty() {
        let state = app_order_export_failure_state(issues.as_slice());
        let actions = app_order_export_failure_actions(&app_order.loaded.document, &issues);
        return Ok(OrderAppRecordExportView {
            state: state.to_owned(),
            source: ORDER_APP_RECORD_SOURCE.to_owned(),
            record_id: args.record_id.clone(),
            dry_run: config.output.dry_run,
            file: args
                .output
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            valid: false,
            order_id: Some(app_order.loaded.document.order.order_id.clone()),
            listing_addr: non_empty_string(app_order.loaded.document.order.listing_addr.clone()),
            listing_event_id: non_empty_string(
                app_order.loaded.document.order.listing_event_id.clone(),
            ),
            listing_relays: order_listing_relays(&app_order.loaded.document),
            buyer_account_id: buyer_account_id(&app_order.loaded.document),
            buyer_pubkey: non_empty_string(app_order.loaded.document.order.buyer_pubkey.clone()),
            buyer_actor_source: buyer_actor_source(&app_order.loaded.document),
            seller_pubkey: non_empty_string(app_order.loaded.document.order.seller_pubkey.clone()),
            issues,
            reason: Some(format!(
                "app-authored local trade record `{}` is not ready as a CLI trade draft",
                args.record_id
            )),
            actions,
        });
    }

    let output_path = order_export_output_path(
        config,
        args.output.as_ref(),
        app_order.loaded.document.order.order_id.as_str(),
    );
    validate_order_export_output_target(output_path.as_path())?;
    if !config.output.dry_run {
        save_draft(output_path.as_path(), &app_order.loaded.document)?;
    }
    issues.clear();

    Ok(OrderAppRecordExportView {
        state: if config.output.dry_run {
            "dry_run"
        } else {
            "exported"
        }
        .to_owned(),
        source: ORDER_APP_RECORD_SOURCE.to_owned(),
        record_id: args.record_id.clone(),
        dry_run: config.output.dry_run,
        file: output_path.display().to_string(),
        valid: true,
        order_id: Some(app_order.loaded.document.order.order_id.clone()),
        listing_addr: non_empty_string(app_order.loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(
            app_order.loaded.document.order.listing_event_id.clone(),
        ),
        listing_relays: order_listing_relays(&app_order.loaded.document),
        buyer_account_id: buyer_account_id(&app_order.loaded.document),
        buyer_pubkey: non_empty_string(app_order.loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&app_order.loaded.document),
        seller_pubkey: non_empty_string(app_order.loaded.document.order.seller_pubkey.clone()),
        issues,
        reason: Some(if config.output.dry_run {
            "dry run requested; trade draft was not written".to_owned()
        } else {
            "app-authored local trade record exported as a CLI trade draft".to_owned()
        }),
        actions: vec![
            format!(
                "radroots trade get {}",
                app_order.loaded.document.order.order_id
            ),
            format!(
                "radroots --relay wss://relay.example.com trade submit {}",
                app_order.loaded.document.order.order_id
            ),
        ],
    })
}

pub fn submit(
    config: &RuntimeConfig,
    args: &TradeSubmitArgs,
) -> Result<OrderSubmitView, CliSdkAdapterError> {
    let file = draft_lookup_path(config, args.key.as_str());
    let (loaded, source_issues) = if file.exists() {
        match load_draft(file.as_path()) {
            Ok(loaded) => (loaded, Vec::new()),
            Err(reason) => {
                return Ok(OrderSubmitView {
                    state: "error".to_owned(),
                    source: ORDER_SOURCE.to_owned(),
                    order_id: args.key.clone(),
                    locator: order_locator_view_from_key(args.key.as_str()),
                    file: file.display().to_string(),
                    listing_lookup: None,
                    listing_addr: None,
                    listing_event_id: None,
                    listing_relays: Vec::new(),
                    buyer_account_id: None,
                    buyer_pubkey: None,
                    buyer_actor_source: None,
                    buyer_custody: None,
                    buyer_write_capable: None,
                    seller_pubkey: None,
                    event_id: None,
                    event_kind: None,
                    dry_run: config.output.dry_run,
                    deduplicated: false,
                    target_relays: Vec::new(),
                    connected_relays: Vec::new(),
                    acknowledged_relays: Vec::new(),
                    failed_relays: Vec::new(),
                    idempotency_key: args.idempotency_key.clone(),
                    signer_mode: None,
                    reason: Some(reason),
                    job: None,
                    issues: Vec::new(),
                    actions: Vec::new(),
                });
            }
        }
    } else if let Some(app_order) = load_app_order_record_for_lookup(config, args.key.as_str())? {
        (app_order.loaded, app_order.source_issues)
    } else {
        return Ok(OrderSubmitView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            order_id: args.key.clone(),
            locator: order_locator_view_from_key(args.key.as_str()),
            file: file.display().to_string(),
            listing_lookup: None,
            listing_addr: None,
            listing_event_id: None,
            listing_relays: Vec::new(),
            buyer_account_id: None,
            buyer_pubkey: None,
            buyer_actor_source: None,
            buyer_custody: None,
            buyer_write_capable: None,
            seller_pubkey: None,
            event_id: None,
            event_kind: None,
            dry_run: config.output.dry_run,
            deduplicated: false,
            target_relays: Vec::new(),
            connected_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            reason: Some(format!("trade draft `{}` was not found", args.key)),
            job: None,
            issues: Vec::new(),
            actions: vec![
                "radroots trade list".to_owned(),
                "radroots basket create".to_owned(),
            ],
        });
    };

    let mut issues = collect_issues(&loaded.document);
    issues.extend(source_issues.clone());
    if let Some(view) = order_submit_app_signed_evidence_view(config, &loaded, args, &issues) {
        return Ok(view);
    }
    if !issues.is_empty() {
        let mut actions = actions_for_document(&loaded.document, loaded.file.as_path(), &issues);
        actions.push(format!(
            "radroots trade get {}",
            loaded.document.order.order_id
        ));
        return Ok(OrderSubmitView {
            state: "unconfigured".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
            listing_relays: order_listing_relays(&loaded.document),
            buyer_account_id: buyer_account_id(&loaded.document),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            buyer_actor_source: buyer_actor_source(&loaded.document),
            buyer_custody: None,
            buyer_write_capable: None,
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            event_id: None,
            event_kind: None,
            dry_run: config.output.dry_run,
            deduplicated: false,
            target_relays: Vec::new(),
            connected_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            reason: Some("trade draft is not ready for submit".to_owned()),
            job: None,
            issues,
            actions,
        });
    }

    let account = validate_bound_order_buyer_account(config, &loaded)?;
    let payload = canonical_order_request_payload_from_loaded(
        &loaded,
        account.record.public_identity.public_key_hex.as_str(),
    )?;

    propose_trade_via_sdk(config, &loaded, args, &account, payload)
}

pub fn rebind(
    config: &RuntimeConfig,
    args: &TradeRebindArgs,
) -> Result<OrderRebindView, RuntimeError> {
    rebind_inner(config, args, false)
}

pub fn rebind_preflight(
    config: &RuntimeConfig,
    args: &TradeRebindArgs,
) -> Result<OrderRebindView, RuntimeError> {
    rebind_inner(config, args, true)
}

fn rebind_inner(
    config: &RuntimeConfig,
    args: &TradeRebindArgs,
    dry_run: bool,
) -> Result<OrderRebindView, RuntimeError> {
    let file = draft_lookup_path(config, args.key.as_str());
    if !file.exists() {
        return Ok(OrderRebindView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup: args.key.clone(),
            file: file.display().to_string(),
            dry_run,
            from_order_id: args.key.clone(),
            to_order_id: args.key.clone(),
            order_id_changed: false,
            from_buyer_account_id: None,
            from_buyer_pubkey: None,
            from_buyer_actor_source: None,
            to_buyer_account_id: args.selector.clone(),
            to_buyer_pubkey: String::new(),
            to_buyer_actor_source: ORDER_BUYER_ACTOR_SOURCE_REBIND.to_owned(),
            buyer_pubkey_changed: false,
            existing_request_check: "not_checked".to_owned(),
            existing_request_event_ids: Vec::new(),
            reason: Some(format!("trade draft `{}` was not found", args.key)),
            actions: vec![
                "radroots trade list".to_owned(),
                "radroots basket create".to_owned(),
            ],
        });
    }

    let loaded = load_draft(file.as_path()).map_err(RuntimeError::Config)?;
    let target_account = account::resolve_account_selector(config, args.selector.as_str())
        .map_err(|error| order_rebind_selector_error(args.selector.as_str(), error))?;
    let existing_request = order_rebind_existing_request_check(config, &loaded)?;
    let from_order_id = loaded.document.order.order_id.clone();
    let from_buyer_account_id = buyer_account_id(&loaded.document);
    let from_buyer_pubkey = non_empty_string(loaded.document.buyer_actor.pubkey.clone());
    let from_buyer_actor_source = buyer_actor_source(&loaded.document);
    let target_account_id = target_account.record.account_id.to_string();
    let target_pubkey = target_account.record.public_identity.public_key_hex.clone();
    let current_buyer_pubkey = from_buyer_pubkey
        .clone()
        .or_else(|| non_empty_string(loaded.document.order.buyer_pubkey.clone()));
    let buyer_pubkey_changed = current_buyer_pubkey
        .as_deref()
        .is_none_or(|pubkey| !pubkey.eq_ignore_ascii_case(target_pubkey.as_str()));

    if !existing_request.event_ids.is_empty() {
        return Ok(OrderRebindView {
            state: "invalid".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup: args.key.clone(),
            file: loaded.file.display().to_string(),
            dry_run,
            from_order_id: from_order_id.clone(),
            to_order_id: from_order_id.clone(),
            order_id_changed: false,
            from_buyer_account_id,
            from_buyer_pubkey,
            from_buyer_actor_source,
            to_buyer_account_id: target_account_id,
            to_buyer_pubkey: target_pubkey,
            to_buyer_actor_source: ORDER_BUYER_ACTOR_SOURCE_REBIND.to_owned(),
            buyer_pubkey_changed,
            existing_request_check: existing_request.state,
            existing_request_event_ids: existing_request.event_ids,
            reason: Some(
                "order rebind refused because a valid order request is already visible for this order id"
                    .to_owned(),
            ),
            actions: vec![
                format!("radroots trade status get {from_order_id}"),
                "radroots basket quote create <basket-id>".to_owned(),
            ],
        });
    }

    let mut document = loaded.document.clone();
    let to_order_id = if buyer_pubkey_changed {
        next_order_id()
    } else {
        from_order_id.clone()
    };
    let order_id_changed = to_order_id != from_order_id;
    document.order.order_id = to_order_id.clone();
    document.order.buyer_pubkey = target_pubkey.clone();
    document.buyer_actor.account_id = target_account_id.clone();
    document.buyer_actor.pubkey = target_pubkey.clone();
    document.buyer_actor.source = ORDER_BUYER_ACTOR_SOURCE_REBIND.to_owned();
    if order_id_changed && let Some(economics) = document.order.economics.as_mut() {
        economics.quote_id =
            protocol_quote_id(format!("quote_{to_order_id}").as_str(), "quote_id")?;
    }

    let output_file = if order_id_changed {
        drafts_dir(config).join(format!("{to_order_id}.toml"))
    } else {
        loaded.file.clone()
    };
    if !dry_run {
        if order_id_changed && output_file.exists() {
            return Err(RuntimeError::Config(format!(
                "order rebind target file {} already exists",
                output_file.display()
            )));
        }
        save_draft(output_file.as_path(), &document)?;
        if order_id_changed && output_file != loaded.file {
            fs::remove_file(loaded.file.as_path())?;
        }
    }

    Ok(OrderRebindView {
        state: if dry_run { "dry_run" } else { "rebound" }.to_owned(),
        source: ORDER_SOURCE.to_owned(),
        lookup: args.key.clone(),
        file: output_file.display().to_string(),
        dry_run,
        from_order_id: from_order_id.clone(),
        to_order_id: to_order_id.clone(),
        order_id_changed,
        from_buyer_account_id,
        from_buyer_pubkey,
        from_buyer_actor_source,
        to_buyer_account_id: target_account_id,
        to_buyer_pubkey: target_pubkey,
        to_buyer_actor_source: ORDER_BUYER_ACTOR_SOURCE_REBIND.to_owned(),
        buyer_pubkey_changed,
        existing_request_check: existing_request.state,
        existing_request_event_ids: Vec::new(),
        reason: Some(if dry_run {
            "dry run requested; order buyer actor binding was not written".to_owned()
        } else {
            "order buyer actor binding updated".to_owned()
        }),
        actions: if dry_run {
            vec![format!(
                "radroots --approval-token approve order rebind {} {}",
                args.key, args.selector
            )]
        } else {
            vec![format!("radroots trade get {to_order_id}")]
        },
    })
}

pub fn event_list(
    config: &RuntimeConfig,
    order_id: Option<&str>,
) -> Result<OrderEventListView, RuntimeError> {
    if config.relay.urls.is_empty() {
        return Ok(order_event_list_unconfigured(
            None,
            ORDER_ACTOR_CONTEXT_NETWORK_ONLY,
            "trade event list requires at least one configured relay".to_owned(),
            Vec::new(),
            vec![ORDER_EVENT_LIST_RELAY_ACTION.to_owned()],
        ));
    }

    let actor_context = match order_event_list_actor_context(config, order_id)? {
        Some(context) => context,
        None => {
            return Ok(order_event_list_unconfigured(
                None,
                ORDER_ACTOR_CONTEXT_NETWORK_ONLY,
                "trade event list requires a selected seller account".to_owned(),
                config.relay.urls.clone(),
                vec!["radroots account create".to_owned()],
            ));
        }
    };
    let seller_pubkey = actor_context.seller_pubkey;
    let filter = order_request_filter(seller_pubkey.as_str(), order_id)?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            return Ok(order_event_list_unavailable(
                seller_pubkey,
                actor_context.source,
                reason,
                target_relays,
                failed_relays,
            ));
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    Ok(order_event_list_from_receipt(
        seller_pubkey,
        order_id,
        actor_context.source,
        receipt,
    ))
}

pub fn decide(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
) -> Result<OrderDecisionView, CliSdkAdapterError> {
    let seller = match account::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_decision_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason = Some(format!(
                "order {} requires a selected seller account",
                args.decision.command()
            ));
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    decide_trade_via_sdk(config, args, &seller)
}

pub fn revision_propose(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
) -> Result<OrderRevisionProposalView, CliSdkAdapterError> {
    if let Some(view) = order_revision_args_preflight_view(config, args) {
        return Ok(view);
    }
    let seller = match account::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_revision_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason =
                Some("trade revision propose requires a selected seller account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    propose_revision_via_sdk(config, args, &seller)
}

pub fn revision_decide(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
) -> Result<OrderRevisionDecisionView, CliSdkAdapterError> {
    if let Some(view) = order_revision_decision_args_preflight_view(config, args) {
        return Ok(view);
    }
    let actor_context = match order_buyer_write_actor_context(config, args.key.as_str())? {
        Some(context) => context,
        None => {
            let mut view = order_revision_decision_base_view(
                config,
                args,
                "unconfigured",
                config.output.dry_run,
            );
            view.reason =
                Some("trade revision decision requires a selected buyer account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    decide_revision_via_sdk(config, args, actor_context)
}

pub fn cancel(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
) -> Result<OrderCancellationView, CliSdkAdapterError> {
    let actor_context = match order_buyer_write_actor_context(config, args.key.as_str())? {
        Some(context) => context,
        None => {
            let mut view =
                order_cancellation_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason = Some("trade cancel requires a selected buyer account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    cancel_trade_via_sdk(config, args, actor_context)
}

pub fn status(
    config: &RuntimeConfig,
    args: &TradeStatusArgs,
) -> Result<OrderStatusView, CliSdkAdapterError> {
    let request = TradeStatusRequest::parse(args.key.as_str())?;
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().trades().status_client().status(request))?;
    Ok(sdk_order_status_view(receipt))
}

fn decide_trade_via_sdk(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    seller: &account::AccountRecordView,
) -> Result<OrderDecisionView, CliSdkAdapterError> {
    let actor = sdk_trade_actor(seller, RadrootsActorRole::Seller, args.decision.command())?;
    let session = connect_sdk_for_trade_actor(config, seller, "trade decision")?;
    let locator = trade_locator_from_key(args.key.as_str())?;
    let status = trade_status_for_locator(&session, locator.clone())?;
    let status_view = sdk_order_status_view(status.clone());
    let publish_mode = trade_publish_mode(config);
    let ack_policy = trade_ack_policy(publish_mode)?;
    let outcome = match args.decision {
        TradeDecisionArg::Accept => {
            let commitments = inventory_commitments_from_status(&status)?;
            let mut request = TradeAcceptRequest::new(
                actor,
                locator,
                commitments,
                trade_relay_resolution_policy(),
                publish_mode,
                ack_policy,
            )
            .with_privacy_confirmation(trade_privacy_confirmation());
            if let Some(idempotency_key) = args.idempotency_key.as_deref() {
                request = request.try_with_idempotency_key(idempotency_key)?;
            }
            session.block_on(session.sdk().trades().seller().accept_trade(request))?
        }
        TradeDecisionArg::Decline => {
            let reason = args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| {
                    RuntimeError::Config("trade decline requires a non-empty reason".to_owned())
                })?;
            let mut request = TradeDeclineRequest::new(
                actor,
                locator,
                reason,
                trade_relay_resolution_policy(),
                publish_mode,
                ack_policy,
            )
            .with_privacy_confirmation(trade_privacy_confirmation());
            if let Some(idempotency_key) = args.idempotency_key.as_deref() {
                request = request.try_with_idempotency_key(idempotency_key)?;
            }
            session.block_on(session.sdk().trades().seller().decline_trade(request))?
        }
    };
    Ok(sdk_trade_decision_outcome_view(
        config,
        args,
        &status_view,
        outcome,
    ))
}

fn propose_revision_via_sdk(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    seller: &account::AccountRecordView,
) -> Result<OrderRevisionProposalView, CliSdkAdapterError> {
    let actor = sdk_trade_actor(seller, RadrootsActorRole::Seller, "revision propose")?;
    let session = connect_sdk_for_trade_actor(config, seller, "trade revision propose")?;
    let locator = trade_locator_from_key(args.key.as_str())?;
    let status = trade_status_for_locator(&session, locator.clone())?;
    let status_view = sdk_order_status_view(status);
    let revision = revision_request_parts_from_status(args, &status_view)?;
    let publish_mode = trade_publish_mode(config);
    let ack_policy = trade_ack_policy(publish_mode)?;
    let mut request = TradeRevisionProposalRequest::new(
        actor,
        locator,
        revision.revision_id.clone(),
        revision.items.clone(),
        revision.economics.clone(),
        args.reason.trim(),
        trade_relay_resolution_policy(),
        publish_mode,
        ack_policy,
    )
    .with_privacy_confirmation(trade_privacy_confirmation());
    if let Some(idempotency_key) = args.idempotency_key.as_deref() {
        request = request.try_with_idempotency_key(idempotency_key)?;
    }
    let outcome = session.block_on(session.sdk().trades().seller().propose_revision(request))?;
    Ok(sdk_trade_revision_outcome_view(
        config,
        args,
        &status_view,
        revision,
        outcome,
    ))
}

fn decide_revision_via_sdk(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    actor_context: OrderBuyerWriteActorContext,
) -> Result<OrderRevisionDecisionView, CliSdkAdapterError> {
    let account = match actor_context.bound {
        Some(bound) => bound.account,
        None => account::resolve_account(config)?.ok_or_else(|| {
            RuntimeError::Config(
                "trade revision decision requires a selected buyer account".to_owned(),
            )
        })?,
    };
    let actor = sdk_trade_actor(&account, RadrootsActorRole::Buyer, "revision decision")?;
    let session = connect_sdk_for_trade_actor(config, &account, "trade revision decision")?;
    let locator = trade_locator_from_key(args.key.as_str())?;
    let status = trade_status_for_locator(&session, locator.clone())?;
    let status_view = sdk_order_status_view(status);
    let revision_id = protocol_revision_id(args.revision_id.trim(), "revision_id")?;
    let decision = match args.decision {
        TradeRevisionDecisionArg::Accept => RadrootsOrderRevisionOutcome::Accepted,
        TradeRevisionDecisionArg::Decline => RadrootsOrderRevisionOutcome::Declined {
            reason: args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| {
                    RuntimeError::Config(
                        "trade revision decline requires a non-empty reason".to_owned(),
                    )
                })?
                .to_owned(),
        },
    };
    let publish_mode = trade_publish_mode(config);
    let ack_policy = trade_ack_policy(publish_mode)?;
    let mut request = TradeRevisionDecisionRequest::new(
        actor,
        locator,
        revision_id,
        decision,
        trade_relay_resolution_policy(),
        publish_mode,
        ack_policy,
    )
    .with_privacy_confirmation(trade_privacy_confirmation());
    if let Some(idempotency_key) = args.idempotency_key.as_deref() {
        request = request.try_with_idempotency_key(idempotency_key)?;
    }
    let outcome = match args.decision {
        TradeRevisionDecisionArg::Accept => {
            session.block_on(session.sdk().trades().buyer().accept_revision(request))?
        }
        TradeRevisionDecisionArg::Decline => {
            session.block_on(session.sdk().trades().buyer().decline_revision(request))?
        }
    };
    Ok(sdk_trade_revision_decision_outcome_view(
        config,
        args,
        &status_view,
        outcome,
    ))
}

fn cancel_trade_via_sdk(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    actor_context: OrderBuyerWriteActorContext,
) -> Result<OrderCancellationView, CliSdkAdapterError> {
    let account = match actor_context.bound {
        Some(bound) => bound.account,
        None => account::resolve_account(config)?.ok_or_else(|| {
            RuntimeError::Config("trade cancel requires a selected buyer account".to_owned())
        })?,
    };
    let actor = sdk_trade_actor(&account, RadrootsActorRole::Buyer, "cancel")?;
    let session = connect_sdk_for_trade_actor(config, &account, "trade cancel")?;
    let locator = trade_locator_from_key(args.key.as_str())?;
    let status = trade_status_for_locator(&session, locator.clone())?;
    let status_view = sdk_order_status_view(status);
    let publish_mode = trade_publish_mode(config);
    let ack_policy = trade_ack_policy(publish_mode)?;
    let mut request = TradeCancelRequest::new(
        actor,
        locator,
        args.reason.trim(),
        trade_relay_resolution_policy(),
        publish_mode,
        ack_policy,
    )
    .with_privacy_confirmation(trade_privacy_confirmation());
    if let Some(idempotency_key) = args.idempotency_key.as_deref() {
        request = request.try_with_idempotency_key(idempotency_key)?;
    }
    let outcome = session.block_on(session.sdk().trades().buyer().cancel_trade(request))?;
    Ok(sdk_trade_cancellation_outcome_view(
        config,
        args,
        &status_view,
        outcome,
    ))
}

fn trade_status_for_locator(
    session: &CliSdkSession,
    locator: RadrootsTradeLocator,
) -> Result<TradeStatusReceipt, CliSdkAdapterError> {
    Ok(session.block_on(
        session
            .sdk()
            .trades()
            .status_client()
            .status(TradeStatusRequest::new(locator)),
    )?)
}

fn inventory_commitments_from_status(
    status: &TradeStatusReceipt,
) -> Result<Vec<RadrootsOrderInventoryCommitment>, RuntimeError> {
    let economics = status.economics.as_ref().ok_or_else(|| {
        RuntimeError::Config("trade accept requires SDK status economics evidence".to_owned())
    })?;
    Ok(economics
        .items
        .iter()
        .map(|item| RadrootsOrderInventoryCommitment {
            bin_id: item.bin_id.clone(),
            bin_count: item.bin_count,
        })
        .collect())
}

#[derive(Debug, Clone)]
struct SdkRevisionRequestParts {
    revision_id: RadrootsOrderRevisionId,
    items: Vec<RadrootsOrderItem>,
    economics: RadrootsOrderEconomics,
}

fn revision_request_parts_from_status(
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
) -> Result<SdkRevisionRequestParts, RuntimeError> {
    let revision_id = protocol_revision_id(next_revision_id().as_str(), "revision_id")?;
    let economics = status.economics.clone().ok_or_else(|| {
        RuntimeError::Config("accepted trade is missing current agreement economics".to_owned())
    })?;
    let economics = revised_order_economics(args, revision_id.as_str(), &economics)?;
    let items = economics
        .items
        .iter()
        .map(|item| RadrootsOrderItem {
            bin_id: item.bin_id.clone(),
            bin_count: item.bin_count,
        })
        .collect::<Vec<_>>();
    Ok(SdkRevisionRequestParts {
        revision_id,
        items,
        economics,
    })
}

fn sdk_trade_decision_outcome_view(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    status: &OrderStatusView,
    outcome: TradeMutationOutcome<TradeDecisionPlan, TradeDecisionReceipt>,
) -> OrderDecisionView {
    match outcome {
        TradeMutationOutcome::DryRun { plan } => {
            let mut view = order_decision_base_view(config, args, "dry_run", true);
            apply_order_decision_status(&mut view, status);
            view.request_event_id = Some(plan.request_event_id.to_string());
            view.root_event_id = Some(plan.request_event_id.to_string());
            view.prev_event_id = Some(plan.request_event_id.to_string());
            view.event_id = Some(plan.expected_event_id.to_string());
            view.event_kind = Some(KIND_ORDER_DECISION);
            view.target_relays = config.relay.urls.clone();
            view.reason = Some(format!(
                "dry run requested; seller trade {} publication skipped",
                args.decision.command()
            ));
            view.actions = vec![format!("radroots trade status get {}", status.order_id)];
            view
        }
        TradeMutationOutcome::Enqueued { receipt } => {
            sdk_enqueued_order_decision_view(config, args, status, receipt, None)
        }
        TradeMutationOutcome::Published { receipt, publish } => {
            sdk_enqueued_order_decision_view(config, args, status, receipt, Some(&publish))
        }
    }
}

fn sdk_trade_revision_outcome_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    revision: SdkRevisionRequestParts,
    outcome: TradeMutationOutcome<TradeRevisionProposalPlan, TradeRevisionProposalReceipt>,
) -> OrderRevisionProposalView {
    match outcome {
        TradeMutationOutcome::DryRun { plan } => {
            let mut view = order_revision_base_view(config, args, "dry_run", true);
            apply_order_revision_status(&mut view, status);
            view.revision_id = Some(revision.revision_id.to_string());
            view.root_event_id = Some(plan.root_event_id.to_string());
            view.prev_event_id = Some(plan.previous_event_id.to_string());
            view.event_id = Some(plan.expected_event_id.to_string());
            view.event_kind = Some(KIND_ORDER_REVISION_PROPOSAL);
            view.items = revision
                .items
                .iter()
                .map(|item| OrderDraftItemView {
                    bin_id: item.bin_id.to_string(),
                    bin_count: item.bin_count,
                })
                .collect();
            view.economics = Some(revision.economics);
            view.target_relays = config.relay.urls.clone();
            view.reason =
                Some("dry run requested; seller revision proposal publication skipped".to_owned());
            view.actions = vec![format!("radroots trade status get {}", status.order_id)];
            view
        }
        TradeMutationOutcome::Enqueued { receipt } => {
            sdk_enqueued_order_revision_view(config, args, status, revision, receipt, None)
        }
        TradeMutationOutcome::Published { receipt, publish } => sdk_enqueued_order_revision_view(
            config,
            args,
            status,
            revision,
            receipt,
            Some(&publish),
        ),
    }
}

fn sdk_trade_revision_decision_outcome_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    status: &OrderStatusView,
    outcome: TradeMutationOutcome<TradeRevisionDecisionPlan, TradeRevisionDecisionReceipt>,
) -> OrderRevisionDecisionView {
    match outcome {
        TradeMutationOutcome::DryRun { plan } => {
            let mut view = order_revision_decision_base_view(config, args, "dry_run", true);
            apply_order_revision_decision_status(&mut view, status);
            view.revision_id = Some(args.revision_id.trim().to_owned());
            view.root_event_id = Some(plan.root_event_id.to_string());
            view.prev_event_id = Some(plan.previous_event_id.to_string());
            view.event_id = Some(plan.expected_event_id.to_string());
            view.event_kind = Some(KIND_ORDER_REVISION_DECISION);
            view.target_relays = config.relay.urls.clone();
            view.reason = Some(format!(
                "dry run requested; buyer revision {} publication skipped",
                args.decision.command()
            ));
            view.actions = vec![format!("radroots trade status get {}", status.order_id)];
            view
        }
        TradeMutationOutcome::Enqueued { receipt } => {
            sdk_enqueued_order_revision_decision_view(config, args, status, receipt, None)
        }
        TradeMutationOutcome::Published { receipt, publish } => {
            sdk_enqueued_order_revision_decision_view(config, args, status, receipt, Some(&publish))
        }
    }
}

fn sdk_trade_cancellation_outcome_view(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    status: &OrderStatusView,
    outcome: TradeMutationOutcome<TradeCancellationPlan, TradeCancellationReceipt>,
) -> OrderCancellationView {
    match outcome {
        TradeMutationOutcome::DryRun { plan } => {
            let mut view = order_cancellation_base_view(config, args, "dry_run", true);
            apply_order_cancellation_status(&mut view, status);
            view.root_event_id = Some(plan.root_event_id.to_string());
            view.prev_event_id = Some(plan.previous_event_id.to_string());
            view.event_id = Some(plan.expected_event_id.to_string());
            view.event_kind = Some(KIND_ORDER_CANCELLATION);
            view.target_relays = config.relay.urls.clone();
            view.reason =
                Some("dry run requested; buyer trade cancellation publication skipped".to_owned());
            view.actions = vec![format!("radroots trade status get {}", status.order_id)];
            view
        }
        TradeMutationOutcome::Enqueued { receipt } => {
            sdk_enqueued_order_cancellation_view(config, args, status, receipt, None)
        }
        TradeMutationOutcome::Published { receipt, publish } => {
            sdk_enqueued_order_cancellation_view(config, args, status, receipt, Some(&publish))
        }
    }
}

fn sdk_order_status_from_relay_receipt(
    config: &RuntimeConfig,
    order_id: &str,
    actor_context_source: &'static str,
    receipt: DirectRelayFetchReceipt,
) -> Result<OrderStatusView, CliSdkAdapterError> {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        events,
    } = receipt;
    let fetched_count = events.len();
    let mut decoded_count = 0usize;
    let mut skipped_count = 0usize;
    let mut issues = Vec::new();
    let session = CliSdkSession::connect(config)?;

    for event in events {
        let event_id = event.id.to_string();
        let event = radroots_event_from_nostr(&event);
        match session.block_on(
            session
                .sdk()
                .trades()
                .ingest_evidence(TradeEvidenceIngestRequest::new(event)),
        ) {
            Ok(_) => decoded_count += 1,
            Err(error) => {
                skipped_count += 1;
                issues.push(issue_with_events(
                    "invalid_trade_evidence",
                    "event_id",
                    format!("trade evidence event `{event_id}` failed SDK ingest: {error}"),
                    vec![event_id],
                ));
            }
        }
    }

    let request = TradeStatusRequest::parse(order_id)?;
    let receipt = session.block_on(session.sdk().trades().status_client().status(request))?;
    let mut view = sdk_order_status_view(receipt);
    view.actor_context_source = actor_context_source.to_owned();
    view.target_relays = target_relays;
    view.connected_relays = connected_relays;
    view.failed_relays = relay_failures(failed_relays);
    view.fetched_count = fetched_count;
    view.decoded_count = decoded_count;
    view.skipped_count = skipped_count;
    if !issues.is_empty() {
        view.state = "invalid".to_owned();
        view.reason = Some(format!(
            "relay trade evidence for `{order_id}` failed SDK ingest"
        ));
        view.reducer_issues.extend(issues);
    }
    Ok(view)
}

enum OrderStatusRecord {
    Request {
        listing_event_id: Option<String>,
        record: RadrootsOrderRequestRecord,
    },
    Decision(RadrootsOrderDecisionRecord),
    RevisionProposal(OrderRevisionProposalRecord),
    RevisionDecision(OrderRevisionDecisionRecord),
    Cancellation(RadrootsOrderCancellationRecord),
}

type OrderRevisionProposalRecord = RadrootsOrderRevisionProposalRecord;
type OrderRevisionDecisionRecord = RadrootsOrderRevisionDecisionRecord;

#[derive(Debug, Clone)]
struct OrderRevisionProposalCandidates {
    records: Vec<OrderRevisionProposalRecord>,
    issues: Vec<OrderIssueView>,
}

#[derive(Debug, Clone, Copy)]
struct OrderRequestCandidateContext<'a> {
    order_id: &'a str,
    seller_pubkey: Option<&'a str>,
}

fn order_request_candidate_matches(
    event: &RadrootsNostrEvent,
    context: OrderRequestCandidateContext<'_>,
) -> bool {
    if event_kind_u32(event) != KIND_ORDER_REQUEST
        || !event_matches_tag_value(event, "d", context.order_id)
    {
        return false;
    }
    context
        .seller_pubkey
        .is_none_or(|seller_pubkey| event_matches_tag_value(event, "p", seller_pubkey))
}

fn order_status_record_from_event(
    event: &RadrootsNostrEvent,
) -> Result<OrderStatusRecord, RuntimeError> {
    match event_kind_u32(event) {
        KIND_ORDER_REQUEST => {
            let event = radroots_event_from_nostr(event);
            let event_id = protocol_event_id(event.id.as_str(), "request_event_id")?;
            let author_pubkey = protocol_pubkey(event.author.as_str(), "request_author_pubkey")?;
            let envelope =
                order_envelope_from_event::<RadrootsOrderRequest>(&event).map_err(|error| {
                    RuntimeError::Config(format!("decode active order request event: {error}"))
                })?;
            if envelope.message_type != RadrootsOrderEventType::OrderRequested {
                return Err(RuntimeError::Config(
                    "active order request event used the wrong message type".to_owned(),
                ));
            }
            let context =
                order_event_context_from_tags(RadrootsOrderEventType::OrderRequested, &event.tags)
                    .map_err(|error| {
                        RuntimeError::Config(format!("decode active order request tags: {error}"))
                    })?;
            if context.counterparty_pubkey != envelope.payload.seller_pubkey {
                return Err(RuntimeError::Config(
                    "active order request p tag does not match seller_pubkey".to_owned(),
                ));
            }
            let listing_addr =
                parse_listing_addr(envelope.payload.listing_addr.as_str()).map_err(|error| {
                    RuntimeError::Config(format!(
                        "active order request listing_addr is invalid: {error}"
                    ))
                })?;
            if listing_addr.seller_pubkey != envelope.payload.seller_pubkey.to_string() {
                return Err(RuntimeError::Config(
                    "active order request listing_addr is outside seller authority".to_owned(),
                ));
            }
            Ok(OrderStatusRecord::Request {
                listing_event_id: context.listing_event.as_ref().map(|event| event.id.clone()),
                record: RadrootsOrderRequestRecord {
                    event_id,
                    author_pubkey,
                    payload: envelope.payload,
                },
            })
        }
        KIND_ORDER_DECISION => {
            let event = radroots_event_from_nostr(event);
            let event_id = protocol_event_id(event.id.as_str(), "decision_event_id")?;
            let author_pubkey = protocol_pubkey(event.author.as_str(), "decision_author_pubkey")?;
            let envelope =
                order_envelope_from_event::<RadrootsOrderDecision>(&event).map_err(|error| {
                    RuntimeError::Config(format!("decode active order decision event: {error}"))
                })?;
            if envelope.message_type != RadrootsOrderEventType::OrderDecision {
                return Err(RuntimeError::Config(
                    "active order decision event used the wrong message type".to_owned(),
                ));
            }
            let context =
                order_event_context_from_tags(RadrootsOrderEventType::OrderDecision, &event.tags)
                    .map_err(|error| {
                    RuntimeError::Config(format!("decode active order decision tags: {error}"))
                })?;
            Ok(OrderStatusRecord::Decision(RadrootsOrderDecisionRecord {
                event_id,
                author_pubkey,
                counterparty_pubkey: context.counterparty_pubkey,
                root_event_id: required_order_context_event_id(
                    context.root_event_id,
                    "e_root",
                    "active order decision",
                )?,
                prev_event_id: required_order_context_event_id(
                    context.prev_event_id,
                    "e_prev",
                    "active order decision",
                )?,
                payload: envelope.payload,
            }))
        }
        KIND_ORDER_REVISION_PROPOSAL => {
            let event = radroots_event_from_nostr(event);
            let event_id = protocol_event_id(event.id.as_str(), "revision_event_id")?;
            let author_pubkey = protocol_pubkey(event.author.as_str(), "revision_author_pubkey")?;
            let envelope = order_revision_proposal_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!(
                    "decode active trade revision proposal event: {error}"
                ))
            })?;
            let context = order_event_context_from_tags(
                RadrootsOrderEventType::OrderRevisionProposed,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!(
                    "decode active trade revision proposal tags: {error}"
                ))
            })?;
            Ok(OrderStatusRecord::RevisionProposal(
                RadrootsOrderRevisionProposalRecord {
                    event_id,
                    author_pubkey,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: required_order_context_event_id(
                        context.root_event_id,
                        "e_root",
                        "active trade revision proposal",
                    )?,
                    prev_event_id: required_order_context_event_id(
                        context.prev_event_id,
                        "e_prev",
                        "active trade revision proposal",
                    )?,
                    payload: envelope.payload,
                },
            ))
        }
        KIND_ORDER_REVISION_DECISION => {
            let event = radroots_event_from_nostr(event);
            let event_id = protocol_event_id(event.id.as_str(), "revision_decision_event_id")?;
            let author_pubkey =
                protocol_pubkey(event.author.as_str(), "revision_decision_author_pubkey")?;
            let envelope = order_revision_decision_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!(
                    "decode active trade revision decision event: {error}"
                ))
            })?;
            let context = order_event_context_from_tags(
                RadrootsOrderEventType::OrderRevisionDecision,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!(
                    "decode active trade revision decision tags: {error}"
                ))
            })?;
            Ok(OrderStatusRecord::RevisionDecision(
                RadrootsOrderRevisionDecisionRecord {
                    event_id,
                    author_pubkey,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: required_order_context_event_id(
                        context.root_event_id,
                        "e_root",
                        "active trade revision decision",
                    )?,
                    prev_event_id: required_order_context_event_id(
                        context.prev_event_id,
                        "e_prev",
                        "active trade revision decision",
                    )?,
                    payload: envelope.payload,
                },
            ))
        }
        KIND_ORDER_CANCELLATION => {
            let event = radroots_event_from_nostr(event);
            let event_id = protocol_event_id(event.id.as_str(), "cancellation_event_id")?;
            let author_pubkey =
                protocol_pubkey(event.author.as_str(), "cancellation_author_pubkey")?;
            let envelope = order_cancellation_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!("decode active trade cancellation event: {error}"))
            })?;
            let context =
                order_event_context_from_tags(RadrootsOrderEventType::OrderCancelled, &event.tags)
                    .map_err(|error| {
                        RuntimeError::Config(format!(
                            "decode active trade cancellation tags: {error}"
                        ))
                    })?;
            Ok(OrderStatusRecord::Cancellation(
                RadrootsOrderCancellationRecord {
                    event_id,
                    author_pubkey,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: required_order_context_event_id(
                        context.root_event_id,
                        "e_root",
                        "active trade cancellation",
                    )?,
                    prev_event_id: required_order_context_event_id(
                        context.prev_event_id,
                        "e_prev",
                        "active trade cancellation",
                    )?,
                    payload: envelope.payload,
                },
            ))
        }
        event_kind => Err(RuntimeError::Config(format!(
            "order status received unexpected kind `{event_kind}`"
        ))),
    }
}

fn order_revision_proposals_from_events(
    order_id: &str,
    events: &[RadrootsNostrEvent],
) -> OrderRevisionProposalCandidates {
    let mut records = Vec::new();
    let mut issues = Vec::new();
    for event in events {
        if event_kind_u32(event) != KIND_ORDER_REVISION_PROPOSAL
            || !event_matches_tag_value(event, "d", order_id)
        {
            continue;
        }
        let event_id = event.id.to_string();
        match order_status_record_from_event(event) {
            Ok(OrderStatusRecord::RevisionProposal(record)) => records.push(record),
            Ok(_) => issues.push(issue_with_events(
                "invalid_revision_candidate",
                "revision_event_id",
                format!("revision event `{event_id}` decoded as the wrong active record type"),
                vec![event_id],
            )),
            Err(error) => issues.push(issue_with_events(
                "invalid_revision_candidate",
                "revision_event_id",
                format!("revision event `{event_id}` failed proposal validation: {error}"),
                vec![event_id],
            )),
        }
    }
    records.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    issues.sort_by(|left, right| left.event_ids.cmp(&right.event_ids));
    OrderRevisionProposalCandidates { records, issues }
}

fn order_event_list_unconfigured(
    seller_pubkey: Option<String>,
    actor_context_source: &'static str,
    reason: String,
    target_relays: Vec<String>,
    actions: Vec<String>,
) -> OrderEventListView {
    OrderEventListView {
        state: "unconfigured".to_owned(),
        source: ORDER_EVENT_LIST_SOURCE.to_owned(),
        actor_context_source: actor_context_source.to_owned(),
        seller_pubkey,
        target_relays,
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: 0,
        decoded_count: 0,
        skipped_count: 0,
        count: 0,
        reason: Some(reason),
        orders: Vec::new(),
        actions,
    }
}

fn order_event_list_unavailable(
    seller_pubkey: String,
    actor_context_source: &'static str,
    reason: String,
    target_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> OrderEventListView {
    OrderEventListView {
        state: "unavailable".to_owned(),
        source: ORDER_EVENT_LIST_SOURCE.to_owned(),
        actor_context_source: actor_context_source.to_owned(),
        seller_pubkey: Some(seller_pubkey),
        target_relays,
        connected_relays: Vec::new(),
        failed_relays: relay_failures(failed_relays),
        fetched_count: 0,
        decoded_count: 0,
        skipped_count: 0,
        count: 0,
        reason: Some(format!("direct relay connection failed: {reason}")),
        orders: Vec::new(),
        actions: Vec::new(),
    }
}

fn order_event_list_from_receipt(
    seller_pubkey: String,
    order_id: Option<&str>,
    actor_context_source: &'static str,
    receipt: DirectRelayFetchReceipt,
) -> OrderEventListView {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        events,
    } = receipt;
    let fetched_count = events.len();
    let mut skipped_count = 0usize;
    let mut decoded_count = 0usize;
    let mut orders = Vec::new();

    for event in events {
        match order_event_list_entry_from_event(&event, seller_pubkey.as_str()) {
            Ok(entry) => {
                decoded_count += 1;
                if order_id.is_none_or(|order_id| entry.id == order_id) {
                    orders.push(entry);
                }
            }
            Err(_) => skipped_count += 1,
        }
    }

    orders.sort_by(|left, right| {
        right
            .updated_at_unix
            .cmp(&left.updated_at_unix)
            .then_with(|| left.id.cmp(&right.id))
    });

    let reason = if orders.is_empty() {
        Some(match order_id {
            Some(order_id) => {
                format!("no relay-backed order request events matched `{order_id}`")
            }
            None => "no relay-backed order request events matched the selected seller".to_owned(),
        })
    } else {
        None
    };

    OrderEventListView {
        state: if orders.is_empty() { "empty" } else { "ready" }.to_owned(),
        source: ORDER_EVENT_LIST_SOURCE.to_owned(),
        actor_context_source: actor_context_source.to_owned(),
        seller_pubkey: Some(seller_pubkey),
        target_relays,
        connected_relays,
        failed_relays: relay_failures(failed_relays),
        fetched_count,
        decoded_count,
        skipped_count,
        count: orders.len(),
        reason,
        orders,
        actions: Vec::new(),
    }
}

fn order_decision_base_view(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    state: &str,
    dry_run: bool,
) -> OrderDecisionView {
    OrderDecisionView {
        state: state.to_owned(),
        source: ORDER_DECISION_SOURCE.to_owned(),
        order_id: args.key.clone(),
        locator: order_locator_view_from_key(args.key.as_str()),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        decision: args.decision.as_str().to_owned(),
        request_event_id: None,
        listing_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
        inventory: None,
        dry_run,
        target_relays: config.relay.urls.clone(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: 0,
        decoded_count: 0,
        skipped_count: 0,
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: None,
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

fn order_revision_base_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    state: &str,
    dry_run: bool,
) -> OrderRevisionProposalView {
    OrderRevisionProposalView {
        state: state.to_owned(),
        source: ORDER_REVISION_PROPOSAL_SOURCE.to_owned(),
        order_id: args.key.clone(),
        locator: order_locator_view_from_key(args.key.as_str()),
        revision_id: None,
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        decision_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
        items: Vec::new(),
        economics: None,
        inventory: None,
        dry_run,
        target_relays: config.relay.urls.clone(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: 0,
        decoded_count: 0,
        skipped_count: 0,
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: None,
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

fn order_revision_decision_base_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    state: &str,
    dry_run: bool,
) -> OrderRevisionDecisionView {
    OrderRevisionDecisionView {
        state: state.to_owned(),
        source: ORDER_REVISION_DECISION_SOURCE.to_owned(),
        order_id: args.key.clone(),
        locator: order_locator_view_from_key(args.key.as_str()),
        revision_id: Some(args.revision_id.trim().to_owned()).filter(|value| !value.is_empty()),
        decision: Some(args.decision.as_str().to_owned()),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        decision_event_id: None,
        agreement_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
        economics: None,
        inventory: None,
        dry_run,
        target_relays: config.relay.urls.clone(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: 0,
        decoded_count: 0,
        skipped_count: 0,
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: args.reason.as_ref().map(|reason| reason.trim().to_owned()),
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

fn order_cancellation_base_view(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    state: &str,
    dry_run: bool,
) -> OrderCancellationView {
    OrderCancellationView {
        state: state.to_owned(),
        source: ORDER_CANCELLATION_SOURCE.to_owned(),
        order_id: args.key.clone(),
        locator: order_locator_view_from_key(args.key.as_str()),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        decision_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
        cancellation_reason: Some(args.reason.clone()),
        dry_run,
        target_relays: config.relay.urls.clone(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: 0,
        decoded_count: 0,
        skipped_count: 0,
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: None,
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

fn apply_order_cancellation_status(view: &mut OrderCancellationView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.locator = order_locator_view_from_status(status);
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.decision_event_id = status.decision_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id = order_cancellation_prev_event_id(status);
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
}

fn order_cancellation_prev_event_id(status: &OrderStatusView) -> Option<String> {
    match status.state.as_str() {
        "requested" => status.request_event_id.clone(),
        "pending_rhi" => status
            .last_event_id
            .clone()
            .or(status.decision_event_id.clone()),
        _ => status.last_event_id.clone(),
    }
}

fn order_cancellation_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
) -> Option<OrderCancellationView> {
    let buyer_matches = status
        .buyer_pubkey
        .as_deref()
        .is_some_and(|buyer| buyer.eq_ignore_ascii_case(selected_pubkey));
    let state = match status.state.as_str() {
        "requested" if buyer_matches => return None,
        "pending_rhi" if buyer_matches => "finalized",
        "committed" | "cancelled" => "terminal",
        "missing" | "declined" | "invalid" | "unavailable" | "unconfigured" => {
            status.state.as_str()
        }
        _ => "invalid",
    };
    let mut view = order_cancellation_base_view(config, args, state, config.output.dry_run);
    apply_order_cancellation_status(&mut view, status);
    if status.state == "cancelled" {
        view.event_id = status
            .lifecycle
            .as_ref()
            .and_then(|lifecycle| lifecycle.event_id.clone());
        view.event_kind = Some(KIND_ORDER_CANCELLATION);
    }
    view.reason = Some(match state {
        "missing" => format!("no active trade events matched `{}`", args.key),
        "declined" => format!(
            "trade cancel refused because order `{}` was declined",
            args.key
        ),
        "terminal" => {
            format!(
                "trade cancel refused because order `{}` is already terminal",
                args.key
            )
        }
        "finalized" => format!(
            "trade cancel refused because order `{}` already has an accepted agreement",
            args.key
        ),
        "invalid" if !buyer_matches && status.buyer_pubkey.is_some() => format!(
            "trade cancel refused because selected account is not buyer for order `{}`",
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "trade cancel refused because active trade events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "trade cancel status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    view.actions = vec![format!("radroots trade status get {}", args.key)];
    Some(view)
}

fn order_decision_view_from_resolution(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    seller_pubkey: String,
    resolution: SellerOrderRequestResolution,
) -> OrderDecisionView {
    let SellerOrderRequestResolution {
        target_relays,
        connected_relays,
        failed_relays,
        fetched_count,
        decoded_count,
        skipped_count,
        requests,
        candidate_issues,
    } = resolution;
    let mut view = order_decision_base_view(config, args, "missing", config.output.dry_run);
    view.seller_pubkey = Some(seller_pubkey);
    view.target_relays = target_relays;
    view.connected_relays = connected_relays;
    view.failed_relays = relay_failures(failed_relays);
    view.fetched_count = fetched_count;
    view.decoded_count = decoded_count;
    view.skipped_count = skipped_count;
    view.issues = candidate_issues;

    if !view.issues.is_empty() {
        view.state = "invalid".to_owned();
        view.reason = Some(format!(
            "seller order request preflight found invalid request candidates for `{}`",
            args.key
        ));
        view.actions = vec![format!("radroots trade status get {}", args.key)];
        return view;
    }
    match requests.as_slice() {
        [] => {
            view.reason = Some(format!(
                "no seller-targeted order request event matched `{}`",
                args.key
            ));
            view
        }
        _ => {
            let event_ids = requests
                .iter()
                .map(|request| request.request_event_id.to_string())
                .collect::<Vec<_>>();
            view.state = "invalid".to_owned();
            view.reason = Some(format!(
                "multiple seller-targeted order request events matched `{}`; refusing to choose an order root",
                args.key
            ));
            view.issues = vec![issue_with_events(
                "multiple_request_candidates",
                "request_event_id",
                format!(
                    "matched {} request events for the same order id: {}",
                    requests.len(),
                    event_ids.join(", ")
                ),
                event_ids,
            )];
            view.actions = vec![format!("radroots trade status get {}", args.key)];
            view
        }
    }
}

fn apply_order_decision_resolution(
    view: &mut OrderDecisionView,
    resolution: &SellerOrderRequestResolution,
) {
    view.target_relays = resolution.target_relays.clone();
    view.connected_relays = resolution.connected_relays.clone();
    view.failed_relays = relay_failures(resolution.failed_relays.clone());
    view.fetched_count = resolution.fetched_count;
    view.decoded_count = resolution.decoded_count;
    view.skipped_count = resolution.skipped_count;
}

fn apply_order_decision_request(
    view: &mut OrderDecisionView,
    request: &ResolvedSellerOrderRequest,
) {
    view.order_id = request.order_id.to_string();
    view.locator = OrderTradeLocatorView {
        trade_id: request.order_id.to_string(),
        root_event_id: Some(request.request_event_id.to_string()),
        listing_addr: Some(request.listing_addr.to_string()),
        buyer_pubkey: Some(request.buyer_pubkey.to_string()),
        seller_pubkey: Some(request.seller_pubkey.to_string()),
    };
    view.listing_addr = Some(request.listing_addr.to_string());
    view.buyer_pubkey = Some(request.buyer_pubkey.to_string());
    view.seller_pubkey = Some(request.seller_pubkey.to_string());
    view.request_event_id = Some(request.request_event_id.to_string());
    view.listing_event_id = request.listing_event_id.clone();
    view.root_event_id = Some(request.request_event_id.to_string());
    view.prev_event_id = Some(request.request_event_id.to_string());
}

fn apply_order_decision_status(view: &mut OrderDecisionView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.locator = order_locator_view_from_status(status);
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.listing_event_id = status.listing_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id = status.last_event_id.clone();
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
    view.inventory = status.inventory.clone();
}

fn apply_order_revision_status(view: &mut OrderRevisionProposalView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.locator = order_locator_view_from_status(status);
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.decision_event_id = status.decision_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id = status.last_event_id.clone();
    view.economics = status.economics.clone();
    view.inventory = status.inventory.clone();
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
}

fn apply_order_revision_decision_status(
    view: &mut OrderRevisionDecisionView,
    status: &OrderStatusView,
) {
    view.order_id = status.order_id.clone();
    view.locator = order_locator_view_from_status(status);
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.decision_event_id = status.decision_event_id.clone();
    view.agreement_event_id = status.agreement_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id = status.last_event_id.clone();
    view.economics = status.economics.clone();
    view.inventory = status.inventory.clone();
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
}

fn order_decision_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
) -> Option<OrderDecisionView> {
    let state = match status.state.as_str() {
        "pending_rhi" | "declined" => "already_decided",
        "committed" | "cancelled" => "terminal",
        "invalid" => "invalid",
        "unavailable" => "unavailable",
        "unconfigured" => "unconfigured",
        _ => return None,
    };
    let mut view = order_decision_base_view(config, args, state, config.output.dry_run);
    apply_order_decision_resolution(&mut view, resolution);
    apply_order_decision_request(&mut view, request);
    apply_order_decision_status(&mut view, status);
    if let Some(decision_event_id) = &status.decision_event_id {
        view.event_id = Some(decision_event_id.clone());
        view.event_kind = Some(KIND_ORDER_DECISION);
    }
    view.reason = Some(match status.state.as_str() {
        "pending_rhi" | "declined" => format!(
            "order {} refused because order `{}` already has a visible `{}` seller decision",
            args.decision.command(),
            request.order_id,
            status.state
        ),
        "committed" | "cancelled" => format!(
            "order {} refused because order `{}` is already terminal",
            args.decision.command(),
            request.order_id
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order {} refused because active trade events for `{}` are invalid",
                args.decision.command(),
                request.order_id
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order {} status preflight failed with state `{}`",
                args.decision.command(),
                status.state
            )
        }),
    });
    view.actions = vec![format!("radroots trade status get {}", request.order_id)];
    Some(view)
}

fn order_revision_args_preflight_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
) -> Option<OrderRevisionProposalView> {
    let mut issues = Vec::new();
    let has_bin_id = args.bin_id.as_deref().and_then(non_empty_ref).is_some();
    let has_bin_count = args.bin_count.is_some();
    if has_bin_id != has_bin_count {
        issues.push(issue_with_code(
            "revision_item_change_incomplete",
            "bin_id",
            "`bin_id` and `bin_count` must be supplied together",
        ));
    }
    if args.bin_count == Some(0) {
        issues.push(issue_with_code(
            "revision_bin_count_invalid",
            "bin_count",
            "bin_count must be greater than zero",
        ));
    }

    let adjustment_inputs = [
        args.adjustment_id.as_deref(),
        args.adjustment_effect.as_deref(),
        args.adjustment_amount.as_deref(),
        args.adjustment_currency.as_deref(),
        args.adjustment_reason.as_deref(),
    ];
    let adjustment_supplied = adjustment_inputs
        .iter()
        .any(|value| value.and_then(non_empty_ref).is_some());
    let adjustment_complete = adjustment_inputs
        .iter()
        .all(|value| value.and_then(non_empty_ref).is_some());
    if adjustment_supplied && !adjustment_complete {
        issues.push(issue_with_code(
            "revision_adjustment_incomplete",
            "adjustment",
            "all revision adjustment fields must be supplied together",
        ));
    }

    if !has_bin_id && !adjustment_supplied {
        issues.push(issue_with_code(
            "revision_no_changes",
            "revision",
            "trade revision propose requires a bin-count change or revision adjustment",
        ));
    }

    if issues.is_empty() {
        return None;
    }
    let mut view = order_revision_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(format!(
        "trade revision propose inputs for `{}` failed validation",
        args.key
    ));
    view.issues = issues;
    Some(view)
}

fn order_revision_decision_args_preflight_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
) -> Option<OrderRevisionDecisionView> {
    let mut issues = Vec::new();
    if args.revision_id.trim().is_empty() {
        issues.push(issue_with_code(
            "revision_id_required",
            "revision_id",
            "trade revision decision requires --revision-id",
        ));
    }
    if args.decision == TradeRevisionDecisionArg::Decline
        && args
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .is_none()
    {
        issues.push(issue_with_code(
            "revision_decline_reason_required",
            "reason",
            "trade revision decline requires a non-empty reason",
        ));
    }

    if issues.is_empty() {
        return None;
    }
    let mut view =
        order_revision_decision_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(format!(
        "trade revision {} inputs for `{}` failed validation",
        args.decision.command(),
        args.key
    ));
    view.issues = issues;
    Some(view)
}

fn order_revision_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
    candidates: &OrderRevisionProposalCandidates,
) -> Option<OrderRevisionProposalView> {
    let pending_revision = pending_revision_proposal_candidate(status, candidates);
    let seller_matches = status
        .seller_pubkey
        .as_deref()
        .is_some_and(|seller| seller.eq_ignore_ascii_case(selected_pubkey));
    let state = match status.state.as_str() {
        "pending_rhi"
            if seller_matches && candidates.issues.is_empty() && pending_revision.is_none() =>
        {
            return None;
        }
        "pending_rhi" if !seller_matches => "invalid",
        "pending_rhi" if !candidates.issues.is_empty() => "invalid",
        "pending_rhi" if pending_revision.is_some() => "forked",
        "committed" | "cancelled" => "terminal",
        "missing" | "requested" | "declined" | "invalid" | "unavailable" | "unconfigured" => {
            status.state.as_str()
        }
        _ => "invalid",
    };
    let mut view = order_revision_base_view(config, args, state, config.output.dry_run);
    apply_order_revision_status(&mut view, status);
    if let Some(record) = pending_revision {
        view.event_id = Some(record.event_id.to_string());
        view.event_kind = Some(KIND_ORDER_REVISION_PROPOSAL);
        view.revision_id = Some(record.payload.revision_id.to_string());
    }
    view.reason = Some(match state {
        "missing" => format!("no active trade events matched `{}`", args.key),
        "requested" => format!(
            "trade revision propose refused because order `{}` has no accepted seller decision",
            args.key
        ),
        "declined" => format!(
            "trade revision propose refused because order `{}` was declined",
            args.key
        ),
        "terminal" => format!(
            "trade revision propose refused because order `{}` is already terminal",
            args.key
        ),
        "forked" => format!(
            "trade revision propose refused because order `{}` already has a pending revision proposal",
            args.key
        ),
        "invalid" if !seller_matches && status.seller_pubkey.is_some() => format!(
            "trade revision propose refused because selected account is not seller for order `{}`",
            args.key
        ),
        "invalid" if !candidates.issues.is_empty() => format!(
            "trade revision propose refused because revision proposal candidates for `{}` are invalid",
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "trade revision propose refused because active trade events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "trade revision propose status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    if state == "forked" {
        view.issues.push(issue_with_events(
            "pending_revision_exists",
            "revision_id",
            "a seller revision proposal is already visible for this accepted order",
            candidates
                .records
                .iter()
                .filter(|record| Some(record.event_id.as_str()) == status.last_event_id.as_deref())
                .map(|record| record.event_id.clone())
                .collect(),
        ));
    }
    view.issues.extend(candidates.issues.clone());
    view.actions = vec![format!("radroots trade status get {}", args.key)];
    Some(view)
}

fn order_revision_decision_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
    candidates: &OrderRevisionProposalCandidates,
) -> Option<OrderRevisionDecisionView> {
    let pending_revision = pending_revision_proposal_candidate(status, candidates);
    let buyer_matches = status
        .buyer_pubkey
        .as_deref()
        .is_some_and(|buyer| buyer.eq_ignore_ascii_case(selected_pubkey));
    let state = match status.state.as_str() {
        "revision_proposed"
            if buyer_matches && candidates.issues.is_empty() && pending_revision.is_some() =>
        {
            return None;
        }
        "revision_proposed" if !buyer_matches => "invalid",
        "revision_proposed" if !candidates.issues.is_empty() => "invalid",
        "revision_proposed" => "missing",
        "committed" | "cancelled" => "terminal",
        "declined" => "order_declined",
        "missing" | "requested" | "pending_rhi" | "invalid" | "unavailable" | "unconfigured" => {
            status.state.as_str()
        }
        _ => "invalid",
    };
    let mut view = order_revision_decision_base_view(config, args, state, config.output.dry_run);
    apply_order_revision_decision_status(&mut view, status);
    if let Some(record) = pending_revision {
        apply_order_revision_decision_proposal(&mut view, record);
        view.event_id = Some(record.event_id.to_string());
        view.event_kind = Some(KIND_ORDER_REVISION_PROPOSAL);
    }
    view.reason = Some(match state {
        "missing" if status.state == "revision_proposed" => format!(
            "trade revision {} refused because order `{}` has no pending revision proposal",
            args.decision.command(),
            args.key
        ),
        "missing" => format!("no active trade events matched `{}`", args.key),
        "requested" => format!(
            "trade revision {} refused because order `{}` has no accepted seller decision",
            args.decision.command(),
            args.key
        ),
        "order_declined" => format!(
            "trade revision {} refused because order `{}` was declined",
            args.decision.command(),
            args.key
        ),
        "terminal" => format!(
            "trade revision {} refused because order `{}` is already terminal",
            args.decision.command(),
            args.key
        ),
        "invalid" if !buyer_matches && status.buyer_pubkey.is_some() => format!(
            "trade revision {} refused because selected account is not buyer for order `{}`",
            args.decision.command(),
            args.key
        ),
        "invalid" if !candidates.issues.is_empty() => format!(
            "trade revision {} refused because revision proposal candidates for `{}` are invalid",
            args.decision.command(),
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "trade revision {} refused because active trade events for `{}` are invalid",
                args.decision.command(),
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "trade revision {} status preflight failed with state `{}`",
                args.decision.command(),
                status.state
            )
        }),
    });
    view.issues.extend(candidates.issues.clone());
    view.actions = vec![format!("radroots trade status get {}", args.key)];
    Some(view)
}

fn pending_revision_proposal_candidate<'a>(
    status: &OrderStatusView,
    candidates: &'a OrderRevisionProposalCandidates,
) -> Option<&'a OrderRevisionProposalRecord> {
    let last_event_id = status.last_event_id.as_deref()?;
    candidates
        .records
        .iter()
        .find(|record| record.event_id == last_event_id)
}

fn order_revision_invalid_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderRevisionProposalView {
    let mut view = order_revision_base_view(config, args, "invalid", config.output.dry_run);
    apply_order_revision_status(&mut view, status);
    view.reason = Some(reason.into());
    view.issues.extend(issues);
    view.actions = vec![format!("radroots trade status get {}", args.key)];
    view
}

fn order_revision_decision_invalid_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    status: &OrderStatusView,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderRevisionDecisionView {
    let mut view =
        order_revision_decision_base_view(config, args, "invalid", config.output.dry_run);
    apply_order_revision_decision_status(&mut view, status);
    view.reason = Some(reason.into());
    view.issues.extend(issues);
    view.actions = vec![format!("radroots trade status get {}", args.key)];
    view
}

fn order_revision_dry_run_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    payload: &RadrootsOrderRevisionProposal,
) -> OrderRevisionProposalView {
    let mut view = order_revision_base_view(config, args, "dry_run", true);
    apply_order_revision_status(&mut view, status);
    apply_order_revision_payload(&mut view, payload);
    view.reason =
        Some("dry run requested; seller revision proposal publication skipped".to_owned());
    view.actions = vec![format!("radroots trade status get {}", status.order_id)];
    view
}

fn order_revision_decision_dry_run_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    status: &OrderStatusView,
    proposal: &OrderRevisionProposalRecord,
    payload: &RadrootsOrderRevisionDecision,
) -> OrderRevisionDecisionView {
    let mut view = order_revision_decision_base_view(config, args, "dry_run", true);
    apply_order_revision_decision_status(&mut view, status);
    apply_order_revision_decision_payload(&mut view, proposal, payload);
    view.reason = Some(format!(
        "dry run requested; buyer revision {} publication skipped",
        args.decision.command()
    ));
    view.actions = vec![format!("radroots trade status get {}", status.order_id)];
    view
}

fn order_cancellation_dry_run_view(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    status: &OrderStatusView,
) -> OrderCancellationView {
    let mut view = order_cancellation_base_view(config, args, "dry_run", true);
    apply_order_cancellation_status(&mut view, status);
    view.reason =
        Some("dry run requested; buyer trade cancellation publication skipped".to_owned());
    view.actions = vec![format!("radroots trade status get {}", status.order_id)];
    view
}

fn order_cancellation_payload_from_status(
    args: &TradeCancelArgs,
    status: &OrderStatusView,
) -> Result<RadrootsOrderCancellation, RuntimeError> {
    Ok(RadrootsOrderCancellation {
        order_id: protocol_order_id(status.order_id.as_str(), "order_id")?,
        listing_addr: protocol_listing_addr(
            status.listing_addr.as_deref().ok_or_else(|| {
                RuntimeError::Config("cancellable order is missing listing_addr".to_owned())
            })?,
            "listing_addr",
        )?,
        buyer_pubkey: protocol_pubkey(
            status.buyer_pubkey.as_deref().ok_or_else(|| {
                RuntimeError::Config("cancellable order is missing buyer_pubkey".to_owned())
            })?,
            "buyer_pubkey",
        )?,
        seller_pubkey: protocol_pubkey(
            status.seller_pubkey.as_deref().ok_or_else(|| {
                RuntimeError::Config("cancellable order is missing seller_pubkey".to_owned())
            })?,
            "seller_pubkey",
        )?,
        reason: args.reason.trim().to_owned(),
    })
}

fn order_revision_payload_from_status(
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
) -> Result<RadrootsOrderRevisionProposal, RuntimeError> {
    let revision_id = protocol_revision_id(next_revision_id().as_str(), "revision_id")?;
    let economics = status.economics.clone().ok_or_else(|| {
        RuntimeError::Config("accepted order is missing current agreement economics".to_owned())
    })?;
    let economics = revised_order_economics(args, revision_id.as_str(), &economics)?;
    let items = economics
        .items
        .iter()
        .map(|item| RadrootsOrderItem {
            bin_id: item.bin_id.clone(),
            bin_count: item.bin_count,
        })
        .collect::<Vec<_>>();
    Ok(RadrootsOrderRevisionProposal {
        revision_id,
        order_id: protocol_order_id(status.order_id.as_str(), "order_id")?,
        listing_addr: protocol_listing_addr(
            status.listing_addr.as_deref().ok_or_else(|| {
                RuntimeError::Config("accepted order is missing listing_addr".to_owned())
            })?,
            "listing_addr",
        )?,
        buyer_pubkey: protocol_pubkey(
            status.buyer_pubkey.as_deref().ok_or_else(|| {
                RuntimeError::Config("accepted order is missing buyer_pubkey".to_owned())
            })?,
            "buyer_pubkey",
        )?,
        seller_pubkey: protocol_pubkey(
            status.seller_pubkey.as_deref().ok_or_else(|| {
                RuntimeError::Config("accepted order is missing seller_pubkey".to_owned())
            })?,
            "seller_pubkey",
        )?,
        root_event_id: protocol_event_id(
            status.request_event_id.as_deref().ok_or_else(|| {
                RuntimeError::Config("accepted order is missing request_event_id".to_owned())
            })?,
            "request_event_id",
        )?,
        prev_event_id: protocol_event_id(
            status
                .last_event_id
                .as_deref()
                .or(status.decision_event_id.as_deref())
                .ok_or_else(|| {
                    RuntimeError::Config("accepted order is missing previous event id".to_owned())
                })?,
            "prev_event_id",
        )?,
        items,
        economics,
        reason: args.reason.trim().to_owned(),
    })
}

fn revised_order_economics(
    args: &TradeRevisionProposeArgs,
    revision_id: &str,
    current: &RadrootsOrderEconomics,
) -> Result<RadrootsOrderEconomics, RuntimeError> {
    let mut current_canonical = current.clone();
    current_canonical.canonicalize();
    let mut economics = current_canonical.clone();
    let mut changed = false;
    economics.quote_id = protocol_quote_id(format!("revision_{revision_id}").as_str(), "quote_id")?;
    economics.quote_version = economics
        .quote_version
        .checked_add(1)
        .ok_or_else(|| RuntimeError::Config("revision quote_version overflowed".to_owned()))?;

    if let Some(bin_id) = args.bin_id.as_deref().and_then(non_empty_ref) {
        let bin_id = protocol_inventory_bin_id(bin_id, "revision bin_id")?;
        let bin_count = args.bin_count.ok_or_else(|| {
            RuntimeError::Config("revision bin_count is required with bin_id".to_owned())
        })?;
        let Some(item) = economics
            .items
            .iter_mut()
            .find(|item| item.bin_id == bin_id)
        else {
            return Err(RuntimeError::Config(format!(
                "revision bin `{bin_id}` is not part of the current agreement"
            )));
        };
        if item.bin_count != bin_count {
            changed = true;
        }
        item.bin_count = bin_count;
        item.line_subtotal = RadrootsCoreMoney::new(
            item.unit_price_amount * item.quantity_amount * RadrootsCoreDecimal::from(bin_count),
            item.unit_price_currency,
        );
    }

    if let Some(line) = revision_adjustment_line(args, economics.currency)? {
        changed = true;
        if economics
            .adjustments
            .iter()
            .any(|existing| existing.id == line.id)
        {
            return Err(RuntimeError::Config(format!(
                "revision adjustment id `{}` already exists in current agreement economics",
                line.id
            )));
        }
        economics.adjustments.push(line);
    }

    economics.canonicalize();
    economics
        .validate()
        .map_err(|error| RuntimeError::Config(format!("build revision economics: {error}")))?;
    if !changed {
        return Err(RuntimeError::Config(
            "trade revision propose requires a changed item count or adjustment".to_owned(),
        ));
    }
    Ok(economics)
}

fn revision_adjustment_line(
    args: &TradeRevisionProposeArgs,
    expected_currency: RadrootsCoreCurrency,
) -> Result<Option<RadrootsOrderEconomicLine>, RuntimeError> {
    let Some(id) = args.adjustment_id.as_deref().and_then(non_empty_ref) else {
        return Ok(None);
    };
    let effect = match args
        .adjustment_effect
        .as_deref()
        .and_then(non_empty_ref)
        .ok_or_else(|| RuntimeError::Config("revision adjustment effect is required".to_owned()))?
    {
        "increase" => RadrootsOrderEconomicEffect::Increase,
        "decrease" => RadrootsOrderEconomicEffect::Decrease,
        other => {
            return Err(RuntimeError::Config(format!(
                "revision adjustment effect `{other}` is invalid"
            )));
        }
    };
    let currency = parse_economics_currency(
        args.adjustment_currency
            .as_deref()
            .and_then(non_empty_ref)
            .ok_or_else(|| {
                RuntimeError::Config("revision adjustment currency is required".to_owned())
            })?,
        "revision_adjustment_currency",
    )?;
    if currency != expected_currency {
        return Err(RuntimeError::Config(
            "revision adjustment currency must match current agreement currency".to_owned(),
        ));
    }
    let amount = decimal_from_adjustment(
        args.adjustment_amount
            .as_deref()
            .and_then(non_empty_ref)
            .ok_or_else(|| {
                RuntimeError::Config("revision adjustment amount is required".to_owned())
            })?,
        "revision_adjustment_amount",
    )?;
    if amount.is_zero() {
        return Err(RuntimeError::Config(
            "revision adjustment amount must be greater than zero".to_owned(),
        ));
    }
    let reason = args
        .adjustment_reason
        .as_deref()
        .and_then(non_empty_ref)
        .ok_or_else(|| RuntimeError::Config("revision adjustment reason is required".to_owned()))?;
    Ok(Some(RadrootsOrderEconomicLine {
        id: id.to_owned(),
        kind: RadrootsOrderEconomicLineKind::RevisionAdjustment,
        actor: RadrootsOrderEconomicActor::Seller,
        effect,
        amount: RadrootsCoreMoney::new(amount, currency),
        reason: reason.to_owned(),
    }))
}

fn order_revision_inventory_preflight_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    payload: &RadrootsOrderRevisionProposal,
) -> Option<OrderRevisionProposalView> {
    let issues = order_revision_inventory_issues(status, payload);
    if issues.is_empty() {
        return None;
    }
    let mut view = order_revision_invalid_view(
        config,
        args,
        status,
        "trade revision propose refused because visible inventory is unavailable for the revised items",
        issues,
    );
    apply_order_revision_payload(&mut view, payload);
    Some(view)
}

fn order_revision_inventory_issues(
    status: &OrderStatusView,
    payload: &RadrootsOrderRevisionProposal,
) -> Vec<OrderIssueView> {
    let Some(current) = status.economics.as_ref() else {
        return vec![issue_with_code(
            "revision_current_economics_missing",
            "economics",
            "current agreement economics are required before revision proposal",
        )];
    };

    let current_counts = current
        .items
        .iter()
        .map(|item| (item.bin_id.as_str(), u64::from(item.bin_count)))
        .collect::<Vec<_>>();
    let mut issues = Vec::new();
    for item in &payload.items {
        let current_count = current_counts
            .iter()
            .find(|(bin_id, _)| *bin_id == item.bin_id)
            .map(|(_, count)| *count)
            .unwrap_or_default();
        let revised_count = u64::from(item.bin_count);
        if revised_count <= current_count {
            continue;
        }
        let Some(bin) = status
            .inventory
            .as_ref()
            .and_then(|inventory| inventory.bins.iter().find(|bin| bin.bin_id == item.bin_id))
        else {
            issues.push(issue_with_code(
                "revision_inventory_unavailable",
                "inventory.bin_id",
                format!(
                    "inventory availability for revised bin `{}` is not visible",
                    item.bin_id
                ),
            ));
            continue;
        };
        let Some(remaining_count) = bin.remaining_count else {
            issues.push(issue_with_code(
                "revision_inventory_unavailable",
                "inventory.remaining_count",
                format!(
                    "remaining inventory for revised bin `{}` is not visible",
                    item.bin_id
                ),
            ));
            continue;
        };
        let available_for_revision = remaining_count.saturating_add(current_count);
        if revised_count > available_for_revision {
            issues.push(issue_with_code(
                "revision_inventory_unavailable",
                "inventory.remaining_count",
                format!(
                    "revision requests {revised_count} of bin `{}`, but only {available_for_revision} are available after current reservation",
                    item.bin_id
                ),
            ));
        }
    }

    issues
}

fn apply_order_revision_payload(
    view: &mut OrderRevisionProposalView,
    payload: &RadrootsOrderRevisionProposal,
) {
    view.revision_id = Some(payload.revision_id.to_string());
    view.root_event_id = Some(payload.root_event_id.to_string());
    view.prev_event_id = Some(payload.prev_event_id.to_string());
    view.items = payload
        .items
        .iter()
        .map(|item| OrderDraftItemView {
            bin_id: item.bin_id.to_string(),
            bin_count: item.bin_count,
        })
        .collect();
    view.economics = Some(payload.economics.clone());
}

fn apply_order_revision_decision_proposal(
    view: &mut OrderRevisionDecisionView,
    proposal: &OrderRevisionProposalRecord,
) {
    view.revision_id = Some(proposal.payload.revision_id.to_string());
    view.root_event_id = Some(proposal.payload.root_event_id.to_string());
    view.prev_event_id = Some(proposal.event_id.to_string());
    view.event_id = Some(proposal.event_id.to_string());
    view.event_kind = Some(KIND_ORDER_REVISION_PROPOSAL);
    if view.decision.as_deref() == Some("accepted") {
        view.economics = Some(proposal.payload.economics.clone());
    }
}

fn apply_order_revision_decision_payload(
    view: &mut OrderRevisionDecisionView,
    proposal: &OrderRevisionProposalRecord,
    payload: &RadrootsOrderRevisionDecision,
) {
    view.revision_id = Some(payload.revision_id.to_string());
    view.root_event_id = Some(payload.root_event_id.to_string());
    view.prev_event_id = Some(payload.prev_event_id.to_string());
    view.decision = Some(
        match &payload.decision {
            RadrootsOrderRevisionOutcome::Accepted => "accepted",
            RadrootsOrderRevisionOutcome::Declined { .. } => "declined",
        }
        .to_owned(),
    );
    if matches!(payload.decision, RadrootsOrderRevisionOutcome::Accepted) {
        view.agreement_event_id = view.event_id.clone();
        view.economics = Some(proposal.payload.economics.clone());
    }
}

fn order_revision_decision_payload_from_proposal(
    args: &TradeRevisionDecisionArgs,
    proposal: &OrderRevisionProposalRecord,
) -> Result<RadrootsOrderRevisionDecision, RuntimeError> {
    let decision = match args.decision {
        TradeRevisionDecisionArg::Accept => RadrootsOrderRevisionOutcome::Accepted,
        TradeRevisionDecisionArg::Decline => {
            let reason = args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| {
                    RuntimeError::Config(
                        "trade revision decline requires a non-empty reason".to_owned(),
                    )
                })?;
            RadrootsOrderRevisionOutcome::Declined {
                reason: reason.to_owned(),
            }
        }
    };
    Ok(RadrootsOrderRevisionDecision {
        revision_id: proposal.payload.revision_id.clone(),
        order_id: proposal.payload.order_id.clone(),
        listing_addr: proposal.payload.listing_addr.clone(),
        buyer_pubkey: proposal.payload.buyer_pubkey.clone(),
        seller_pubkey: proposal.payload.seller_pubkey.clone(),
        root_event_id: proposal.payload.root_event_id.clone(),
        prev_event_id: proposal.event_id.clone(),
        decision,
    })
}

fn sdk_enqueued_order_revision_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    revision: SdkRevisionRequestParts,
    enqueue: TradeRevisionProposalReceipt,
    push: Option<&PushOutboxReceipt>,
) -> OrderRevisionProposalView {
    let push_event =
        push.and_then(|push| sdk_push_event_for_event_id(&enqueue.signed_event_id, push));
    let mut view = order_revision_base_view(
        config,
        args,
        sdk_order_lifecycle_state("proposed", push_event).as_str(),
        false,
    );
    apply_order_revision_status(&mut view, status);
    view.locator = order_locator_view_from_locator(&enqueue.locator);
    view.revision_id = Some(revision.revision_id.to_string());
    view.root_event_id = Some(enqueue.root_event_id.to_string());
    view.prev_event_id = Some(enqueue.previous_event_id.to_string());
    view.items = revision
        .items
        .iter()
        .map(|item| OrderDraftItemView {
            bin_id: item.bin_id.to_string(),
            bin_count: item.bin_count,
        })
        .collect();
    view.economics = Some(revision.economics);
    view.event_id = Some(enqueue.signed_event_id.as_str().to_owned());
    view.event_kind = Some(KIND_ORDER_REVISION_PROPOSAL);
    view.target_relays = push_event
        .map(sdk_push_target_relays)
        .unwrap_or_else(|| config.relay.urls.clone());
    view.connected_relays = push_event
        .map(sdk_push_connected_relays)
        .unwrap_or_default();
    view.acknowledged_relays = push_event
        .map(sdk_push_acknowledged_relays)
        .unwrap_or_default();
    view.failed_relays = push_event.map(sdk_push_failed_relays).unwrap_or_default();
    view.reason =
        sdk_order_lifecycle_reason("trade revision proposal", &enqueue.workflow, push_event);
    view.actions = sdk_order_lifecycle_actions(push_event);
    view
}

fn sdk_enqueued_order_revision_decision_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    status: &OrderStatusView,
    enqueue: TradeRevisionDecisionReceipt,
    push: Option<&PushOutboxReceipt>,
) -> OrderRevisionDecisionView {
    let push_event =
        push.and_then(|push| sdk_push_event_for_event_id(&enqueue.signed_event_id, push));
    let success_state = args.decision.as_str();
    let mut view = order_revision_decision_base_view(
        config,
        args,
        sdk_order_lifecycle_state(success_state, push_event).as_str(),
        false,
    );
    apply_order_revision_decision_status(&mut view, status);
    view.locator = order_locator_view_from_locator(&enqueue.locator);
    view.revision_id = Some(args.revision_id.trim().to_owned());
    view.root_event_id = Some(enqueue.root_event_id.to_string());
    view.prev_event_id = Some(enqueue.previous_event_id.to_string());
    view.decision = Some(args.decision.as_str().to_owned());
    view.event_id = Some(enqueue.signed_event_id.as_str().to_owned());
    view.event_kind = Some(KIND_ORDER_REVISION_DECISION);
    if args.decision == TradeRevisionDecisionArg::Accept {
        view.agreement_event_id = Some(enqueue.signed_event_id.as_str().to_owned());
    }
    view.target_relays = push_event
        .map(sdk_push_target_relays)
        .unwrap_or_else(|| config.relay.urls.clone());
    view.connected_relays = push_event
        .map(sdk_push_connected_relays)
        .unwrap_or_default();
    view.acknowledged_relays = push_event
        .map(sdk_push_acknowledged_relays)
        .unwrap_or_default();
    view.failed_relays = push_event.map(sdk_push_failed_relays).unwrap_or_default();
    view.reason =
        sdk_order_lifecycle_reason("trade revision decision", &enqueue.workflow, push_event);
    view.actions = sdk_order_lifecycle_actions(push_event);
    view
}

fn sdk_enqueued_order_cancellation_view(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    status: &OrderStatusView,
    enqueue: TradeCancellationReceipt,
    push: Option<&PushOutboxReceipt>,
) -> OrderCancellationView {
    let push_event =
        push.and_then(|push| sdk_push_event_for_event_id(&enqueue.signed_event_id, push));
    let mut view = order_cancellation_base_view(
        config,
        args,
        sdk_order_lifecycle_state("cancelled", push_event).as_str(),
        false,
    );
    apply_order_cancellation_status(&mut view, status);
    view.locator = order_locator_view_from_locator(&enqueue.locator);
    view.root_event_id = Some(enqueue.root_event_id.to_string());
    view.prev_event_id = Some(enqueue.previous_event_id.to_string());
    view.event_id = Some(enqueue.signed_event_id.as_str().to_owned());
    view.event_kind = Some(KIND_ORDER_CANCELLATION);
    view.target_relays = push_event
        .map(sdk_push_target_relays)
        .unwrap_or_else(|| config.relay.urls.clone());
    view.connected_relays = push_event
        .map(sdk_push_connected_relays)
        .unwrap_or_default();
    view.acknowledged_relays = push_event
        .map(sdk_push_acknowledged_relays)
        .unwrap_or_default();
    view.failed_relays = push_event.map(sdk_push_failed_relays).unwrap_or_default();
    view.reason = sdk_order_lifecycle_reason("trade cancellation", &enqueue.workflow, push_event);
    view.actions = sdk_order_lifecycle_actions(push_event);
    view
}

fn sdk_push_event_for_event_id<'a>(
    event_id: &RadrootsEventId,
    push: &'a PushOutboxReceipt,
) -> Option<&'a PushOutboxEventReceipt> {
    push.events.iter().find(|event| event.event_id == *event_id)
}

fn sdk_order_lifecycle_state(
    published_state: &str,
    push_event: Option<&PushOutboxEventReceipt>,
) -> String {
    match push_event.map(|event| event.final_state) {
        Some(PushOutboxEventState::Published) => published_state,
        Some(PushOutboxEventState::PublishRetryable | PushOutboxEventState::FailedTerminal) => {
            "unavailable"
        }
        Some(_) | None => "queued",
    }
    .to_owned()
}

fn sdk_order_lifecycle_reason(
    workflow: &str,
    enqueue: &TradeWorkflowEnqueueReceipt,
    push_event: Option<&PushOutboxEventReceipt>,
) -> Option<String> {
    match push_event.map(|event| event.final_state) {
        Some(PushOutboxEventState::Published) => None,
        Some(PushOutboxEventState::PublishRetryable) => Some(format!(
            "{}; SDK relay publish for {workflow} did not reach accepted quorum; outbox event remains retryable; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        Some(PushOutboxEventState::FailedTerminal) => Some(format!(
            "{}; SDK relay publish for {workflow} failed terminally; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        Some(state) => Some(format!(
            "{}; SDK relay push for {workflow} left event in state `{state:?}`; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        None => Some(format!(
            "{}; {workflow} queued in SDK outbox; no ready SDK outbox event was pushed; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
    }
}

fn sdk_order_lifecycle_actions(push_event: Option<&PushOutboxEventReceipt>) -> Vec<String> {
    if !matches!(
        push_event.map(|event| event.final_state),
        Some(PushOutboxEventState::Published)
    ) {
        return sdk_order_push_recovery_actions();
    }
    Vec::new()
}

fn sdk_order_enqueue_summary(enqueue: &TradeWorkflowEnqueueReceipt) -> String {
    format!(
        "local SDK enqueued `{}` as `{}` with outbox_event_id {}; {}",
        enqueue.operation_kind,
        sdk_mutation_state_label(&enqueue.state),
        enqueue.outbox_event_id,
        sdk_order_idempotency_summary(enqueue)
    )
}

fn sdk_order_idempotency_summary(enqueue: &TradeWorkflowEnqueueReceipt) -> &'static str {
    if enqueue.idempotency.replayed_existing_operation {
        "idempotency replayed an existing queued operation"
    } else if enqueue.idempotency.safe_to_retry_with_same_idempotency_key {
        "same idempotency key remains retry-safe"
    } else {
        "same idempotency key retry safety is unavailable"
    }
}

fn sdk_order_enqueue_retry_summary(enqueue: &TradeWorkflowEnqueueReceipt) -> &'static str {
    if enqueue
        .retry
        .safe_to_retry_enqueue_with_same_idempotency_key
    {
        "enqueue is safe to retry with the same idempotency key"
    } else if enqueue.retry.retryable_after_error {
        "inspect local SDK state before retrying enqueue"
    } else {
        "do not retry enqueue before inspecting local SDK state"
    }
}

fn sdk_mutation_state_label(state: &SdkMutationState) -> &'static str {
    match state {
        SdkMutationState::StoredAndQueued => "stored_and_queued",
        SdkMutationState::AlreadyQueued => "already_queued",
        _ => "unknown",
    }
}

fn sdk_order_push_recovery_actions() -> Vec<String> {
    vec![
        "radroots sync push".to_owned(),
        "radroots sync status get".to_owned(),
    ]
}

fn order_actor_write_binding_error_parts(
    error: ActorWriteBindingError,
) -> (String, String, Vec<String>) {
    (
        "unconfigured".to_owned(),
        error.reason(),
        vec!["run radroots signer status get".to_owned()],
    )
}

fn order_revision_binding_error_view(
    config: &RuntimeConfig,
    args: &TradeRevisionProposeArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderRevisionProposalView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view = order_revision_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_revision_status(&mut view, status);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn order_revision_decision_binding_error_view(
    config: &RuntimeConfig,
    args: &TradeRevisionDecisionArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderRevisionDecisionView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view =
        order_revision_decision_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_revision_decision_status(&mut view, status);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn order_cancellation_binding_error_view(
    config: &RuntimeConfig,
    args: &TradeCancelArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderCancellationView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view =
        order_cancellation_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_cancellation_status(&mut view, status);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn seller_order_request_resolution_from_receipt(
    seller_pubkey: &str,
    order_id: &str,
    receipt: DirectRelayFetchReceipt,
) -> Result<SellerOrderRequestResolution, RuntimeError> {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        events,
    } = receipt;
    let fetched_count = events.len();
    let mut skipped_count = 0usize;
    let mut decoded_count = 0usize;
    let mut requests = Vec::new();
    let mut candidate_issues = Vec::new();
    let candidate_context = OrderRequestCandidateContext {
        order_id,
        seller_pubkey: Some(seller_pubkey),
    };

    for event in events {
        if !order_request_candidate_matches(&event, candidate_context) {
            skipped_count += 1;
            continue;
        }
        let event_id = event.id.to_string();
        match seller_order_request_from_event(&event, seller_pubkey, order_id) {
            Ok(request) => {
                decoded_count += 1;
                requests.push(request);
            }
            Err(error) => {
                skipped_count += 1;
                candidate_issues.push(issue_with_events(
                    "invalid_request_candidate",
                    "request_event_id",
                    format!("request event `{event_id}` failed seller decision preflight: {error}"),
                    vec![event_id],
                ));
            }
        }
    }

    requests.sort_by(|left, right| left.request_event_id.cmp(&right.request_event_id));
    candidate_issues.sort_by(|left, right| left.message.cmp(&right.message));

    Ok(SellerOrderRequestResolution {
        target_relays,
        connected_relays,
        failed_relays,
        fetched_count,
        decoded_count,
        skipped_count,
        requests,
        candidate_issues,
    })
}

fn event_matches_tag_value(event: &RadrootsNostrEvent, key: &str, value: &str) -> bool {
    event.tags.iter().any(|tag| {
        let values = tag.as_slice();
        values.first().map(String::as_str) == Some(key)
            && values.get(1).map(String::as_str) == Some(value)
    })
}

fn seller_order_request_from_event(
    event: &RadrootsNostrEvent,
    seller_pubkey: &str,
    order_id: &str,
) -> Result<ResolvedSellerOrderRequest, RuntimeError> {
    let event_kind = event_kind_u32(event);
    if event_kind != KIND_ORDER_REQUEST {
        return Err(RuntimeError::Config(format!(
            "order decision received unexpected kind `{event_kind}`"
        )));
    }

    let request_event = radroots_event_from_nostr(event);
    let event_id = protocol_event_id(request_event.id.as_str(), "request_event_id")?;
    let seller_protocol_pubkey = protocol_pubkey(seller_pubkey, "seller_pubkey")?;
    let envelope = order_request_from_event(&request_event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context =
        order_event_context_from_tags(RadrootsOrderEventType::OrderRequested, &request_event.tags)
            .map_err(|error| RuntimeError::Config(format!("decode order request tags: {error}")))?;

    if envelope.order_id.to_string() != order_id
        || envelope.payload.order_id.to_string() != order_id
    {
        return Err(RuntimeError::Config(
            "order request does not match requested order id".to_owned(),
        ));
    }
    if context.counterparty_pubkey != seller_protocol_pubkey
        || envelope.payload.seller_pubkey != seller_protocol_pubkey
    {
        return Err(RuntimeError::Config(
            "order request is not targeted at the selected seller".to_owned(),
        ));
    }
    let listing_addr =
        parse_listing_addr(envelope.payload.listing_addr.as_str()).map_err(|error| {
            RuntimeError::Config(format!("order request listing_addr is invalid: {error}"))
        })?;
    if listing_addr.seller_pubkey != seller_pubkey {
        return Err(RuntimeError::Config(
            "order request listing address is outside selected seller authority".to_owned(),
        ));
    }
    let listing_event_id = context.listing_event.as_ref().map(|event| event.id.clone());

    Ok(ResolvedSellerOrderRequest {
        request_event,
        request_event_id: event_id,
        listing_event_id,
        order_id: envelope.payload.order_id,
        listing_addr: envelope.payload.listing_addr,
        buyer_pubkey: envelope.payload.buyer_pubkey,
        seller_pubkey: envelope.payload.seller_pubkey,
        items: envelope.payload.items,
        economics: envelope.payload.economics,
    })
}

fn sdk_enqueued_order_decision_view(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    status: &OrderStatusView,
    enqueue: TradeDecisionReceipt,
    push: Option<&PushOutboxReceipt>,
) -> OrderDecisionView {
    let push_event = push.and_then(|push| sdk_push_event_for_order_decision(&enqueue, push));
    let mut view = order_decision_base_view(
        config,
        args,
        sdk_order_decision_state(args.decision, push_event).as_str(),
        false,
    );
    apply_order_decision_status(&mut view, status);
    view.locator = order_locator_view_from_locator(&enqueue.locator);
    view.request_event_id = Some(enqueue.request_event_id.to_string());
    view.root_event_id = Some(enqueue.request_event_id.to_string());
    view.prev_event_id = Some(enqueue.request_event_id.to_string());
    view.event_id = Some(enqueue.signed_event_id.as_str().to_owned());
    view.event_kind = Some(KIND_ORDER_DECISION);
    view.target_relays = push_event
        .map(sdk_push_target_relays)
        .unwrap_or_else(|| config.relay.urls.clone());
    view.connected_relays = push_event
        .map(sdk_push_connected_relays)
        .unwrap_or_default();
    view.acknowledged_relays = push_event
        .map(sdk_push_acknowledged_relays)
        .unwrap_or_default();
    view.failed_relays = push_event.map(sdk_push_failed_relays).unwrap_or_default();
    view.reason = sdk_order_decision_reason(&enqueue.workflow, push_event);
    view.actions = sdk_order_decision_actions(push_event);
    view
}

fn sdk_push_event_for_order_decision<'a>(
    enqueue: &TradeDecisionReceipt,
    push: &'a PushOutboxReceipt,
) -> Option<&'a PushOutboxEventReceipt> {
    push.events
        .iter()
        .find(|event| event.event_id == enqueue.signed_event_id)
}

fn sdk_order_decision_state(
    decision: TradeDecisionArg,
    push_event: Option<&PushOutboxEventReceipt>,
) -> String {
    match push_event.map(|event| event.final_state) {
        Some(PushOutboxEventState::Published) => decision.as_str(),
        Some(PushOutboxEventState::PublishRetryable | PushOutboxEventState::FailedTerminal) => {
            "unavailable"
        }
        Some(_) | None => "queued",
    }
    .to_owned()
}

fn sdk_order_decision_reason(
    enqueue: &TradeWorkflowEnqueueReceipt,
    push_event: Option<&PushOutboxEventReceipt>,
) -> Option<String> {
    match push_event.map(|event| event.final_state) {
        Some(PushOutboxEventState::Published) => None,
        Some(PushOutboxEventState::PublishRetryable) => Some(format!(
            "{}; SDK relay publish did not reach accepted quorum; outbox event remains retryable; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        Some(PushOutboxEventState::FailedTerminal) => Some(format!(
            "{}; SDK relay publish failed terminally; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        Some(state) => Some(format!(
            "{}; SDK relay push left event in state `{state:?}`; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        None => Some(format!(
            "{}; order decision queued in SDK outbox; no ready SDK outbox event was pushed; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
    }
}

fn sdk_order_decision_actions(push_event: Option<&PushOutboxEventReceipt>) -> Vec<String> {
    if !matches!(
        push_event.map(|event| event.final_state),
        Some(PushOutboxEventState::Published)
    ) {
        return sdk_order_push_recovery_actions();
    }
    Vec::new()
}

fn order_decision_binding_error_view(
    config: &RuntimeConfig,
    args: &TradeDecisionArgs,
    request: ResolvedSellerOrderRequest,
    resolution: SellerOrderRequestResolution,
    error: ActorWriteBindingError,
) -> OrderDecisionView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view = order_decision_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_decision_resolution(&mut view, &resolution);
    apply_order_decision_request(&mut view, &request);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn order_event_list_entry_from_event(
    event: &RadrootsNostrEvent,
    seller_pubkey: &str,
) -> Result<OrderEventListEntryView, RuntimeError> {
    let event_kind = event_kind_u32(event);
    if event_kind != KIND_ORDER_REQUEST {
        return Err(RuntimeError::Config(format!(
            "trade event list received unexpected kind `{event_kind}`"
        )));
    }

    let event = radroots_event_from_nostr(event);
    let envelope = order_request_from_event(&event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context =
        order_event_context_from_tags(RadrootsOrderEventType::OrderRequested, &event.tags)
            .map_err(|error| RuntimeError::Config(format!("decode order request tags: {error}")))?;

    if context.counterparty_pubkey != seller_pubkey
        || envelope.payload.seller_pubkey != seller_pubkey
    {
        return Err(RuntimeError::Config(
            "order request is not targeted at the selected seller".to_owned(),
        ));
    }

    let listing_event_id = context.listing_event.as_ref().map(|event| event.id.clone());
    let created_at_unix = u64::from(event.created_at);

    Ok(OrderEventListEntryView {
        id: envelope.order_id.clone(),
        state: "requested".to_owned(),
        event_id: Some(event.id),
        event_kind: Some(event.kind),
        listing_lookup: None,
        listing_addr: Some(envelope.listing_addr),
        listing_event_id,
        buyer_account_id: None,
        buyer_pubkey: Some(envelope.payload.buyer_pubkey.to_string()),
        seller_pubkey: Some(envelope.payload.seller_pubkey.to_string()),
        item_count: Some(envelope.payload.items.len()),
        created_at_unix: Some(created_at_unix),
        submitted_at_unix: Some(created_at_unix),
        updated_at_unix: created_at_unix,
        job: None,
        workflow: None,
        issues: Vec::new(),
    })
}

fn order_request_filter(
    seller_pubkey: &str,
    order_id: Option<&str>,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_ORDER_REQUEST as u16))
        .limit(1_000);
    let filter = radroots_nostr_filter_tag(filter, "p", vec![seller_pubkey.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order event filter: {error}")))?;
    if let Some(order_id) = order_id {
        return radroots_nostr_filter_tag(filter, "d", vec![order_id.to_owned()])
            .map_err(|error| RuntimeError::Config(format!("build order event filter: {error}")));
    }
    Ok(filter)
}

fn listing_event_filter(
    listing_addr: &ParsedListingAddress,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_LISTING as u16))
        .limit(100);
    radroots_nostr_filter_tag(filter, "d", vec![listing_addr.listing_id.clone()])
        .map_err(|error| RuntimeError::Config(format!("build listing event filter: {error}")))
}

fn order_listing_request_filter(
    seller_pubkey: &str,
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_ORDER_REQUEST as u16))
        .limit(1_000);
    let filter = radroots_nostr_filter_tag(filter, "p", vec![seller_pubkey.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order request filter: {error}")))?;
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order request filter: {error}")))
}

fn order_listing_decision_filter(listing_addr: &str) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_ORDER_DECISION as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order decision filter: {error}")))
}

fn order_listing_revision_proposal_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_ORDER_REVISION_PROPOSAL as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build revision proposal filter: {error}")))
}

fn order_listing_revision_decision_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_ORDER_REVISION_DECISION as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build revision decision filter: {error}")))
}

fn order_listing_cancellation_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_ORDER_CANCELLATION as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build cancellation filter: {error}")))
}

fn order_status_filter(order_id: &str) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kinds([
            radroots_nostr_kind(KIND_ORDER_REQUEST as u16),
            radroots_nostr_kind(KIND_ORDER_DECISION as u16),
            radroots_nostr_kind(KIND_ORDER_REVISION_PROPOSAL as u16),
            radroots_nostr_kind(KIND_ORDER_REVISION_DECISION as u16),
            radroots_nostr_kind(KIND_ORDER_CANCELLATION as u16),
        ])
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "d", vec![order_id.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order status filter: {error}")))
}

fn event_kind_u32(event: &RadrootsNostrEvent) -> u32 {
    u32::from(event.kind.as_u16())
}

fn order_evidence_from_relay_events(events: &[RadrootsNostrEvent]) -> Vec<SdkRadrootsNostrEvent> {
    events.iter().map(radroots_event_from_nostr).collect()
}

fn validate_scaffold_args(args: &OrderDraftCreateArgs) -> Result<(), RuntimeError> {
    match (normalize_optional(args.bin_id.as_deref()), args.bin_count) {
        (None, Some(_)) => Err(RuntimeError::Config(
            "`--qty` requires `--bin` when creating an trade draft".to_owned(),
        )),
        (Some(_), Some(0)) => Err(RuntimeError::Config(
            "`--qty` must be greater than zero".to_owned(),
        )),
        (Some(_), None) | (Some(_), Some(_)) | (None, None) => Ok(()),
    }
}

fn resolve_order_listing(
    config: &RuntimeConfig,
    listing_lookup: Option<&str>,
    explicit_listing_addr: Option<&str>,
) -> Result<Option<ResolvedOrderListing>, RuntimeError> {
    if let Some(listing_addr) = explicit_listing_addr {
        let parsed = parse_listing_addr(listing_addr).map_err(|error| {
            RuntimeError::Config(format!("explicit listing_addr is invalid: {error}"))
        })?;
        if parsed.kind != KIND_LISTING {
            return Err(RuntimeError::Config(
                "explicit listing_addr must reference a public NIP-99 listing".to_owned(),
            ));
        }
        let replica_listing_event_id =
            resolve_active_listing_event_id(config, listing_addr, &parsed)?;
        let shared_provenance = resolve_shared_signed_listing_provenance(
            config,
            listing_addr,
            replica_listing_event_id.as_deref(),
        )?;
        let listing_event_id = replica_listing_event_id
            .or_else(|| {
                shared_provenance
                    .as_ref()
                    .map(|provenance| provenance.event_id.clone())
            })
            .unwrap_or_default();
        let listing_relays = listing_provenance_relays(
            config,
            listing_event_id.as_str(),
            shared_provenance.as_ref(),
        )?;
        let economics_product = resolve_trade_product_by_listing_addr(config, listing_addr)?;
        return Ok(Some(ResolvedOrderListing {
            listing_addr: listing_addr.to_owned(),
            listing_event_id,
            listing_relays,
            seller_pubkey: parsed.seller_pubkey,
            economics_product,
        }));
    }

    let Some(listing_lookup) = listing_lookup else {
        return Ok(None);
    };

    if !config.local.replica_db_path.exists() {
        return Err(RuntimeError::Config(format!(
            "trade listing lookup `{listing_lookup}` requires local market data; run `radroots store init` and `radroots market refresh` before creating a trade from a listing"
        )));
    }

    let db = ReplicaSql::new(SqliteExecutor::open(&config.local.replica_db_path)?);
    let rows = db.trade_product_lookup(listing_lookup)?;
    match rows.len() {
        0 => Err(RuntimeError::Config(format!(
            "listing `{listing_lookup}` is not available in the local replica; run `radroots market refresh` or pass `--listing-addr`"
        ))),
        1 => {
            let row = rows.into_iter().next().expect("one row");
            let economics_product = ResolvedOrderEconomicsProduct::from_summary(&row);
            let listing_addr = normalize_optional(row.listing_addr.as_deref()).ok_or_else(|| {
                RuntimeError::Config(format!(
                    "listing `{listing_lookup}` is missing a canonical listing address; run `radroots market refresh` or pass `--listing-addr`"
                ))
            })?;
            let parsed = parse_listing_addr(listing_addr.as_str()).map_err(|error| {
                RuntimeError::Config(format!(
                    "listing `{listing_lookup}` has invalid listing_addr: {error}; run `radroots market refresh` or pass `--listing-addr`"
                ))
            })?;
            if parsed.kind != KIND_LISTING {
                return Err(RuntimeError::Config(format!(
                    "listing `{listing_lookup}` listing_addr must reference a public NIP-99 listing; run `radroots market refresh` or pass `--listing-addr`"
                )));
            }

            let listing_event_id = resolve_active_listing_event_id(
                config,
                listing_addr.as_str(),
                &parsed,
            )?
            .ok_or_else(|| {
                RuntimeError::Config(format!(
                    "listing `{listing_lookup}` is missing the latest listing event pointer; run `radroots market refresh` before creating a trade from this listing"
                ))
            })?;
            let shared_provenance = resolve_shared_signed_listing_provenance(
                config,
                listing_addr.as_str(),
                Some(listing_event_id.as_str()),
            )?;
            let listing_relays = listing_provenance_relays(
                config,
                listing_event_id.as_str(),
                shared_provenance.as_ref(),
            )?;

            Ok(Some(ResolvedOrderListing {
                listing_addr,
                listing_event_id,
                listing_relays,
                seller_pubkey: parsed.seller_pubkey,
                economics_product: Some(economics_product),
            }))
        }
        count => Err(RuntimeError::Config(format!(
            "listing lookup `{listing_lookup}` matched {count} local listings; use a unique product key or pass `--listing-addr`"
        ))),
    }
}

fn resolve_trade_product_by_listing_addr(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Option<ResolvedOrderEconomicsProduct>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(None);
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let product_rows = trade_product::find_many(
        &executor,
        &ITradeProductFindMany {
            filter: Some(trade_product_listing_addr_filter(listing_addr)),
        },
    )
    .map_err(|error| RuntimeError::Config(format!("resolve listing product state: {error:?}")))?
    .results;

    match product_rows.len() {
        0 => Ok(None),
        1 => Ok(product_rows
            .into_iter()
            .next()
            .map(ResolvedOrderEconomicsProduct::from_product)),
        count => Err(RuntimeError::Config(format!(
            "listing address `{listing_addr}` matched {count} active local listing rows"
        ))),
    }
}

fn resolve_active_listing_event_id(
    config: &RuntimeConfig,
    listing_addr: &str,
    parsed: &ParsedListingAddress,
) -> Result<Option<String>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(None);
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let product_rows = trade_product::find_many(
        &executor,
        &ITradeProductFindMany {
            filter: Some(trade_product_listing_addr_filter(listing_addr)),
        },
    )
    .map_err(|error| RuntimeError::Config(format!("resolve listing product state: {error:?}")))?
    .results;

    match product_rows.len() {
        0 => return Ok(None),
        1 => {}
        count => {
            return Err(RuntimeError::Config(format!(
                "listing address `{listing_addr}` matched {count} active local listing rows"
            )));
        }
    }

    let key = format!(
        "{}:{}:{}",
        parsed.kind, parsed.seller_pubkey, parsed.listing_id
    );
    let state = nostr_event_head::find_one(
        &executor,
        &INostrEventHeadFindOne::On(INostrEventHeadFindOneArgs {
            on: NostrEventHeadQueryBindValues::Key { key },
        }),
    )
    .map_err(|error| RuntimeError::Config(format!("resolve listing event state: {error:?}")))?
    .result;

    let Some(state) = state else {
        return Ok(None);
    };
    if !is_valid_event_id(state.last_event_id.as_str()) {
        return Err(RuntimeError::Config(format!(
            "listing address `{listing_addr}` has invalid latest listing event id in local replica"
        )));
    }

    Ok(Some(state.last_event_id))
}

#[derive(Debug, Clone)]
struct SharedListingProvenance {
    event_id: String,
    relays: Vec<String>,
}

fn listing_provenance_relays(
    config: &RuntimeConfig,
    listing_event_id: &str,
    shared_provenance: Option<&SharedListingProvenance>,
) -> Result<Vec<String>, RuntimeError> {
    let mut relays = Vec::<String>::new();
    if let Some(provenance) = shared_provenance
        && provenance.event_id == listing_event_id
    {
        relays.extend(provenance.relays.iter().cloned());
    }
    relays.extend(relay_provenance_relays_for_scope(
        config,
        RelayIngestScope::MarketRefresh,
    )?);
    normalize_listing_relay_set(relays)
        .map_err(|error| RuntimeError::Config(format!("listing provenance relays: {error}")))
}

fn resolve_shared_signed_listing_provenance(
    config: &RuntimeConfig,
    listing_addr: &str,
    listing_event_id: Option<&str>,
) -> Result<Option<SharedListingProvenance>, RuntimeError> {
    let mut candidates = list_shared_records_latest(config, ORDER_APP_RECORD_LIST_LIMIT)?
        .into_iter()
        .filter(|record| record.family == LocalRecordFamily::SignedEvent)
        .filter(|record| record.status == LocalRecordStatus::Published)
        .filter(|record| record.event_kind == Some(i64::from(KIND_LISTING)))
        .filter(|record| record.listing_addr.as_deref() == Some(listing_addr))
        .filter(|record| {
            listing_event_id.is_none() || record.event_id.as_deref() == listing_event_id
        })
        .filter_map(|record| {
            let event_id = record.event_id?;
            if !is_valid_event_id(event_id.as_str()) {
                return None;
            }
            let delivery = record.relay_delivery_json.as_ref()?;
            let evidence = RelayDeliveryEvidence::from_json_value(delivery).ok()?;
            let relays = listing_provenance_relays_from_delivery_evidence(evidence).ok()?;
            if relays.is_empty() {
                return None;
            }
            Some(SharedListingProvenance { event_id, relays })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    candidates.dedup_by(|left, right| left.event_id == right.event_id);
    if candidates.len() > 1 && listing_event_id.is_none() {
        return Err(RuntimeError::Config(format!(
            "listing address `{listing_addr}` has multiple published shared local listing events; run `radroots market refresh` or pass a current listing event id source"
        )));
    }
    Ok(candidates.pop())
}

fn listing_provenance_relays_from_delivery_evidence(
    evidence: RelayDeliveryEvidence,
) -> Result<Vec<String>, String> {
    let relays = match evidence.state {
        RelayDeliveryState::Acknowledged => evidence.acknowledged_relays,
        RelayDeliveryState::Observed => evidence.observed_relays,
        RelayDeliveryState::Pending | RelayDeliveryState::Failed => Vec::new(),
    };
    normalize_listing_relay_set(relays)
}

fn trade_product_listing_addr_filter(listing_addr: &str) -> ITradeProductFieldsFilter {
    ITradeProductFieldsFilter {
        id: None,
        created_at: None,
        updated_at: None,
        key: None,
        category: None,
        title: None,
        summary: None,
        process: None,
        lot: None,
        profile: None,
        year: None,
        qty_amt: None,
        qty_amt_exact: None,
        qty_unit: None,
        qty_label: None,
        qty_avail: None,
        price_amt: None,
        price_amt_exact: None,
        price_currency: None,
        price_qty_amt: None,
        price_qty_amt_exact: None,
        price_qty_unit: None,
        listing_addr: Some(listing_addr.to_owned()),
        primary_bin_id: None,
        verified_primary_bin_id: None,
        notes: None,
    }
}

fn order_economics_from_resolved_listing(
    order_id: &str,
    resolved_listing: Option<&ResolvedOrderListing>,
    items: &[OrderDraftItem],
    adjustments: &[crate::cli::global::OrderDraftAdjustmentArgs],
) -> Result<Option<RadrootsOrderEconomics>, RuntimeError> {
    let Some(listing) = resolved_listing else {
        return Ok(None);
    };
    let Some(product) = listing.economics_product.as_ref() else {
        return Ok(None);
    };
    let Some(primary_bin_id) = product.primary_bin_id.as_deref().and_then(non_empty_ref) else {
        return Ok(None);
    };
    let Some(verified_primary_bin_id) = product
        .verified_primary_bin_id
        .as_deref()
        .and_then(non_empty_ref)
    else {
        return Err(RuntimeError::Config(format!(
            "listing_primary_bin_invalid: listing `{}` primary bin `{primary_bin_id}` is not verified in the current local replica",
            listing.listing_addr
        )));
    };
    if verified_primary_bin_id != primary_bin_id {
        return Err(RuntimeError::Config(format!(
            "listing_primary_bin_invalid: listing `{}` primary bin `{primary_bin_id}` does not match verified primary bin `{verified_primary_bin_id}` in the current local replica",
            listing.listing_addr
        )));
    }
    if items.is_empty()
        || items
            .iter()
            .any(|item| item.bin_id.as_str() != primary_bin_id)
    {
        return Ok(None);
    }

    let currency = parse_economics_currency(product.price_currency.as_str(), "price_currency")?;
    let quantity_amount =
        exact_non_negative_decimal(product.qty_amt_exact.as_deref(), "qty_amt_exact")?;
    let quantity_unit = parse_economics_unit(product.qty_unit.as_str(), "qty_unit")?;
    let price_amount =
        exact_non_negative_decimal(product.price_amt_exact.as_deref(), "price_amt_exact")?;
    let price_quantity_amount = exact_positive_decimal(
        product.price_qty_amt_exact.as_deref(),
        "price_qty_amt_exact",
    )?;
    let price_unit = parse_economics_unit(product.price_qty_unit.as_str(), "price_qty_unit")?;
    let quantity_unit_in_price_units =
        convert_unit_decimal(RadrootsCoreDecimal::ONE, quantity_unit, price_unit).map_err(
            |error| {
                RuntimeError::Config(format!(
                    "listing quantity unit and price unit are incompatible: {error}"
                ))
            },
        )?;
    let unit_price_amount = (price_amount / price_quantity_amount) * quantity_unit_in_price_units;

    let mut subtotal_amount = RadrootsCoreDecimal::ZERO;
    let mut economic_items = Vec::with_capacity(items.len());
    for item in items {
        let line_amount =
            unit_price_amount * quantity_amount * RadrootsCoreDecimal::from(item.bin_count);
        subtotal_amount = subtotal_amount + line_amount;
        economic_items.push(RadrootsOrderEconomicItem {
            bin_id: protocol_inventory_bin_id(item.bin_id.as_str(), "order item bin_id")?,
            bin_count: item.bin_count,
            quantity_amount,
            quantity_unit,
            unit_price_amount,
            unit_price_currency: currency,
            line_subtotal: RadrootsCoreMoney::new(line_amount, currency),
        });
    }

    let subtotal = RadrootsCoreMoney::new(subtotal_amount, currency);
    let discounts = listing_discount_lines_from_product(
        product,
        &subtotal,
        items,
        quantity_amount,
        quantity_unit,
    )?;
    let adjustments = basket_adjustment_lines(adjustments)?;
    let zero = RadrootsCoreMoney::zero(currency);
    let mut economics = RadrootsOrderEconomics {
        quote_id: protocol_quote_id(format!("quote_{order_id}").as_str(), "quote_id")?,
        quote_version: 1,
        pricing_basis: RadrootsOrderPricingBasis::ListingEvent,
        currency,
        items: economic_items,
        discounts,
        adjustments,
        subtotal: subtotal.clone(),
        discount_total: zero.clone(),
        adjustment_total: zero,
        total: subtotal,
    };
    economics.canonicalize();
    economics
        .validate()
        .map_err(|error| RuntimeError::Config(format!("build order economics: {error}")))?;
    Ok(Some(economics))
}

fn listing_discount_lines_from_product(
    product: &ResolvedOrderEconomicsProduct,
    subtotal: &RadrootsCoreMoney,
    items: &[OrderDraftItem],
    quantity_amount: RadrootsCoreDecimal,
    quantity_unit: RadrootsCoreUnit,
) -> Result<Vec<RadrootsOrderEconomicLine>, RuntimeError> {
    let Some(notes) = product.notes.as_deref().and_then(non_empty_ref) else {
        return Ok(Vec::new());
    };
    let parsed = serde_json::from_str::<ResolvedTradeProductNotes>(notes).map_err(|error| {
        RuntimeError::Config(format!("listing discount metadata is invalid: {error}"))
    })?;
    let mut lines = Vec::new();
    for (index, discount) in parsed.listing_discounts.iter().enumerate() {
        if !discount_applies(discount, items, quantity_amount, quantity_unit)? {
            continue;
        }
        let amount = listing_discount_amount(discount, subtotal, items)?;
        if amount.is_zero() {
            return Err(RuntimeError::Config(
                "listing discount amount must be greater than zero".to_owned(),
            ));
        }
        lines.push(RadrootsOrderEconomicLine {
            id: format!("listing_discount_{}", index + 1),
            kind: RadrootsOrderEconomicLineKind::ListingDiscount,
            actor: RadrootsOrderEconomicActor::Seller,
            effect: RadrootsOrderEconomicEffect::Decrease,
            amount,
            reason: format!("listing discount {}", index + 1),
        });
    }
    Ok(lines)
}

fn discount_applies(
    discount: &RadrootsCoreDiscount,
    items: &[OrderDraftItem],
    quantity_amount: RadrootsCoreDecimal,
    quantity_unit: RadrootsCoreUnit,
) -> Result<bool, RuntimeError> {
    match &discount.threshold {
        RadrootsCoreDiscountThreshold::BinCount { bin_id, min } => Ok(items
            .iter()
            .any(|item| item.bin_id == *bin_id && item.bin_count >= *min)),
        RadrootsCoreDiscountThreshold::OrderQuantity { min } => {
            let requested = items.iter().fold(RadrootsCoreDecimal::ZERO, |total, item| {
                total + quantity_amount * RadrootsCoreDecimal::from(item.bin_count)
            });
            let converted =
                convert_unit_decimal(requested, quantity_unit, min.unit).map_err(|error| {
                    RuntimeError::Config(format!(
                        "listing discount quantity threshold is incompatible: {error}"
                    ))
                })?;
            Ok(converted >= min.amount)
        }
    }
}

fn listing_discount_amount(
    discount: &RadrootsCoreDiscount,
    subtotal: &RadrootsCoreMoney,
    items: &[OrderDraftItem],
) -> Result<RadrootsCoreMoney, RuntimeError> {
    match &discount.value {
        RadrootsCoreDiscountValue::Percent(percent) => Ok(percent.of_money(subtotal)),
        RadrootsCoreDiscountValue::MoneyPerBin(money) => {
            if money.currency != subtotal.currency {
                return Err(RuntimeError::Config(
                    "listing discount currency must match listing price currency".to_owned(),
                ));
            }
            let multiplier = match &discount.scope {
                RadrootsCoreDiscountScope::Bin => {
                    items.iter().map(|item| item.bin_count).sum::<u32>().max(1)
                }
                RadrootsCoreDiscountScope::OrderTotal => 1,
            };
            Ok(money.mul_decimal(RadrootsCoreDecimal::from(multiplier)))
        }
    }
}

fn basket_adjustment_lines(
    adjustments: &[crate::cli::global::OrderDraftAdjustmentArgs],
) -> Result<Vec<RadrootsOrderEconomicLine>, RuntimeError> {
    adjustments
        .iter()
        .map(|adjustment| {
            let currency =
                parse_economics_currency(adjustment.currency.as_str(), "adjustment_currency")?;
            let amount = decimal_from_adjustment(adjustment.amount.as_str(), "adjustment_amount")?;
            if amount.is_zero() {
                return Err(RuntimeError::Config(
                    "basket adjustment amount must be greater than zero".to_owned(),
                ));
            }
            let effect = match adjustment.effect.as_str() {
                "increase" => RadrootsOrderEconomicEffect::Increase,
                "decrease" => RadrootsOrderEconomicEffect::Decrease,
                other => {
                    return Err(RuntimeError::Config(format!(
                        "basket adjustment effect `{other}` is invalid"
                    )));
                }
            };
            if adjustment.id.trim().is_empty() {
                return Err(RuntimeError::Config(
                    "basket adjustment id must not be empty".to_owned(),
                ));
            }
            if adjustment.reason.trim().is_empty() {
                return Err(RuntimeError::Config(
                    "basket adjustment reason must not be empty".to_owned(),
                ));
            }
            Ok(RadrootsOrderEconomicLine {
                id: adjustment.id.trim().to_owned(),
                kind: RadrootsOrderEconomicLineKind::BasketAdjustment,
                actor: RadrootsOrderEconomicActor::Buyer,
                effect,
                amount: RadrootsCoreMoney::new(amount, currency),
                reason: adjustment.reason.trim().to_owned(),
            })
        })
        .collect()
}

fn parse_economics_currency(
    value: &str,
    field: &str,
) -> Result<RadrootsCoreCurrency, RuntimeError> {
    value
        .parse::<RadrootsCoreCurrency>()
        .map_err(|error| RuntimeError::Config(format!("listing {field} is invalid: {error}")))
}

fn parse_economics_unit(value: &str, field: &str) -> Result<RadrootsCoreUnit, RuntimeError> {
    value
        .parse::<RadrootsCoreUnit>()
        .map_err(|error| RuntimeError::Config(format!("listing {field} is invalid: {error}")))
}

fn exact_non_negative_decimal(
    value: Option<&str>,
    field: &str,
) -> Result<RadrootsCoreDecimal, RuntimeError> {
    let parsed = exact_decimal(value, field)?;
    if parsed.is_sign_negative() {
        return Err(RuntimeError::Config(format!(
            "listing {field} must be non-negative"
        )));
    }
    Ok(parsed)
}

fn exact_positive_decimal(
    value: Option<&str>,
    field: &str,
) -> Result<RadrootsCoreDecimal, RuntimeError> {
    let parsed = exact_non_negative_decimal(value, field)?;
    if parsed.is_zero() {
        return Err(RuntimeError::Config(format!(
            "listing {field} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn exact_decimal(value: Option<&str>, field: &str) -> Result<RadrootsCoreDecimal, RuntimeError> {
    let Some(value) = value.and_then(non_empty_ref) else {
        return Err(RuntimeError::Config(format!(
            "listing {field} exact source is missing"
        )));
    };
    value
        .parse::<RadrootsCoreDecimal>()
        .map_err(|error| RuntimeError::Config(format!("listing {field} is invalid: {error}")))
}

fn decimal_from_adjustment(value: &str, field: &str) -> Result<RadrootsCoreDecimal, RuntimeError> {
    let parsed = value
        .trim()
        .parse::<RadrootsCoreDecimal>()
        .map_err(|error| RuntimeError::Config(format!("basket {field} is invalid: {error}")))?;
    if parsed.is_sign_negative() {
        return Err(RuntimeError::Config(format!(
            "basket {field} must be non-negative"
        )));
    }
    Ok(parsed)
}

fn view_from_loaded(
    config: &RuntimeConfig,
    loaded: LoadedOrderDraft,
) -> Result<OrderGetView, RuntimeError> {
    view_from_loaded_with_source_issues(config, loaded, &[])
}

fn view_from_loaded_with_source_issues(
    config: &RuntimeConfig,
    loaded: LoadedOrderDraft,
    source_issues: &[OrderIssueView],
) -> Result<OrderGetView, RuntimeError> {
    let OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        listing_event_id,
        seller_pubkey,
        buyer_custody,
        buyer_write_capable,
        issues,
    } = inspect_document_with_source_issues(config, &loaded.document, source_issues)?;

    let actions = actions_for_document(&loaded.document, loaded.file.as_path(), issues.as_slice());

    Ok(OrderGetView {
        state,
        source: ORDER_SOURCE.to_owned(),
        lookup: loaded.document.order.order_id.clone(),
        order_id: Some(loaded.document.order.order_id.clone()),
        file: Some(loaded.file.display().to_string()),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr,
        listing_event_id,
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody,
        buyer_write_capable,
        seller_pubkey,
        ready_for_submit,
        items: loaded
            .document
            .order
            .items
            .iter()
            .map(|item| OrderDraftItemView {
                bin_id: item.bin_id.clone(),
                bin_count: item.bin_count,
            })
            .collect(),
        economics: loaded.document.order.economics.clone(),
        updated_at_unix: Some(loaded.updated_at_unix),
        job: None,
        workflow: None,
        reason: None,
        issues,
        actions,
    })
}

fn summary_from_loaded(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
) -> Result<OrderSummaryView, RuntimeError> {
    summary_from_loaded_with_source_issues(config, loaded, &[])
}

fn summary_from_loaded_with_source_issues(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    source_issues: &[OrderIssueView],
) -> Result<OrderSummaryView, RuntimeError> {
    let OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        listing_event_id,
        seller_pubkey: _,
        buyer_custody,
        buyer_write_capable,
        issues,
    } = inspect_document_with_source_issues(config, &loaded.document, source_issues)?;

    Ok(OrderSummaryView {
        id: loaded.document.order.order_id.clone(),
        state,
        ready_for_submit,
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr,
        listing_event_id,
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody,
        buyer_write_capable,
        item_count: loaded.document.order.items.len(),
        economics: loaded.document.order.economics.clone(),
        updated_at_unix: loaded.updated_at_unix,
        job: None,
        issues,
    })
}

fn summary_for_invalid_file(path: &Path, reason: String) -> OrderSummaryView {
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_owned();
    OrderSummaryView {
        id,
        state: "error".to_owned(),
        ready_for_submit: false,
        file: path.display().to_string(),
        listing_lookup: None,
        listing_addr: None,
        listing_event_id: None,
        listing_relays: Vec::new(),
        buyer_account_id: None,
        buyer_pubkey: None,
        buyer_actor_source: None,
        buyer_custody: None,
        buyer_write_capable: None,
        item_count: 0,
        economics: None,
        updated_at_unix: modified_unix(path).unwrap_or_default(),
        job: None,
        issues: vec![issue_with_code("invalid_order_draft", "draft", reason)],
    }
}

fn app_order_local_records(config: &RuntimeConfig) -> Result<Vec<LocalEventRecord>, RuntimeError> {
    let mut app_records = Vec::new();
    let mut before_cursor = None::<(i64, i64)>;
    loop {
        let shared_records = if let Some((before_change_seq, before_seq)) = before_cursor {
            list_shared_records_before(
                config,
                before_change_seq,
                before_seq,
                ORDER_APP_RECORD_LIST_LIMIT,
            )?
        } else {
            list_shared_records_latest(config, ORDER_APP_RECORD_LIST_LIMIT)?
        };
        let Some(next_cursor) = shared_records
            .last()
            .map(|record| (record.change_seq, record.seq))
        else {
            break;
        };
        let has_more = shared_records.len() == ORDER_APP_RECORD_LIST_LIMIT as usize;
        app_records.extend(shared_records.into_iter().filter(is_app_order_local_record));
        if !has_more {
            break;
        }
        before_cursor = Some(next_cursor);
    }
    Ok(app_records)
}

fn is_app_order_local_record(record: &LocalEventRecord) -> bool {
    record.source_runtime == SourceRuntime::App
        && record.family == LocalRecordFamily::LocalWork
        && record.status == LocalRecordStatus::LocalSaved
        && local_record_kind(record).as_deref() == Some(BUYER_ORDER_REQUEST_LOCAL_WORK_RECORD_KIND)
}

fn current_app_order_record_entries(
    mut records: Vec<LocalEventRecord>,
) -> Vec<AppOrderRecordListEntry> {
    records.sort_by(|left, right| {
        right
            .change_seq
            .cmp(&left.change_seq)
            .then_with(|| right.seq.cmp(&left.seq))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });

    let mut entries = Vec::<AppOrderRecordListEntry>::new();
    let mut seen = HashMap::<String, usize>::new();
    for record in records {
        let key = app_order_record_current_key(&record);
        if let Some(index) = seen.get(&key).copied() {
            entries[index].superseded_count += 1;
        } else {
            seen.insert(key, entries.len());
            entries.push(AppOrderRecordListEntry {
                record,
                superseded_count: 0,
            });
        }
    }
    entries
}

fn current_app_order_record_for(
    config: &RuntimeConfig,
    record: &LocalEventRecord,
) -> Result<Option<LocalEventRecord>, RuntimeError> {
    let key = app_order_record_current_key(record);
    Ok(app_order_local_records(config)?
        .into_iter()
        .filter(|candidate| app_order_record_current_key(candidate) == key)
        .max_by(|left, right| {
            left.change_seq
                .cmp(&right.change_seq)
                .then_with(|| left.seq.cmp(&right.seq))
        }))
}

fn app_order_conflicting_record_ids_for(
    config: &RuntimeConfig,
    record: &LocalEventRecord,
) -> Result<Vec<String>, RuntimeError> {
    if app_order_record_order_id(record).is_none() {
        return Ok(Vec::new());
    }
    let key = app_order_record_current_key(record);
    let mut record_ids = app_order_local_records(config)?
        .into_iter()
        .filter(|candidate| candidate.record_id != record.record_id)
        .filter(|candidate| app_order_record_current_key(candidate) == key)
        .map(|candidate| candidate.record_id)
        .collect::<Vec<_>>();
    record_ids.sort();
    record_ids.dedup();
    Ok(record_ids)
}

fn load_app_order_record_for_lookup(
    config: &RuntimeConfig,
    lookup: &str,
) -> Result<Option<LoadedAppOrderRecord>, RuntimeError> {
    if let Some(record) = get_shared_record(config, lookup)?
        && is_app_order_local_record(&record)
    {
        return load_app_order_record_from_record(config, record).map(Some);
    }
    for entry in current_app_order_record_entries(app_order_local_records(config)?) {
        if app_order_record_order_id(&entry.record).as_deref() == Some(lookup) {
            return load_app_order_record_from_record(config, entry.record).map(Some);
        }
    }
    Ok(None)
}

fn load_app_order_record_from_record(
    config: &RuntimeConfig,
    record: LocalEventRecord,
) -> Result<LoadedAppOrderRecord, RuntimeError> {
    let mut source_issues = app_order_record_source_issues(config, &record)?;
    let payload = record.local_work_json.clone().unwrap_or(Value::Null);
    let document = match payload.get("document").cloned() {
        Some(value) => match serde_json::from_value::<OrderDraftDocument>(value) {
            Ok(document) => document,
            Err(error) => {
                source_issues.push(issue_with_code(
                    "invalid_app_order_record",
                    "document",
                    format!("app-authored order document cannot be decoded: {error}"),
                ));
                placeholder_app_order_document(&record)
            }
        },
        None => {
            source_issues.push(issue_with_code(
                "invalid_app_order_record",
                "document",
                "app-authored trade record is missing document",
            ));
            placeholder_app_order_document(&record)
        }
    };
    let loaded = LoadedOrderDraft {
        file: PathBuf::from(format!("shared-local-events/{}", record.record_id)),
        updated_at_unix: u64::try_from(record.updated_at_ms / 1000).unwrap_or_default(),
        document,
    };
    source_issues.extend(app_order_signed_evidence_issues(config, &loaded)?);

    Ok(LoadedAppOrderRecord {
        loaded,
        record,
        source_issues,
    })
}

fn app_order_record_source_issues(
    config: &RuntimeConfig,
    record: &LocalEventRecord,
) -> Result<Vec<OrderIssueView>, RuntimeError> {
    let mut issues = Vec::new();
    if record.source_runtime != SourceRuntime::App {
        issues.push(issue_with_code(
            "app_order_unsupported",
            "source_runtime",
            "trade record must come from radroots_studio_app",
        ));
    }
    if record.family != LocalRecordFamily::LocalWork {
        issues.push(issue_with_code(
            "app_order_unsupported",
            "family",
            "trade record must be shared local work",
        ));
    }
    if record.status != LocalRecordStatus::LocalSaved {
        issues.push(issue_with_code(
            "app_order_unsupported",
            "status",
            format!(
                "trade record status `{}` is not consumable as local saved work",
                record.status.as_str()
            ),
        ));
    }
    let Some(payload) = record.local_work_json.as_ref() else {
        issues.push(issue_with_code(
            "invalid_app_order_record",
            "local_work_json",
            "app-authored trade record is missing local work payload",
        ));
        return Ok(issues);
    };
    let current = payload["currentness"]["current"].as_bool() == Some(true);
    if !current {
        issues.push(issue_with_code(
            "app_order_stale",
            "currentness.current",
            "app-authored trade record is not marked current",
        ));
    }
    if payload["currentness"]["record_id"].as_str() != Some(record.record_id.as_str()) {
        issues.push(issue_with_code(
            "invalid_app_order_record",
            "currentness.record_id",
            "app-authored trade record currentness id does not match the shared record id",
        ));
    }
    if current {
        match validate_supported_buyer_order_request_local_work_payload(payload) {
            Ok(_) => {}
            Err(error) => {
                let support_state = payload["support_status"]["state"].as_str();
                let support_issues = payload["support_status"]["issues"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if support_state == Some("unsupported") {
                    issues.push(issue_with_code(
                        "app_order_unsupported",
                        "support_status.state",
                        "app-authored trade record is not marked supported",
                    ));
                    for support_issue in support_issues {
                        if let Some(support_issue) = support_issue.as_str() {
                            issues.push(issue_with_code(
                                "app_order_unsupported",
                                "support_status.issues",
                                format!("app order support issue: {support_issue}"),
                            ));
                        }
                    }
                } else {
                    issues.push(issue_with_code(
                        "invalid_app_order_record",
                        "local_work_json",
                        error.to_string(),
                    ));
                }
            }
        }
    }
    if let Some(current_record) = current_app_order_record_for(config, record)?
        && current_record.record_id != record.record_id
    {
        issues.push(issue_with_code(
            "app_order_stale",
            "record_id",
            format!(
                "app-authored local trade record `{}` was superseded by `{}`",
                record.record_id, current_record.record_id
            ),
        ));
    }
    let conflicting_record_ids = app_order_conflicting_record_ids_for(config, record)?;
    if !conflicting_record_ids.is_empty() {
        issues.push(issue_with_code(
            "app_order_conflict",
            "order_id",
            format!(
                "app-authored order id conflicts with other shared records: {}",
                conflicting_record_ids.join(", ")
            ),
        ));
    }
    Ok(issues)
}

fn app_order_signed_evidence_issues(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
) -> Result<Vec<OrderIssueView>, RuntimeError> {
    let order_id = loaded.document.order.order_id.as_str();
    let candidate_records = visible_signed_order_request_records(config, order_id)?;
    if candidate_records.is_empty() {
        return Ok(Vec::new());
    }

    let expected_payload = match canonical_order_request_payload_from_loaded(
        loaded,
        loaded.document.order.buyer_pubkey.as_str(),
    ) {
        Ok(payload) => payload,
        Err(error) => {
            let event_ids = candidate_records
                .iter()
                .map(signed_record_event_id)
                .collect::<Vec<_>>();
            return Ok(vec![issue_with_events(
                APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE,
                "signed_event",
                format!(
                    "signed order request evidence cannot be compared with local work: {error}"
                ),
                event_ids,
            )]);
        }
    };

    let mut submitted_event_ids = Vec::new();
    let mut conflict_issues = Vec::new();
    for record in candidate_records {
        let event_id = signed_record_event_id(&record);
        match signed_order_request_from_record(&record)
            .and_then(|event| order_submit_request_from_event(&event, loaded))
        {
            Ok(request)
                if order_submit_request_matches_draft(&request, loaded, &expected_payload) =>
            {
                submitted_event_ids.push(request.request_event_id);
            }
            Ok(request) => conflict_issues.push(issue_with_events(
                APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE,
                "signed_event",
                format!(
                    "signed order request event `{}` conflicts with the app-authored local order",
                    request.request_event_id
                ),
                vec![request.request_event_id],
            )),
            Err(error) => conflict_issues.push(issue_with_events(
                APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE,
                "signed_event",
                format!("signed order request event `{event_id}` cannot be validated: {error}"),
                vec![event_id],
            )),
        }
    }

    conflict_issues.sort_by(|left, right| {
        left.event_ids
            .cmp(&right.event_ids)
            .then_with(|| left.message.cmp(&right.message))
    });
    if !conflict_issues.is_empty() {
        return Ok(conflict_issues);
    }

    submitted_event_ids.sort();
    submitted_event_ids.dedup();
    if submitted_event_ids.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![issue_with_events(
            APP_ORDER_ALREADY_SUBMITTED_ISSUE,
            "signed_event",
            "app-authored local order already has matching signed order request evidence",
            submitted_event_ids,
        )])
    }
}

fn visible_signed_order_request_records(
    config: &RuntimeConfig,
    order_id: &str,
) -> Result<Vec<LocalEventRecord>, RuntimeError> {
    let mut records = Vec::new();
    let mut before_cursor = None::<(i64, i64)>;
    loop {
        let shared_records = if let Some((before_change_seq, before_seq)) = before_cursor {
            list_shared_records_before(
                config,
                before_change_seq,
                before_seq,
                ORDER_APP_RECORD_LIST_LIMIT,
            )?
        } else {
            list_shared_records_latest(config, ORDER_APP_RECORD_LIST_LIMIT)?
        };
        let Some(next_cursor) = shared_records
            .last()
            .map(|record| (record.change_seq, record.seq))
        else {
            break;
        };
        let has_more = shared_records.len() == ORDER_APP_RECORD_LIST_LIMIT as usize;
        records.extend(
            shared_records
                .into_iter()
                .filter(|record| is_visible_signed_order_request_record(record, order_id)),
        );
        if !has_more {
            break;
        }
        before_cursor = Some(next_cursor);
    }
    Ok(records)
}

fn is_visible_signed_order_request_record(record: &LocalEventRecord, order_id: &str) -> bool {
    record.family == LocalRecordFamily::SignedEvent
        && record.status == LocalRecordStatus::Published
        && record.outbox_status == PublishOutboxStatus::Acknowledged
        && record.event_kind == Some(i64::from(KIND_ORDER_REQUEST))
        && signed_record_tag_values(record, "d")
            .iter()
            .any(|value| value == order_id)
}

fn signed_order_request_from_record(
    record: &LocalEventRecord,
) -> Result<RadrootsNostrEvent, RuntimeError> {
    let raw_event_json = record.raw_event_json.as_ref().ok_or_else(|| {
        RuntimeError::Config(format!(
            "signed event record `{}` is missing raw_event_json",
            record.record_id
        ))
    })?;
    serde_json::from_value::<RadrootsNostrEvent>(raw_event_json.clone()).map_err(|error| {
        RuntimeError::Config(format!(
            "signed event record `{}` raw_event_json cannot be decoded: {error}",
            record.record_id
        ))
    })
}

fn signed_record_tag_values(record: &LocalEventRecord, key: &str) -> Vec<String> {
    record
        .event_tags_json
        .as_ref()
        .or(record
            .raw_event_json
            .as_ref()
            .and_then(|event| event.get("tags")))
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_array)
                .filter_map(|tag| {
                    if tag.first().and_then(Value::as_str) == Some(key) {
                        tag.get(1).and_then(Value::as_str).map(str::to_owned)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn signed_record_event_id(record: &LocalEventRecord) -> String {
    record
        .event_id
        .clone()
        .unwrap_or_else(|| record.record_id.clone())
}

fn source_and_document_issues(
    config: &RuntimeConfig,
    app_order: &LoadedAppOrderRecord,
) -> Result<Vec<OrderIssueView>, RuntimeError> {
    Ok(inspect_document_with_source_issues(
        config,
        &app_order.loaded.document,
        app_order.source_issues.as_slice(),
    )?
    .issues)
}

fn app_order_record_summary(
    config: &RuntimeConfig,
    record: &LocalEventRecord,
    superseded_count: usize,
) -> Result<OrderAppRecordSummaryView, RuntimeError> {
    let record_kind = local_record_kind(record).unwrap_or_else(|| "unknown".to_owned());
    let app_order = load_app_order_record_from_record(config, record.clone())?;
    let issues = source_and_document_issues(config, &app_order)?;
    let exportable = issues.is_empty();
    let reason = issues.first().map(|issue| issue.message.clone());
    let document = &app_order.loaded.document;
    let status = if app_order_issue_present(&issues, APP_ORDER_ALREADY_SUBMITTED_ISSUE) {
        "submitted".to_owned()
    } else if app_order_issue_present(&issues, APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE) {
        "conflict".to_owned()
    } else {
        record.status.as_str().to_owned()
    };
    let actions = if exportable {
        vec![
            format!("radroots trade get {}", document.order.order_id),
            format!("radroots trade app export {}", record.record_id),
            format!(
                "radroots --relay wss://relay.example.com trade submit {}",
                document.order.order_id
            ),
        ]
    } else if app_order_issue_present(&issues, APP_ORDER_ALREADY_SUBMITTED_ISSUE) {
        vec![format!(
            "radroots trade status get {}",
            document.order.order_id
        )]
    } else if app_order_issue_present(&issues, APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE) {
        vec![
            format!("radroots trade status get {}", document.order.order_id),
            "radroots trade app list".to_owned(),
        ]
    } else {
        Vec::new()
    };
    Ok(OrderAppRecordSummaryView {
        record_id: record.record_id.clone(),
        seq: record.seq,
        change_seq: record.change_seq,
        superseded_count,
        record_kind,
        status,
        source_runtime: record.source_runtime.as_str().to_owned(),
        owner_account_id: record.owner_account_id.clone(),
        owner_pubkey: record.owner_pubkey.clone(),
        farm_id: record.farm_id.clone(),
        listing_addr: record
            .listing_addr
            .clone()
            .or_else(|| non_empty_string(app_order.loaded.document.order.listing_addr.clone())),
        listing_relays: order_listing_relays(document),
        order_id: non_empty_string(document.order.order_id.clone()),
        buyer_account_id: buyer_account_id(document),
        buyer_pubkey: non_empty_string(document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(document.order.seller_pubkey.clone()),
        ready_for_submit: exportable,
        exportable,
        reason,
        actions,
    })
}

fn app_order_record_current_key(record: &LocalEventRecord) -> String {
    app_order_record_order_id(record)
        .map(|order_id| format!("order:{order_id}"))
        .unwrap_or_else(|| format!("record:{}", record.record_id))
}

fn app_order_record_order_id(record: &LocalEventRecord) -> Option<String> {
    record
        .local_work_json
        .as_ref()
        .and_then(|payload| payload["document"]["order"]["order_id"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn placeholder_app_order_document(record: &LocalEventRecord) -> OrderDraftDocument {
    OrderDraftDocument {
        version: 0,
        kind: "invalid_app_order_record".to_owned(),
        order: OrderDraft {
            order_id: app_order_record_order_id(record).unwrap_or_else(|| record.record_id.clone()),
            listing_addr: String::new(),
            listing_event_id: String::new(),
            listing_relays: Vec::new(),
            buyer_pubkey: String::new(),
            seller_pubkey: String::new(),
            items: Vec::new(),
            economics: None,
        },
        buyer_actor: OrderDraftBuyerActor {
            account_id: String::new(),
            pubkey: String::new(),
            source: String::new(),
        },
        listing_lookup: None,
    }
}

fn app_order_export_failure_state(issues: &[OrderIssueView]) -> &'static str {
    if issues
        .iter()
        .any(|issue| issue.code == APP_ORDER_ALREADY_SUBMITTED_ISSUE)
    {
        "already_submitted"
    } else if issues.iter().any(|issue| {
        issue.code == "app_order_conflict" || issue.code == APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE
    }) {
        "conflict"
    } else if issues.iter().any(|issue| issue.code == "app_order_stale") {
        "stale"
    } else if issues
        .iter()
        .any(|issue| issue.code == "invalid_app_order_record")
    {
        "invalid"
    } else if issues
        .iter()
        .any(|issue| issue.code == "app_order_unsupported")
    {
        "unsupported"
    } else {
        "invalid"
    }
}

fn app_order_export_failure_actions(
    document: &OrderDraftDocument,
    issues: &[OrderIssueView],
) -> Vec<String> {
    if app_order_issue_present(issues, APP_ORDER_ALREADY_SUBMITTED_ISSUE) {
        vec![format!(
            "radroots trade status get {}",
            document.order.order_id
        )]
    } else if app_order_issue_present(issues, APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE) {
        vec![
            format!("radroots trade status get {}", document.order.order_id),
            "radroots trade app list".to_owned(),
        ]
    } else {
        vec!["radroots trade app list".to_owned()]
    }
}

fn order_export_output_path(
    config: &RuntimeConfig,
    output: Option<&PathBuf>,
    order_id: &str,
) -> PathBuf {
    output
        .cloned()
        .unwrap_or_else(|| drafts_dir(config).join(format!("{order_id}.toml")))
}

fn validate_order_export_output_target(output_path: &Path) -> Result<(), RuntimeError> {
    if output_path.exists() {
        return Err(RuntimeError::Config(format!(
            "trade draft output {} must not already exist",
            output_path.display()
        )));
    }
    if let Some(parent) = output_path.parent() {
        if parent.exists() && !parent.is_dir() {
            return Err(RuntimeError::Config(format!(
                "trade draft parent {} is not a directory",
                parent.display()
            )));
        }
    }
    Ok(())
}

fn local_record_kind(record: &LocalEventRecord) -> Option<String> {
    record
        .local_work_json
        .as_ref()
        .and_then(|payload| payload.get("record_kind"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn inspect_document(
    config: &RuntimeConfig,
    document: &OrderDraftDocument,
) -> Result<OrderInspection, RuntimeError> {
    inspect_document_with_source_issues(config, document, &[])
}

fn inspect_document_with_source_issues(
    config: &RuntimeConfig,
    document: &OrderDraftDocument,
    source_issues: &[OrderIssueView],
) -> Result<OrderInspection, RuntimeError> {
    let listing_addr = non_empty_string(document.order.listing_addr.clone());
    let listing_event_id = non_empty_string(document.order.listing_event_id.clone());
    let parsed_listing_addr = listing_addr
        .as_deref()
        .and_then(|value| parse_listing_addr(value).ok());
    let seller_pubkey = non_empty_string(document.order.seller_pubkey.clone()).or_else(|| {
        parsed_listing_addr
            .as_ref()
            .map(|listing| listing.seller_pubkey.clone())
    });
    let mut issues = collect_issues(document);
    let buyer_readiness = inspect_buyer_actor_readiness(config, document)?;
    issues.extend(buyer_readiness.issues);
    issues.extend(source_issues.iter().cloned());
    let ready_for_submit = issues.is_empty();
    let state = if app_order_issue_present(&issues, APP_ORDER_ALREADY_SUBMITTED_ISSUE) {
        "submitted".to_owned()
    } else if app_order_issue_present(&issues, APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE) {
        "conflict".to_owned()
    } else if ready_for_submit {
        "ready".to_owned()
    } else {
        "draft".to_owned()
    };

    Ok(OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        listing_event_id,
        seller_pubkey,
        buyer_custody: buyer_readiness
            .account
            .as_ref()
            .map(|account| account.custody.as_str().to_owned()),
        buyer_write_capable: buyer_readiness
            .account
            .as_ref()
            .map(|account| account.write_capable),
        issues,
    })
}

#[derive(Debug, Clone)]
struct OrderBuyerActorReadiness {
    account: Option<account::AccountRecordView>,
    issues: Vec<OrderIssueView>,
}

fn inspect_buyer_actor_readiness(
    config: &RuntimeConfig,
    document: &OrderDraftDocument,
) -> Result<OrderBuyerActorReadiness, RuntimeError> {
    let account_id = document.buyer_actor.account_id.trim();
    let buyer_pubkey = document.buyer_actor.pubkey.trim();
    if account_id.is_empty() || buyer_pubkey.is_empty() {
        return Ok(OrderBuyerActorReadiness {
            account: None,
            issues: Vec::new(),
        });
    }

    let snapshot = account::snapshot(config)?;
    let Some(account) = snapshot
        .accounts
        .into_iter()
        .find(|account| account.record.account_id.as_str() == account_id)
    else {
        return Ok(OrderBuyerActorReadiness {
            account: None,
            issues: vec![issue_with_code(
                "account_unresolved",
                "buyer_actor.account_id",
                format!(
                    "order buyer_actor account_id `{account_id}` is not present in the local account store"
                ),
            )],
        });
    };

    let account_pubkey = account.record.public_identity.public_key_hex.as_str();
    let mut issues = Vec::new();
    if !account_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        issues.push(issue_with_code(
            "account_mismatch",
            "buyer_actor.pubkey",
            format!(
                "order buyer_actor pubkey `{buyer_pubkey}` does not match local account `{account_id}` pubkey `{account_pubkey}`"
            ),
        ));
    }
    if !account.write_capable {
        issues.push(issue_with_code(
            "account_watch_only",
            "buyer_actor.account_id",
            format!(
                "order buyer_actor account `{account_id}` is watch_only and cannot sign until a matching secret is attached"
            ),
        ));
    }

    Ok(OrderBuyerActorReadiness {
        account: Some(account),
        issues,
    })
}

fn collect_issues(document: &OrderDraftDocument) -> Vec<OrderIssueView> {
    let mut issues = Vec::new();
    if document.version != 1 {
        issues.push(issue("version", "version must be 1"));
    }
    if document.kind != ORDER_DRAFT_KIND {
        issues.push(issue("kind", format!("kind must be `{ORDER_DRAFT_KIND}`")));
    }
    if !is_valid_order_id(document.order.order_id.as_str()) {
        issues.push(issue(
            "order.order_id",
            "order_id must look like `ord_<base64url>` or a canonical UUID",
        ));
    }

    match normalize_optional(Some(document.order.listing_addr.as_str())) {
        Some(listing_addr) => match parse_listing_addr(listing_addr.as_str()) {
            Ok(parsed) => {
                if parsed.kind != KIND_LISTING {
                    issues.push(issue(
                        "trade.listing_addr",
                        "listing_addr must reference a public NIP-99 listing",
                    ));
                }
                if let Some(seller_pubkey) = non_empty_string(document.order.seller_pubkey.clone())
                {
                    if seller_pubkey != parsed.seller_pubkey {
                        issues.push(issue(
                            "order.seller_pubkey",
                            "seller_pubkey must match listing_addr seller when both are set",
                        ));
                    }
                }
            }
            Err(error) => issues.push(issue(
                "trade.listing_addr",
                format!("listing_addr is invalid: {error}"),
            )),
        },
        None => issues.push(issue(
            "trade.listing_addr",
            "listing_addr is required before trade submit",
        )),
    }

    match normalize_optional(Some(document.order.listing_event_id.as_str())) {
        Some(listing_event_id) => {
            if !is_valid_event_id(listing_event_id.as_str()) {
                issues.push(issue(
                    "trade.listing_event_id",
                    "listing_event_id must be a 64-character hex Nostr event id",
                ));
            }
        }
        None => issues.push(issue(
            "trade.listing_event_id",
            "latest active listing event id is required before trade submit; run `radroots market refresh` and create the trade from local market data",
        )),
    }

    match normalize_listing_relay_set(document.order.listing_relays.iter()) {
        Ok(listing_relays) if listing_relays.is_empty() => issues.push(issue_with_code(
            "listing_provenance_missing",
            "trade.listing_relays",
            "listing relay provenance is required before trade submit; run `radroots market refresh` and create the trade from current local market data",
        )),
        Ok(_) => {}
        Err(error) => issues.push(issue_with_code(
            "listing_provenance_invalid",
            "trade.listing_relays",
            format!("listing relay provenance is invalid: {error}"),
        )),
    }

    if document.order.items.is_empty() {
        issues.push(issue(
            "order.items",
            "at least one order item is required before trade submit",
        ));
    }
    for (index, item) in document.order.items.iter().enumerate() {
        if item.bin_id.trim().is_empty() {
            issues.push(issue(
                format!("order.items[{index}].bin_id"),
                "bin_id must not be empty",
            ));
        }
        if item.bin_count == 0 {
            issues.push(issue(
                format!("order.items[{index}].bin_count"),
                "bin_count must be greater than zero",
            ));
        }
    }

    match &document.order.economics {
        Some(economics) => {
            if let Err(error) = economics.validate() {
                issues.push(issue(
                    "order.economics",
                    format!("order economics is invalid: {error}"),
                ));
            }
            if !order_items_match_economics(document.order.items.as_slice(), economics) {
                issues.push(issue(
                    "order.economics",
                    "order economics must match the order item bin ids and counts",
                ));
            }
        }
        None => issues.push(issue(
            "order.economics",
            "quote economics is required before trade submit; run `radroots basket quote create` from current local market data",
        )),
    }

    if document.buyer_actor.account_id.trim().is_empty() {
        issues.push(issue(
            "buyer_actor.account_id",
            "buyer_actor account_id is required before trade submit",
        ));
    }
    if document.buyer_actor.pubkey.trim().is_empty() {
        issues.push(issue(
            "buyer_actor.pubkey",
            "buyer_actor pubkey is required before trade submit",
        ));
    }
    if document.buyer_actor.source.trim().is_empty() {
        issues.push(issue(
            "buyer_actor.source",
            "buyer_actor source is required before trade submit",
        ));
    } else if !matches!(
        document.buyer_actor.source.as_str(),
        ORDER_BUYER_ACTOR_SOURCE_RESOLVED_ACCOUNT | ORDER_BUYER_ACTOR_SOURCE_REBIND
    ) {
        issues.push(issue(
            "buyer_actor.source",
            format!(
                "unsupported buyer_actor source `{}`",
                document.buyer_actor.source
            ),
        ));
    }
    if document.order.buyer_pubkey.trim().is_empty() {
        issues.push(issue(
            "order.buyer_pubkey",
            "order buyer_pubkey is required before trade submit",
        ));
    } else if !document
        .order
        .buyer_pubkey
        .eq_ignore_ascii_case(document.buyer_actor.pubkey.as_str())
    {
        issues.push(issue(
            "order.buyer_pubkey",
            "order buyer_pubkey must match buyer_actor pubkey",
        ));
    }

    issues
}

fn order_items_match_economics(
    items: &[OrderDraftItem],
    economics: &RadrootsOrderEconomics,
) -> bool {
    let mut order_items = items
        .iter()
        .map(|item| (item.bin_id.as_str(), item.bin_count))
        .collect::<Vec<_>>();
    let mut economic_items = economics
        .items
        .iter()
        .map(|item| (item.bin_id.as_str(), item.bin_count))
        .collect::<Vec<_>>();
    order_items.sort_unstable();
    economic_items.sort_unstable();
    order_items == economic_items
}

fn actions_for_document(
    document: &OrderDraftDocument,
    file: &Path,
    issues: &[OrderIssueView],
) -> Vec<String> {
    if app_order_issue_present(issues, APP_ORDER_ALREADY_SUBMITTED_ISSUE) {
        return vec![format!(
            "radroots trade status get {}",
            document.order.order_id
        )];
    }
    if app_order_issue_present(issues, APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE) {
        return vec![
            format!("radroots trade status get {}", document.order.order_id),
            "radroots trade app list".to_owned(),
        ];
    }

    let mut actions = Vec::new();
    actions.push(format!(
        "edit {} and fill the remaining draft fields",
        file.display()
    ));
    if document.buyer_actor.account_id.trim().is_empty()
        || document.buyer_actor.pubkey.trim().is_empty()
        || document.order.buyer_pubkey.trim().is_empty()
        || !document
            .order
            .buyer_pubkey
            .eq_ignore_ascii_case(document.buyer_actor.pubkey.as_str())
    {
        actions.push(format!(
            "radroots trade rebind {} <selector>",
            document.order.order_id
        ));
    }
    if issues
        .iter()
        .any(|issue| issue.code == "account_unresolved")
    {
        actions.push("radroots account import <path>".to_owned());
        actions.push(format!(
            "radroots trade rebind {} <selector>",
            document.order.order_id
        ));
    }
    if issues
        .iter()
        .any(|issue| issue.code == "account_watch_only")
    {
        actions.push(format!(
            "radroots account attach-secret {} <path>",
            document.buyer_actor.account_id
        ));
        actions.push(format!("radroots trade get {}", document.order.order_id));
    }
    if issues.iter().any(|issue| issue.code == "account_mismatch") {
        actions.push(format!(
            "radroots trade rebind {} <selector>",
            document.order.order_id
        ));
    }
    if document.order.items.is_empty()
        || issues
            .iter()
            .any(|issue| issue.field.starts_with("order.items["))
    {
        actions.push(format!("radroots trade get {}", document.order.order_id));
    }
    let mut deduped = Vec::new();
    for action in actions {
        if !deduped.contains(&action) {
            deduped.push(action);
        }
    }
    deduped
}

fn app_order_issue_present(issues: &[OrderIssueView], code: &str) -> bool {
    issues.iter().any(|issue| issue.code == code)
}

fn app_order_issue<'a>(issues: &'a [OrderIssueView], code: &str) -> Option<&'a OrderIssueView> {
    issues.iter().find(|issue| issue.code == code)
}

fn order_rebind_selector_error(selector: &str, error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::Accounts(_) | RuntimeError::Account(_) => {
            account::AccountRuntimeFailure::unresolved_with_detail(
                format!("order rebind target selector `{selector}` did not resolve"),
                json!({
                    "selector": selector,
                    "actions": [
                        "radroots account list",
                        "radroots account import <path>",
                        "radroots account create",
                    ],
                }),
            )
            .into()
        }
        other => other,
    }
}

fn order_rebind_existing_request_check(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
) -> Result<OrderRebindExistingRequestCheck, RuntimeError> {
    if config.relay.urls.is_empty() {
        return Ok(OrderRebindExistingRequestCheck {
            state: "skipped_no_relays".to_owned(),
            event_ids: Vec::new(),
        });
    }

    let filter = order_request_filter(
        loaded.document.order.seller_pubkey.as_str(),
        Some(loaded.document.order.order_id.as_str()),
    )?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut event_ids = receipt
        .events
        .iter()
        .filter_map(|event| {
            order_submit_request_from_event(event, loaded)
                .ok()
                .map(|request| request.request_event_id)
        })
        .collect::<Vec<_>>();
    event_ids.sort();
    event_ids.dedup();

    Ok(OrderRebindExistingRequestCheck {
        state: if event_ids.is_empty() {
            "clear".to_owned()
        } else {
            "blocked_existing_request".to_owned()
        },
        event_ids,
    })
}

fn resolve_initial_buyer_actor(
    config: &RuntimeConfig,
) -> Result<OrderDraftBuyerActor, RuntimeError> {
    let resolution = account::resolve_account_resolution(config)?;
    let Some(account) = resolution.resolved_account else {
        return Err(account::AccountRuntimeFailure::unresolved_with_detail(
            account::unresolved_account_reason(config)?,
            json!({
                "buyer_actor_source": ORDER_BUYER_ACTOR_SOURCE_RESOLVED_ACCOUNT,
                "actions": [
                    "radroots account create",
                    "radroots account import <path>",
                ],
            }),
        )
        .into());
    };
    Ok(OrderDraftBuyerActor {
        account_id: account.record.account_id.to_string(),
        pubkey: account.record.public_identity.public_key_hex,
        source: ORDER_BUYER_ACTOR_SOURCE_RESOLVED_ACCOUNT.to_owned(),
    })
}

fn buyer_account_id(document: &OrderDraftDocument) -> Option<String> {
    non_empty_string(document.buyer_actor.account_id.clone())
}

fn buyer_actor_source(document: &OrderDraftDocument) -> Option<String> {
    non_empty_string(document.buyer_actor.source.clone())
}

fn load_local_order_draft_if_exists(
    config: &RuntimeConfig,
    lookup: &str,
) -> Result<Option<LoadedOrderDraft>, RuntimeError> {
    let file = draft_lookup_path(config, lookup);
    if !file.exists() {
        return Ok(None);
    }
    load_draft(file.as_path())
        .map(Some)
        .map_err(RuntimeError::Config)
}

fn order_status_actor_context(
    config: &RuntimeConfig,
    order_id: &str,
) -> Result<OrderDraftStatusActorContext, RuntimeError> {
    if let Some(loaded) = load_local_order_draft_if_exists(config, order_id)? {
        return Ok(OrderDraftStatusActorContext {
            source: ORDER_ACTOR_CONTEXT_ORDER_DRAFT,
            buyer_pubkey: non_empty_string(loaded.document.buyer_actor.pubkey.clone())
                .or_else(|| non_empty_string(loaded.document.order.buyer_pubkey.clone())),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey),
            selected_account_pubkey: None,
        });
    }

    let selected_account = account::resolve_account(config)?;
    let Some(account) = selected_account else {
        return Ok(OrderDraftStatusActorContext {
            source: ORDER_ACTOR_CONTEXT_NETWORK_ONLY,
            buyer_pubkey: None,
            seller_pubkey: None,
            selected_account_pubkey: None,
        });
    };

    Ok(OrderDraftStatusActorContext {
        source: ORDER_ACTOR_CONTEXT_RESOLVED_ACCOUNT,
        buyer_pubkey: None,
        seller_pubkey: None,
        selected_account_pubkey: Some(account.record.public_identity.public_key_hex),
    })
}

fn order_event_list_actor_context(
    config: &RuntimeConfig,
    order_id: Option<&str>,
) -> Result<Option<OrderEventListActorContext>, RuntimeError> {
    if let Some(order_id) = order_id
        && let Some(loaded) = load_local_order_draft_if_exists(config, order_id)?
    {
        let seller_pubkey =
            non_empty_string(loaded.document.order.seller_pubkey).ok_or_else(|| {
                RuntimeError::Config(format!(
                    "local trade draft `{order_id}` is missing seller_pubkey"
                ))
            })?;
        return Ok(Some(OrderEventListActorContext {
            source: ORDER_ACTOR_CONTEXT_ORDER_DRAFT,
            seller_pubkey,
        }));
    }

    Ok(
        account::resolve_account(config)?.map(|account| OrderEventListActorContext {
            source: ORDER_ACTOR_CONTEXT_RESOLVED_ACCOUNT,
            seller_pubkey: account.record.public_identity.public_key_hex,
        }),
    )
}

fn bound_buyer_write_context_if_exists(
    config: &RuntimeConfig,
    order_id: &str,
) -> Result<Option<OrderBoundBuyerWriteContext>, RuntimeError> {
    let Some(loaded) = load_local_order_draft_if_exists(config, order_id)? else {
        return Ok(None);
    };
    let account = validate_bound_order_buyer_account(config, &loaded)?;
    Ok(Some(OrderBoundBuyerWriteContext { loaded, account }))
}

fn order_buyer_write_actor_context(
    config: &RuntimeConfig,
    order_id: &str,
) -> Result<Option<OrderBuyerWriteActorContext>, RuntimeError> {
    if let Some(bound) = bound_buyer_write_context_if_exists(config, order_id)? {
        let selected_pubkey = bound.account.record.public_identity.public_key_hex.clone();
        let status_seller_pubkey =
            non_empty_string(bound.loaded.document.order.seller_pubkey.clone());
        return Ok(Some(OrderBuyerWriteActorContext {
            bound: Some(bound),
            selected_pubkey: selected_pubkey.clone(),
            status_buyer_pubkey: Some(selected_pubkey),
            status_seller_pubkey,
            status_context_source: ORDER_ACTOR_CONTEXT_ORDER_DRAFT,
        }));
    }

    Ok(account::resolve_account(config)?.map(|account| {
        let selected_pubkey = account.record.public_identity.public_key_hex;
        OrderBuyerWriteActorContext {
            bound: None,
            selected_pubkey: selected_pubkey.clone(),
            status_buyer_pubkey: None,
            status_seller_pubkey: None,
            status_context_source: ORDER_ACTOR_CONTEXT_RESOLVED_ACCOUNT,
        }
    }))
}

fn order_submit_listing_freshness_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(Some(order_submit_unconfigured_view(
            config,
            loaded,
            args,
            "trade submit requires local market data to confirm the listing is still active; run `radroots store init` and `radroots market refresh` before submitting",
            vec![issue(
                "trade.listing_addr",
                "local replica database is missing; run `radroots store init` and `radroots market refresh` before submitting",
            )],
            vec![
                "radroots store init".to_owned(),
                "radroots market refresh".to_owned(),
            ],
        )));
    }

    let listing_addr = loaded.document.order.listing_addr.as_str();
    let parsed = parse_listing_addr(listing_addr)
        .map_err(|error| RuntimeError::Config(format!("trade listing_addr is invalid: {error}")))?;
    let active_event_id = match resolve_active_listing_event_id(config, listing_addr, &parsed)? {
        Some(event_id) => event_id,
        None => {
            return Ok(Some(order_submit_unconfigured_view(
                config,
                loaded,
                args,
                "trade listing is not active in the local replica; run `radroots market refresh` and create a new trade from current market data",
                vec![issue(
                    "trade.listing_addr",
                    "listing is missing, archived, or superseded in the local replica",
                )],
                vec!["radroots market refresh".to_owned()],
            )));
        }
    };

    if !active_event_id.eq_ignore_ascii_case(loaded.document.order.listing_event_id.as_str()) {
        return Ok(Some(order_submit_unconfigured_view(
            config,
            loaded,
            args,
            "trade listing event is no longer current in the local replica; run `radroots market refresh` and create a new trade from current market data",
            vec![issue(
                "trade.listing_event_id",
                format!(
                    "draft listing_event_id does not match latest local listing event `{active_event_id}`"
                ),
            )],
            vec!["radroots market refresh".to_owned()],
        )));
    }

    Ok(None)
}

fn order_submit_quantity_preflight_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(Some(order_submit_unconfigured_view(
            config,
            loaded,
            args,
            "trade submit requires local market data to confirm current listing availability; run `radroots store init` and `radroots market refresh` before submitting",
            vec![issue(
                "trade.listing_addr",
                "local replica database is missing; run `radroots store init` and `radroots market refresh` before submitting",
            )],
            vec![
                "radroots store init".to_owned(),
                "radroots market refresh".to_owned(),
            ],
        )));
    }

    let requested_count =
        loaded
            .document
            .order
            .items
            .iter()
            .enumerate()
            .try_fold(0u64, |total, (index, item)| {
                if item.bin_count == 0 {
                    return Err(RuntimeError::Config(format!(
                        "order item {index} quantity must be greater than zero"
                    )));
                }
                total.checked_add(u64::from(item.bin_count)).ok_or_else(|| {
                    RuntimeError::Config("order quantity exceeds supported range".to_owned())
                })
            })?;

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let product_rows = trade_product::find_many(
        &executor,
        &ITradeProductFindMany {
            filter: Some(trade_product_listing_addr_filter(
                loaded.document.order.listing_addr.as_str(),
            )),
        },
    )
    .map_err(|error| RuntimeError::Config(format!("resolve listing product state: {error:?}")))?
    .results;

    let product = match product_rows.as_slice() {
        [product] => product,
        [] => {
            return Ok(Some(order_submit_unconfigured_view(
                config,
                loaded,
                args,
                "trade listing is not active in the local replica; run `radroots market refresh` and create a new trade from current market data",
                vec![issue(
                    "trade.listing_addr",
                    "listing is missing, archived, or superseded in the local replica",
                )],
                vec!["radroots market refresh".to_owned()],
            )));
        }
        _ => {
            return Err(RuntimeError::Config(format!(
                "listing address `{}` matched {} active local listing rows",
                loaded.document.order.listing_addr,
                product_rows.len()
            )));
        }
    };

    let Some(primary_bin_id) = product.primary_bin_id.as_deref().and_then(non_empty_ref) else {
        return Ok(Some(order_submit_invalid_quantity_view(
            config,
            loaded,
            args,
            "trade listing bin identity is missing in the local replica",
            vec![issue_with_code(
                "listing_primary_bin_missing",
                "inventory.primary_bin_id",
                "current local replica listing primary bin is required before submit",
            )],
        )));
    };
    let Some(verified_primary_bin_id) = product
        .verified_primary_bin_id
        .as_deref()
        .and_then(non_empty_ref)
    else {
        return Ok(Some(order_submit_invalid_quantity_view(
            config,
            loaded,
            args,
            "trade listing bin identity is not verified in the local replica",
            vec![issue_with_code(
                "listing_primary_bin_invalid",
                "inventory.primary_bin_id",
                format!("current local replica primary bin `{primary_bin_id}` is not verified"),
            )],
        )));
    };
    if verified_primary_bin_id != primary_bin_id {
        return Ok(Some(order_submit_invalid_quantity_view(
            config,
            loaded,
            args,
            "trade listing bin identity is invalid in the local replica",
            vec![issue_with_code(
                "listing_primary_bin_invalid",
                "inventory.primary_bin_id",
                format!(
                    "current local replica primary bin `{primary_bin_id}` does not match verified primary bin `{verified_primary_bin_id}`"
                ),
            )],
        )));
    }

    let mut bin_issues = Vec::new();
    for (index, item) in loaded.document.order.items.iter().enumerate() {
        if item.bin_id != primary_bin_id {
            bin_issues.push(issue_with_code(
                "order_bin_unknown",
                format!("order.items[{index}].bin_id"),
                format!(
                    "draft bin `{}` is not in the current local listing bin set; expected primary bin `{primary_bin_id}`",
                    item.bin_id
                ),
            ));
        }
    }
    if !bin_issues.is_empty() {
        return Ok(Some(order_submit_invalid_quantity_view(
            config,
            loaded,
            args,
            "trade draft references a bin outside the current local listing",
            bin_issues,
        )));
    }

    let available_count = match product.qty_avail {
        Some(value) if value >= 0 => value as u64,
        Some(value) => {
            return Ok(Some(order_submit_invalid_quantity_view(
                config,
                loaded,
                args,
                "trade listing availability is invalid in the local replica",
                vec![issue_with_code(
                    "listing_inventory_availability_invalid",
                    "inventory.available",
                    format!("current local replica availability is negative: {value}"),
                )],
            )));
        }
        None => {
            return Ok(Some(order_submit_invalid_quantity_view(
                config,
                loaded,
                args,
                "trade listing availability is missing in the local replica",
                vec![issue_with_code(
                    "listing_inventory_availability_missing",
                    "inventory.available",
                    "current local replica listing availability is required before submit",
                )],
            )));
        }
    };

    if requested_count > available_count {
        return Ok(Some(order_submit_invalid_quantity_view(
            config,
            loaded,
            args,
            "order requested quantity exceeds current local listing availability",
            vec![issue_with_code(
                "order_quantity_exceeds_available",
                "order.items",
                format!(
                    "requested quantity {requested_count} exceeds current local replica available quantity {available_count}"
                ),
            )],
        )));
    }

    Ok(None)
}

fn order_submit_unconfigured_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
    mut actions: Vec<String>,
) -> OrderSubmitView {
    actions.push(format!(
        "radroots trade get {}",
        loaded.document.order.order_id
    ));

    OrderSubmitView {
        state: "unconfigured".to_owned(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: None,
        reason: Some(reason.into()),
        job: None,
        issues,
        actions,
    }
}

fn order_submit_app_signed_evidence_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    issues: &[OrderIssueView],
) -> Option<OrderSubmitView> {
    if let Some(issue) = app_order_issue(issues, APP_ORDER_ALREADY_SUBMITTED_ISSUE) {
        return Some(OrderSubmitView {
            state: "submitted".to_owned(),
            source: ORDER_SUBMIT_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
            listing_relays: order_listing_relays(&loaded.document),
            buyer_account_id: buyer_account_id(&loaded.document),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            buyer_actor_source: buyer_actor_source(&loaded.document),
            buyer_custody: None,
            buyer_write_capable: None,
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            event_id: issue.event_ids.first().cloned(),
            event_kind: Some(KIND_ORDER_REQUEST),
            dry_run: config.output.dry_run,
            deduplicated: true,
            target_relays: Vec::new(),
            connected_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            reason: Some(
                "matching signed order request evidence already exists; publish skipped".to_owned(),
            ),
            job: None,
            issues: vec![issue.clone()],
            actions: Vec::new(),
        });
    }

    if app_order_issue_present(issues, APP_ORDER_SIGNED_EVIDENCE_CONFLICT_ISSUE) {
        return Some(OrderSubmitView {
            state: "invalid".to_owned(),
            source: ORDER_SUBMIT_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
            listing_relays: order_listing_relays(&loaded.document),
            buyer_account_id: buyer_account_id(&loaded.document),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            buyer_actor_source: buyer_actor_source(&loaded.document),
            buyer_custody: None,
            buyer_write_capable: None,
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            event_id: None,
            event_kind: Some(KIND_ORDER_REQUEST),
            dry_run: config.output.dry_run,
            deduplicated: false,
            target_relays: Vec::new(),
            connected_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            reason: Some(
                "signed order request evidence conflicts with the app-authored local order"
                    .to_owned(),
            ),
            job: None,
            issues: issues.to_vec(),
            actions: vec![format!(
                "radroots trade status get {}",
                loaded.document.order.order_id
            )],
        });
    }

    None
}

fn order_submit_invalid_quantity_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "invalid".to_owned(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: None,
        reason: Some(reason.into()),
        job: None,
        issues,
        actions: vec![
            "radroots market refresh".to_owned(),
            format!("radroots trade get {}", loaded.document.order.order_id),
        ],
    }
}

fn order_submit_listing_provenance_preflight_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    let listing_relays =
        normalize_listing_relay_set(loaded.document.order.listing_relays.iter())
            .map_err(|error| RuntimeError::Config(format!("listing provenance relays: {error}")))?;
    let target_relays = normalize_listing_relay_set(config.relay.urls.iter())
        .map_err(|error| RuntimeError::Config(format!("configured relay target: {error}")))?;
    if target_relays.is_empty() {
        return Ok(None);
    }
    let reachable_relays = listing_relays
        .iter()
        .filter(|relay| target_relays.contains(relay))
        .cloned()
        .collect::<Vec<_>>();
    if !reachable_relays.is_empty() {
        return Ok(None);
    }

    let mut actions = listing_relays
        .iter()
        .map(|relay| {
            format!(
                "radroots --relay {} trade submit {}",
                relay, loaded.document.order.order_id
            )
        })
        .collect::<Vec<_>>();
    actions.push(format!(
        "radroots trade get {}",
        loaded.document.order.order_id
    ));
    Ok(Some(OrderSubmitView {
        state: "unconfigured".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays,
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: Some(KIND_ORDER_REQUEST),
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays,
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: Some(
            "trade submit requires at least one configured relay that is known to carry the listing"
                .to_owned(),
        ),
        job: None,
        issues: vec![issue_with_code(
            "listing_relay_target_mismatch",
            "trade.listing_relays",
            format!(
                "configured relays must include one of the listing provenance relays: {}",
                loaded.document.order.listing_relays.join(", ")
            ),
        )],
        actions,
    }))
}

fn order_submit_market_freshness_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    if config.output.dry_run || config.relay.urls.is_empty() {
        return Ok(None);
    }

    let mut freshness = freshness_for_scope(config, RelayIngestScope::MarketRefresh)?;
    if freshness_requires_refresh(&freshness) {
        let _ = market_refresh(config)?;
        freshness = freshness_for_scope(config, RelayIngestScope::MarketRefresh)?;
    }
    if !freshness_requires_refresh(&freshness) {
        return Ok(None);
    }

    Ok(Some(order_submit_unconfigured_view(
        config,
        loaded,
        args,
        "trade submit requires a current market refresh before signing; run `radroots market refresh` with the relays you trust, then submit again",
        vec![issue(
            "trade.listing_addr",
            format!(
                "local market freshness is `{}`; current listing state must be refreshed before trade submit",
                freshness.state
            ),
        )],
        vec!["radroots market refresh".to_owned()],
    )))
}

fn order_submit_existing_request_view_from_receipt(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    payload: &RadrootsOrderRequest,
    receipt: DirectRelayFetchReceipt,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        events,
    } = receipt;
    let mut requests = Vec::new();
    let mut candidate_issues = Vec::new();
    let candidate_context = OrderRequestCandidateContext {
        order_id: loaded.document.order.order_id.as_str(),
        seller_pubkey: Some(loaded.document.order.seller_pubkey.as_str()),
    };

    for event in events {
        if !order_request_candidate_matches(&event, candidate_context) {
            continue;
        }
        let event_id = event.id.to_string();
        match order_submit_request_from_event(&event, loaded) {
            Ok(request) => requests.push(request),
            Err(error) => candidate_issues.push(issue_with_events(
                "invalid_request_candidate",
                "request_event_id",
                format!("request event `{event_id}` failed trade submit preflight: {error}"),
                vec![event_id],
            )),
        }
    }

    requests.sort_by(|left, right| left.request_event_id.cmp(&right.request_event_id));
    candidate_issues.sort_by(|left, right| {
        left.event_ids
            .cmp(&right.event_ids)
            .then_with(|| left.message.cmp(&right.message))
    });
    if !candidate_issues.is_empty() {
        return Ok(Some(order_submit_invalid_existing_request_view(
            config,
            loaded,
            args,
            "visible order request candidates failed submit preflight validation",
            candidate_issues,
            target_relays,
            failed_relays,
        )));
    }

    let request_event_ids = requests
        .iter()
        .map(|request| request.request_event_id.clone())
        .collect::<Vec<_>>();

    match requests.as_slice() {
        [] => Ok(None),
        [request] if order_submit_request_matches_draft(request, loaded, payload) => {
            Ok(Some(order_submit_deduplicated_view(
                config,
                loaded,
                args,
                request,
                target_relays,
                connected_relays,
                failed_relays,
            )))
        }
        [request] => Ok(Some(order_submit_invalid_existing_request_view(
            config,
            loaded,
            args,
            "visible order request event conflicts with the local trade draft; refusing to publish a second request for the same order id",
            vec![issue_with_events(
                "existing_request_conflict",
                "request_event_id",
                format!(
                    "request event `{}` does not match the local trade draft",
                    request.request_event_id
                ),
                vec![request.request_event_id.clone()],
            )],
            target_relays,
            failed_relays,
        ))),
        _ => Ok(Some(order_submit_invalid_existing_request_view(
            config,
            loaded,
            args,
            "multiple visible order request events matched the local order id; refusing to publish another request",
            vec![issue_with_events(
                "multiple_request_candidates",
                "request_event_id",
                format!(
                    "matched {} request events for the same order id",
                    requests.len()
                ),
                request_event_ids,
            )],
            target_relays,
            failed_relays,
        ))),
    }
}

fn order_submit_request_from_event(
    event: &RadrootsNostrEvent,
    loaded: &LoadedOrderDraft,
) -> Result<ResolvedOrderSubmitRequest, RuntimeError> {
    let event = radroots_event_from_nostr(event);
    let envelope = order_request_from_event(&event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context =
        order_event_context_from_tags(RadrootsOrderEventType::OrderRequested, &event.tags)
            .map_err(|error| RuntimeError::Config(format!("decode order request tags: {error}")))?;

    if envelope.order_id != loaded.document.order.order_id
        || envelope.payload.order_id != loaded.document.order.order_id
    {
        return Err(RuntimeError::Config(
            "order request does not match local order id".to_owned(),
        ));
    }
    if context.counterparty_pubkey != envelope.payload.seller_pubkey {
        return Err(RuntimeError::Config(
            "order request p tag does not match seller_pubkey".to_owned(),
        ));
    }
    let listing_addr =
        parse_listing_addr(envelope.payload.listing_addr.as_str()).map_err(|error| {
            RuntimeError::Config(format!("order request listing_addr is invalid: {error}"))
        })?;
    if listing_addr.seller_pubkey != envelope.payload.seller_pubkey {
        return Err(RuntimeError::Config(
            "order request listing address is outside seller authority".to_owned(),
        ));
    }
    let payload = canonicalize_order_request_for_signer(envelope.payload, event.author.as_str())
        .map_err(|error| RuntimeError::Config(format!("canonicalize order request: {error}")))?;
    let listing_event_id = context.listing_event.as_ref().map(|event| event.id.clone());

    Ok(ResolvedOrderSubmitRequest {
        request_event_id: event.id,
        listing_event_id,
        payload,
    })
}

fn order_submit_request_matches_draft(
    request: &ResolvedOrderSubmitRequest,
    loaded: &LoadedOrderDraft,
    payload: &RadrootsOrderRequest,
) -> bool {
    request.payload == *payload
        && request.listing_event_id.as_deref()
            == Some(loaded.document.order.listing_event_id.as_str())
}

fn order_submit_deduplicated_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    request: &ResolvedOrderSubmitRequest,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "submitted".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: OrderTradeLocatorView {
            trade_id: loaded.document.order.order_id.clone(),
            root_event_id: Some(request.request_event_id.clone()),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        },
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: Some(request.request_event_id.clone()),
        event_kind: Some(KIND_ORDER_REQUEST),
        dry_run: config.output.dry_run,
        deduplicated: true,
        target_relays,
        connected_relays: connected_relays.clone(),
        acknowledged_relays: connected_relays,
        failed_relays: relay_failures(failed_relays),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: Some(
            "an identical order request is already visible on the configured relays; publish skipped"
                .to_owned(),
        ),
        job: None,
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

fn order_submit_dry_run_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    plan: TradeSubmitPlan,
    target_relays: Vec<String>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "dry_run".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: OrderTradeLocatorView {
            trade_id: loaded.document.order.order_id.clone(),
            root_event_id: Some(plan.expected_event_id.to_string()),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        },
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: Some(plan.expected_event_id.as_str().to_owned()),
        event_kind: Some(KIND_ORDER_REQUEST),
        dry_run: true,
        deduplicated: false,
        target_relays,
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: Some("dry run requested; SDK enqueue and relay push skipped".to_owned()),
        job: None,
        issues: Vec::new(),
        actions: vec![format!(
            "radroots trade submit {}",
            loaded.document.order.order_id
        )],
    }
}

fn order_submit_invalid_existing_request_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
    target_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "invalid".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: Some(KIND_ORDER_REQUEST),
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays,
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: relay_failures(failed_relays),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: Some(reason.into()),
        job: None,
        issues,
        actions: vec![format!(
            "radroots trade status get {}",
            loaded.document.order.order_id
        )],
    }
}

fn canonical_order_request_payload_from_loaded(
    loaded: &LoadedOrderDraft,
    signer_pubkey: &str,
) -> Result<RadrootsOrderRequest, RuntimeError> {
    let economics =
        loaded.document.order.economics.clone().ok_or_else(|| {
            RuntimeError::Config("trade draft is missing quote economics".to_owned())
        })?;
    let items = loaded
        .document
        .order
        .items
        .iter()
        .map(|item| {
            Ok(RadrootsOrderItem {
                bin_id: protocol_inventory_bin_id(item.bin_id.as_str(), "order item bin_id")?,
                bin_count: item.bin_count,
            })
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    let payload = RadrootsOrderRequest {
        order_id: protocol_order_id(loaded.document.order.order_id.as_str(), "order_id")?,
        listing_addr: protocol_listing_addr(
            loaded.document.order.listing_addr.as_str(),
            "listing_addr",
        )?,
        buyer_pubkey: protocol_pubkey(loaded.document.order.buyer_pubkey.as_str(), "buyer_pubkey")?,
        seller_pubkey: protocol_pubkey(
            loaded.document.order.seller_pubkey.as_str(),
            "seller_pubkey",
        )?,
        items,
        economics,
    };
    canonicalize_order_request_for_signer(payload, signer_pubkey)
        .map_err(|error| RuntimeError::Config(format!("canonicalize order request: {error}")))
}

fn order_submit_listing_event_ptr(
    loaded: &LoadedOrderDraft,
) -> Result<RadrootsNostrEventPtr, RuntimeError> {
    let listing_relays =
        normalize_listing_relay_set(loaded.document.order.listing_relays.iter())
            .map_err(|error| RuntimeError::Config(format!("listing provenance relays: {error}")))?;
    Ok(RadrootsNostrEventPtr {
        id: loaded.document.order.listing_event_id.clone(),
        relays: listing_relays.first().cloned(),
    })
}

fn propose_trade_via_sdk(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    account: &account::AccountRecordView,
    payload: RadrootsOrderRequest,
) -> Result<OrderSubmitView, CliSdkAdapterError> {
    let actor = sdk_trade_actor(account, RadrootsActorRole::Buyer, "propose")?;
    let publish_mode = trade_publish_mode(config);
    let ack_policy = trade_ack_policy(publish_mode)?;
    let mut request = TradeProposeRequest::new(
        actor,
        order_submit_listing_event_ptr(loaded)?,
        payload,
        trade_relay_resolution_policy(),
        publish_mode,
        ack_policy,
    )
    .with_privacy_confirmation(trade_privacy_confirmation());
    if let Some(idempotency_key) = args.idempotency_key.as_deref() {
        request = request.try_with_idempotency_key(idempotency_key)?;
    }

    let session = connect_sdk_for_trade_actor(config, account, "trade propose")?;
    let outcome = session.block_on(session.sdk().trades().buyer().propose_trade(request))?;
    Ok(sdk_trade_submit_outcome_view(config, loaded, args, outcome))
}

fn sdk_trade_submit_outcome_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    outcome: TradeMutationOutcome<TradeSubmitPlan, TradeSubmitReceipt>,
) -> OrderSubmitView {
    match outcome {
        TradeMutationOutcome::DryRun { plan } => {
            order_submit_dry_run_view(config, loaded, args, plan, config.relay.urls.clone())
        }
        TradeMutationOutcome::Enqueued { receipt } => {
            sdk_enqueued_order_submit_view(config, loaded, args, receipt, None)
        }
        TradeMutationOutcome::Published { receipt, publish } => {
            sdk_enqueued_order_submit_view(config, loaded, args, receipt, Some(&publish))
        }
    }
}

fn sdk_enqueued_order_submit_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    enqueue: TradeSubmitReceipt,
    push: Option<&PushOutboxReceipt>,
) -> OrderSubmitView {
    let push_event = push.and_then(|push| sdk_push_event_for_order_submit(&enqueue, push));
    OrderSubmitView {
        state: sdk_order_submit_state(push_event),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: order_locator_view_from_locator(&enqueue.locator),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: Some(enqueue.signed_event_id.as_str().to_owned()),
        event_kind: Some(KIND_ORDER_REQUEST),
        dry_run: false,
        deduplicated: matches!(enqueue.state, SdkMutationState::AlreadyQueued),
        target_relays: push_event
            .map(sdk_push_target_relays)
            .unwrap_or_else(|| config.relay.urls.clone()),
        connected_relays: push_event
            .map(sdk_push_connected_relays)
            .unwrap_or_default(),
        acknowledged_relays: push_event
            .map(sdk_push_acknowledged_relays)
            .unwrap_or_default(),
        failed_relays: push_event.map(sdk_push_failed_relays).unwrap_or_default(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: sdk_order_submit_reason(&enqueue.workflow, push_event),
        job: None,
        issues: Vec::new(),
        actions: sdk_order_submit_actions(push_event),
    }
}

fn sdk_push_event_for_order_submit<'a>(
    enqueue: &TradeSubmitReceipt,
    push: &'a PushOutboxReceipt,
) -> Option<&'a PushOutboxEventReceipt> {
    push.events
        .iter()
        .find(|event| event.event_id == enqueue.signed_event_id)
}

fn sdk_order_submit_state(push_event: Option<&PushOutboxEventReceipt>) -> String {
    match push_event.map(|event| event.final_state) {
        Some(PushOutboxEventState::Published) => "submitted",
        Some(PushOutboxEventState::PublishRetryable | PushOutboxEventState::FailedTerminal) => {
            "unavailable"
        }
        Some(_) | None => "queued",
    }
    .to_owned()
}

fn sdk_order_submit_reason(
    enqueue: &TradeWorkflowEnqueueReceipt,
    push_event: Option<&PushOutboxEventReceipt>,
) -> Option<String> {
    match push_event.map(|event| event.final_state) {
        Some(PushOutboxEventState::Published) => None,
        Some(PushOutboxEventState::PublishRetryable) => Some(format!(
            "{}; SDK relay publish did not reach accepted quorum; outbox event remains retryable; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        Some(PushOutboxEventState::FailedTerminal) => Some(format!(
            "{}; SDK relay publish failed terminally; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        Some(state) => Some(format!(
            "{}; SDK relay push left event in state `{state:?}`; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
        None => Some(format!(
            "{}; trade submit queued in SDK outbox; no ready SDK outbox event was pushed; {}",
            sdk_order_enqueue_summary(enqueue),
            sdk_order_enqueue_retry_summary(enqueue)
        )),
    }
}

fn sdk_order_submit_actions(push_event: Option<&PushOutboxEventReceipt>) -> Vec<String> {
    if !matches!(
        push_event.map(|event| event.final_state),
        Some(PushOutboxEventState::Published)
    ) {
        return sdk_order_push_recovery_actions();
    }
    Vec::new()
}

fn sdk_push_target_relays(event: &PushOutboxEventReceipt) -> Vec<String> {
    event
        .relays
        .iter()
        .map(|relay| relay.relay_url.clone())
        .collect()
}

fn sdk_push_connected_relays(event: &PushOutboxEventReceipt) -> Vec<String> {
    event
        .relays
        .iter()
        .filter(|relay| relay.attempted)
        .map(|relay| relay.relay_url.clone())
        .collect()
}

fn sdk_push_acknowledged_relays(event: &PushOutboxEventReceipt) -> Vec<String> {
    event
        .relays
        .iter()
        .filter(|relay| {
            matches!(
                relay.outcome_kind,
                PushOutboxRelayOutcomeKind::Accepted
                    | PushOutboxRelayOutcomeKind::DuplicateAccepted
            )
        })
        .map(|relay| relay.relay_url.clone())
        .collect()
}

fn sdk_push_failed_relays(event: &PushOutboxEventReceipt) -> Vec<RelayFailureView> {
    event
        .relays
        .iter()
        .filter(|relay| {
            !matches!(
                relay.outcome_kind,
                PushOutboxRelayOutcomeKind::Accepted
                    | PushOutboxRelayOutcomeKind::DuplicateAccepted
            )
        })
        .map(|relay| RelayFailureView {
            relay: relay.relay_url.clone(),
            reason: relay
                .message
                .clone()
                .unwrap_or_else(|| sdk_relay_outcome_kind(relay.outcome_kind).to_owned()),
        })
        .collect()
}

fn sdk_relay_outcome_kind(kind: PushOutboxRelayOutcomeKind) -> &'static str {
    match kind {
        PushOutboxRelayOutcomeKind::Accepted => "accepted",
        PushOutboxRelayOutcomeKind::DuplicateAccepted => "duplicate_accepted",
        PushOutboxRelayOutcomeKind::Blocked => "blocked",
        PushOutboxRelayOutcomeKind::RateLimited => "rate_limited",
        PushOutboxRelayOutcomeKind::Invalid => "invalid",
        PushOutboxRelayOutcomeKind::PowRequired => "pow_required",
        PushOutboxRelayOutcomeKind::Restricted => "restricted",
        PushOutboxRelayOutcomeKind::AuthRequired => "auth_required",
        PushOutboxRelayOutcomeKind::Error => "error",
        PushOutboxRelayOutcomeKind::Timeout => "timeout",
        PushOutboxRelayOutcomeKind::ConnectionFailed => "connection_failed",
        PushOutboxRelayOutcomeKind::Unknown => "unknown",
        _ => "unknown",
    }
}

fn order_binding_error_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &TradeSubmitArgs,
    error: ActorWriteBindingError,
) -> OrderSubmitView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);

    let mut actions = actions;
    actions.push(format!(
        "radroots trade get {}",
        loaded.document.order.order_id
    ));

    OrderSubmitView {
        state: state.clone(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        locator: order_locator_view_from_key(loaded.document.order.order_id.as_str()),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        listing_relays: order_listing_relays(&loaded.document),
        buyer_account_id: buyer_account_id(&loaded.document),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        buyer_actor_source: buyer_actor_source(&loaded.document),
        buyer_custody: None,
        buyer_write_capable: None,
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        reason: Some(reason),
        job: None,
        issues: Vec::new(),
        actions,
    }
}

fn validate_bound_order_buyer_account(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
) -> Result<account::AccountRecordView, RuntimeError> {
    let document = &loaded.document;
    let account_id = document.buyer_actor.account_id.trim();
    let buyer_pubkey = document.buyer_actor.pubkey.trim();
    let snapshot = account::snapshot(config)?;
    let Some(account) = snapshot
        .accounts
        .iter()
        .find(|account| account.record.account_id.as_str() == account_id)
        .cloned()
    else {
        return Err(account::AccountRuntimeFailure::unresolved_with_detail(
            format!(
                "order-bound buyer account `{account_id}` is not present in the local account store"
            ),
            order_buyer_failure_detail(
                loaded,
                json!({
                    "actions": [
                        "radroots account import <path>",
                        format!("radroots trade rebind {} <selector>", document.order.order_id),
                        format!("radroots trade get {}", document.order.order_id),
                    ],
                }),
            ),
        )
        .into());
    };

    let account_pubkey = account.record.public_identity.public_key_hex.as_str();
    if !account_pubkey.eq_ignore_ascii_case(buyer_pubkey)
        || !document
            .order
            .buyer_pubkey
            .eq_ignore_ascii_case(buyer_pubkey)
    {
        return Err(account::AccountRuntimeFailure::mismatch_with_detail(
            format!(
                "order-bound buyer account `{account_id}` does not match order buyer pubkey `{buyer_pubkey}`"
            ),
            order_buyer_failure_detail(
                loaded,
                json!({
                    "attempted_buyer_account_id": account_id,
                    "attempted_buyer_pubkey": account_pubkey,
                    "actions": [
                        format!("radroots trade rebind {} <selector>", document.order.order_id),
                        format!("radroots trade get {}", document.order.order_id),
                    ],
                }),
            ),
        )
        .into());
    }

    if !account.write_capable {
        return Err(account::AccountRuntimeFailure::watch_only_with_detail(
            account_id,
            order_buyer_failure_detail(
                loaded,
                json!({
                    "actions": [
                        format!("radroots account attach-secret {account_id} <path>"),
                        format!("radroots trade get {}", document.order.order_id),
                    ],
                }),
            ),
        )
        .into());
    }

    if let Some(selector) = config.account.selector.as_deref() {
        let attempted = account::resolve_account_selector(config, selector).map_err(|_| {
            account::AccountRuntimeFailure::unresolved_with_detail(
                format!("account override `{selector}` did not resolve to a local buyer account"),
                order_buyer_failure_detail(
                    loaded,
                    json!({
                        "attempted_buyer_account_id": selector,
                        "actions": [
                            "radroots account list",
                            format!("radroots trade get {}", document.order.order_id),
                        ],
                    }),
                ),
            )
        })?;
        if attempted.record.account_id.as_str() != account_id {
            let attempted_pubkey = attempted.record.public_identity.public_key_hex.as_str();
            return Err(account::AccountRuntimeFailure::mismatch_with_detail(
                format!(
                    "account override `{}` cannot retarget order `{}` bound to buyer account `{account_id}`",
                    attempted.record.account_id, document.order.order_id
                ),
                order_buyer_failure_detail(
                    loaded,
                    json!({
                        "attempted_buyer_account_id": attempted.record.account_id.to_string(),
                        "attempted_buyer_pubkey": attempted_pubkey,
                        "actions": [
                            format!("radroots --account-id {account_id} trade submit {}", document.order.order_id),
                            format!("radroots trade rebind {} <selector>", document.order.order_id),
                            format!("radroots trade get {}", document.order.order_id),
                        ],
                    }),
                ),
            )
            .into());
        }
    }

    Ok(account)
}

fn order_buyer_failure_detail(
    loaded: &LoadedOrderDraft,
    mut extra: serde_json::Value,
) -> serde_json::Value {
    let mut detail = json!({
        "buyer_actor_source": loaded.document.buyer_actor.source.as_str(),
        "order_buyer_account_id": loaded.document.buyer_actor.account_id.as_str(),
        "order_buyer_pubkey": loaded.document.buyer_actor.pubkey.as_str(),
        "order_file": loaded.file.display().to_string(),
        "trade_id": loaded.document.order.order_id.as_str(),
    });
    if let (Some(detail), Some(extra)) = (detail.as_object_mut(), extra.as_object_mut()) {
        for (key, value) in std::mem::take(extra) {
            detail.insert(key, value);
        }
    }
    detail
}

fn resolve_local_order_signing_identity(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    resolve_local_order_bound_buyer_signing_identity(config, loaded, "trade submit")
}

fn resolve_local_order_bound_buyer_signing_identity(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    action: &str,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "{action} requires signer mode `local`"
        )));
    }
    let account_id = loaded.document.buyer_actor.account_id.trim();
    let buyer_pubkey = loaded.document.buyer_actor.pubkey.trim();
    let signing = account::resolve_local_signing_identity_for_account(config, account_id)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            account::AccountRuntimeFailure::mismatch_with_detail(
                format!(
                    "account mismatch: order-bound buyer account `{account_id}` pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
                ),
                order_buyer_failure_detail(
                    loaded,
                    json!({
                        "attempted_buyer_account_id": signing.account.record.account_id.to_string(),
                        "attempted_buyer_pubkey": selected_pubkey,
                        "actions": [
                            format!("radroots trade rebind {} <selector>", loaded.document.order.order_id),
                            format!("radroots trade get {}", loaded.document.order.order_id),
                        ],
                    }),
                ),
            ),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_decision_signing_identity(
    config: &RuntimeConfig,
    seller_pubkey: &str,
    decision: TradeDecisionArg,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "order {} requires signer mode `local`",
            decision.command()
        )));
    }
    let signing = account::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(seller_pubkey) {
        return Err(ActorWriteBindingError::Account(
            account::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order seller_pubkey `{seller_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_revision_signing_identity(
    config: &RuntimeConfig,
    seller_pubkey: &str,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "trade revision propose requires signer mode `local`".to_owned(),
        ));
    }
    let signing = account::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(seller_pubkey) {
        return Err(ActorWriteBindingError::Account(
            account::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order seller_pubkey `{seller_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_cancellation_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "trade cancel requires signer mode `local`".to_owned(),
        ));
    }
    let signing = account::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            account::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_revision_decision_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
    args: &TradeRevisionDecisionArgs,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "trade revision {} requires signer mode `local`",
            args.decision.command()
        )));
    }
    let signing = account::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            account::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn relay_failures(failures: Vec<DirectRelayFailure>) -> Vec<RelayFailureView> {
    failures
        .into_iter()
        .map(|failure| RelayFailureView {
            relay: failure.relay,
            reason: failure.reason,
        })
        .collect()
}

fn load_draft(path: &Path) -> Result<LoadedOrderDraft, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("read trade draft {}: {error}", path.display()))?;
    let document = toml::from_str::<OrderDraftDocument>(contents.as_str())
        .map_err(|error| format!("parse trade draft {}: {error}", path.display()))?;
    Ok(LoadedOrderDraft {
        file: path.to_path_buf(),
        updated_at_unix: modified_unix(path).unwrap_or_default(),
        document,
    })
}

fn save_draft(path: &Path, draft: &OrderDraftDocument) -> Result<(), RuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, scaffold_contents(draft)?)?;
    Ok(())
}

fn scaffold_contents(draft: &OrderDraftDocument) -> Result<String, RuntimeError> {
    let toml = toml::to_string_pretty(draft)
        .map_err(|error| RuntimeError::Config(format!("render trade draft: {error}")))?;
    Ok(format!(
        "# radroots trade draft v1\n# fill listing_addr and any missing order items before submit\n\n{toml}"
    ))
}

fn drafts_dir(config: &RuntimeConfig) -> PathBuf {
    config.paths.app_data_root.join(ORDERS_DIR)
}

fn draft_lookup_path(config: &RuntimeConfig, lookup: &str) -> PathBuf {
    let candidate = PathBuf::from(lookup);
    if candidate.is_absolute() || lookup.contains(std::path::MAIN_SEPARATOR) {
        return candidate;
    }
    let file_name = if lookup.ends_with(".toml") {
        lookup.to_owned()
    } else {
        format!("{lookup}.toml")
    };
    drafts_dir(config).join(file_name)
}

#[derive(Debug, Clone)]
struct ParsedListingAddress {
    kind: u32,
    seller_pubkey: String,
    listing_id: String,
}

fn parse_listing_addr(raw: &str) -> Result<ParsedListingAddress, String> {
    let parsed = RadrootsListingAddress::parse(raw).map_err(|error| error.to_string())?;
    let (kind, rest) = parsed
        .as_str()
        .split_once(':')
        .ok_or_else(|| "listing address has invalid format".to_owned())?;
    let (seller_pubkey, listing_id) = rest
        .split_once(':')
        .ok_or_else(|| "listing address has invalid format".to_owned())?;
    let kind = kind
        .parse::<u32>()
        .map_err(|_| "listing address kind is invalid".to_owned())?;
    Ok(ParsedListingAddress {
        kind,
        seller_pubkey: seller_pubkey.to_owned(),
        listing_id: listing_id.to_owned(),
    })
}

fn issue(field: impl Into<String>, message: impl Into<String>) -> OrderIssueView {
    let field = field.into();
    issue_with_code(validation_issue_code(&field), field, message)
}

fn issue_with_code(
    code: impl Into<String>,
    field: impl Into<String>,
    message: impl Into<String>,
) -> OrderIssueView {
    OrderIssueView {
        code: code.into(),
        field: field.into(),
        message: message.into(),
        event_ids: Vec::new(),
    }
}

fn issue_with_events(
    code: impl Into<String>,
    field: impl Into<String>,
    message: impl Into<String>,
    event_ids: Vec<impl ToString>,
) -> OrderIssueView {
    let mut event_ids = event_ids
        .into_iter()
        .map(|event_id| event_id.to_string())
        .collect::<Vec<_>>();
    event_ids.sort();
    event_ids.dedup();
    OrderIssueView {
        code: code.into(),
        field: field.into(),
        message: message.into(),
        event_ids,
    }
}

fn validation_issue_code(field: &str) -> String {
    let mut code = String::new();
    let mut previous_separator = false;
    for character in field.chars() {
        if character.is_ascii_alphanumeric() {
            code.push(character.to_ascii_lowercase());
            previous_separator = false;
        } else if !previous_separator {
            code.push('_');
            previous_separator = true;
        }
    }
    let code = code.trim_matches('_');
    if code.is_empty() {
        "validation_failed".to_owned()
    } else {
        format!("{code}_invalid")
    }
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn normalize_listing_relay_set<I, S>(values: I) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    normalize_relay_urls(values).map_err(|error| error.to_string())
}

fn order_listing_relays(document: &OrderDraftDocument) -> Vec<String> {
    normalize_listing_relay_set(document.order.listing_relays.iter())
        .unwrap_or_else(|_| document.order.listing_relays.clone())
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn non_empty_ref(value: &str) -> Option<&str> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn modified_unix(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn next_order_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = ORDER_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    format!(
        "ord_{}",
        encode_base64url_no_pad((nanos ^ counter).to_be_bytes())
    )
}

fn next_revision_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = ORDER_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    format!(
        "rev_{}",
        encode_base64url_no_pad((nanos ^ counter).to_be_bytes())
    )
}

fn is_valid_order_id(value: &str) -> bool {
    if let Some(encoded) = value.strip_prefix("ord_") {
        return encoded.len() == 22 && is_d_tag_base64url(encoded);
    }
    is_canonical_uuid(value)
}

fn is_canonical_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    for (index, character) in value.chars().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if character != '-' {
                return false;
            }
        } else if !character.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn is_valid_event_id(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn encode_base64url_no_pad(bytes: [u8; 16]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(22);
    let mut index = 0usize;
    while index + 3 <= bytes.len() {
        let block = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
        output.push(ALPHABET[(block & 0x3f) as usize] as char);
        index += 3;
    }
    let remaining = bytes.len() - index;
    if remaining == 1 {
        let block = (bytes[index] as u32) << 16;
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
    } else if remaining == 2 {
        let block = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
    }
    output
}

#[derive(Debug, Clone)]
struct OrderInspection {
    state: String,
    ready_for_submit: bool,
    listing_addr: Option<String>,
    listing_event_id: Option<String>,
    seller_pubkey: Option<String>,
    buyer_custody: Option<String>,
    buyer_write_capable: Option<bool>,
    issues: Vec<OrderIssueView>,
}

impl From<OrderGetView> for OrderNewView {
    fn from(view: OrderGetView) -> Self {
        Self {
            state: "draft_created".to_owned(),
            source: view.source,
            order_id: view.order_id.unwrap_or_default(),
            file: view.file.unwrap_or_default(),
            listing_lookup: view.listing_lookup,
            listing_addr: view.listing_addr,
            listing_event_id: view.listing_event_id,
            listing_relays: view.listing_relays,
            buyer_account_id: view.buyer_account_id,
            buyer_pubkey: view.buyer_pubkey,
            buyer_actor_source: view.buyer_actor_source,
            buyer_custody: view.buyer_custody,
            buyer_write_capable: view.buyer_write_capable,
            seller_pubkey: view.seller_pubkey,
            ready_for_submit: view.ready_for_submit,
            items: view.items,
            economics: view.economics,
            issues: view.issues,
            actions: view.actions,
        }
    }
}
