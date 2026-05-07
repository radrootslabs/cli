#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_core::{
    RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreDiscount, RadrootsCoreDiscountScope,
    RadrootsCoreDiscountThreshold, RadrootsCoreDiscountValue, RadrootsCoreMoney, RadrootsCoreUnit,
    convert_unit_decimal,
};
use radroots_events::RadrootsNostrEventPtr;
use radroots_events::kinds::{
    KIND_LISTING, KIND_TRADE_CANCEL, KIND_TRADE_FULFILLMENT_UPDATE, KIND_TRADE_ORDER_DECISION,
    KIND_TRADE_ORDER_REQUEST, KIND_TRADE_ORDER_REVISION, KIND_TRADE_ORDER_REVISION_RESPONSE,
    KIND_TRADE_PAYMENT_RECORDED, KIND_TRADE_RECEIPT, KIND_TRADE_SETTLEMENT_DECISION,
};
use radroots_events::listing::{
    RadrootsListing, RadrootsListingAvailability, RadrootsListingStatus,
};
use radroots_events::trade::{
    RadrootsActiveTradeFulfillmentState, RadrootsActiveTradeMessageType, RadrootsTradeBuyerReceipt,
    RadrootsTradeEconomicActor, RadrootsTradeEconomicEffect, RadrootsTradeEconomicLineKind,
    RadrootsTradeFulfillmentUpdated, RadrootsTradeInventoryCommitment, RadrootsTradeOrderCancelled,
    RadrootsTradeOrderDecision, RadrootsTradeOrderDecisionEvent, RadrootsTradeOrderEconomicItem,
    RadrootsTradeOrderEconomicLine, RadrootsTradeOrderEconomics, RadrootsTradeOrderItem,
    RadrootsTradeOrderRequested, RadrootsTradeOrderRevisionDecision,
    RadrootsTradeOrderRevisionDecisionEvent, RadrootsTradeOrderRevisionProposed,
    RadrootsTradePaymentMethod, RadrootsTradePaymentRecorded, RadrootsTradePricingBasis,
    RadrootsTradeSettlementDecision, RadrootsTradeSettlementDecisionEvent,
};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::listing::decode::listing_from_event;
use radroots_events_codec::trade::{
    RadrootsTradeListingAddress, active_trade_buyer_receipt_event_build,
    active_trade_buyer_receipt_from_event, active_trade_envelope_from_event,
    active_trade_event_context_from_tags, active_trade_fulfillment_update_event_build,
    active_trade_fulfillment_update_from_event, active_trade_order_cancel_event_build,
    active_trade_order_cancel_from_event, active_trade_order_decision_event_build,
    active_trade_order_request_event_build, active_trade_order_request_from_event,
    active_trade_order_revision_decision_event_build,
    active_trade_order_revision_decision_from_event,
    active_trade_order_revision_proposal_event_build,
    active_trade_order_revision_proposal_from_event, active_trade_payment_recorded_event_build,
    active_trade_payment_recorded_from_event, active_trade_settlement_decision_event_build,
    active_trade_settlement_decision_from_event,
};
use radroots_events_codec::wire::WireEventParts;
use radroots_nostr::prelude::{
    RadrootsNostrEvent, RadrootsNostrFilter, radroots_event_from_nostr, radroots_nostr_filter_tag,
    radroots_nostr_kind,
};
use radroots_replica_db::{
    ReplicaSql, ReplicaTradeProductSummaryRow, nostr_event_state, trade_product,
};
use radroots_replica_db_schema::nostr_event_state::{
    INostrEventStateFindOne, INostrEventStateFindOneArgs, NostrEventStateQueryBindValues,
};
use radroots_replica_db_schema::trade_product::{
    ITradeProductFieldsFilter, ITradeProductFindMany, TradeProduct,
};
use radroots_sql_core::SqliteExecutor;
use radroots_trade::order::{
    RadrootsActiveOrderCancellationRecord, RadrootsActiveOrderDecisionRecord,
    RadrootsActiveOrderFulfillmentRecord, RadrootsActiveOrderPaymentProjection,
    RadrootsActiveOrderPaymentRecord, RadrootsActiveOrderPaymentState,
    RadrootsActiveOrderReceiptRecord, RadrootsActiveOrderReducerIssue,
    RadrootsActiveOrderRequestRecord, RadrootsActiveOrderRevisionDecisionRecord,
    RadrootsActiveOrderRevisionProposalRecord, RadrootsActiveOrderSettlementRecord,
    RadrootsActiveOrderSettlementState, RadrootsActiveOrderStatus,
    RadrootsListingInventoryAccountingIssue, RadrootsListingInventoryAccountingProjection,
    RadrootsListingInventoryBinAvailability, canonicalize_active_order_decision_for_signer,
    canonicalize_active_order_request_for_signer, radroots_trade_order_economics_digest,
    reduce_active_order_events, reduce_listing_inventory_accounting,
};
use serde::{Deserialize, Serialize};

use crate::domain::runtime::{
    OrderCancellationView, OrderDecisionView, OrderDraftItemView, OrderFulfillmentView,
    OrderGetView, OrderHistoryEntryView, OrderHistoryView, OrderInventoryBinView,
    OrderInventoryView, OrderIssueView, OrderListView, OrderNewView, OrderPaymentView,
    OrderReceiptView, OrderRevisionDecisionView, OrderRevisionProposalView, OrderSettlementView,
    OrderStatusFulfillmentView, OrderStatusLifecycleCancellationView,
    OrderStatusLifecycleReceiptView, OrderStatusLifecycleView, OrderStatusPaymentView,
    OrderStatusRevisionView, OrderStatusView, OrderSubmitView, OrderSummaryView, OrderWatchView,
    RelayFailureView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayFetchError, DirectRelayFetchReceipt, DirectRelayPublishReceipt,
    fetch_events_from_relays, publish_parts_with_identity,
};
use crate::runtime::signer::ActorWriteBindingError;
use crate::runtime_args::{
    OrderCancelArgs, OrderDecisionArg, OrderDecisionArgs, OrderDraftCreateArgs,
    OrderFulfillmentArgs, OrderPaymentArgs, OrderReceiptArgs, OrderRevisionDecisionArg,
    OrderRevisionDecisionArgs, OrderRevisionProposeArgs, OrderSettlementArgs,
    OrderSettlementDecisionArg, OrderStatusArgs, OrderSubmitArgs, OrderWatchArgs, RecordLookupArgs,
};

const ORDER_DRAFT_KIND: &str = "order_draft_v1";
const ORDER_SOURCE: &str = "local order drafts · local first";
const ORDER_SUBMIT_SOURCE: &str = "direct Nostr relay publish · local key";
const ORDER_DECISION_SOURCE: &str = "direct Nostr relay decision publish · local key";
const ORDER_REVISION_PROPOSAL_SOURCE: &str =
    "direct Nostr relay revision proposal publish · local key";
const ORDER_REVISION_DECISION_SOURCE: &str =
    "direct Nostr relay revision decision publish · local key";
const ORDER_FULFILLMENT_SOURCE: &str = "direct Nostr relay fulfillment publish · local key";
const ORDER_CANCELLATION_SOURCE: &str = "direct Nostr relay cancellation publish · local key";
const ORDER_RECEIPT_SOURCE: &str = "direct Nostr relay receipt publish · local key";
const ORDER_PAYMENT_SOURCE: &str = "direct Nostr relay payment publish · local key";
const ORDER_SETTLEMENT_SOURCE: &str = "direct Nostr relay settlement publish · local key";
const ORDER_EVENT_LIST_SOURCE: &str = "direct Nostr relay fetch · selected seller identity";
const ORDER_STATUS_SOURCE: &str = "direct Nostr relay status fetch · active order reducer";
const ORDER_EVENT_WATCH_UNAVAILABLE_REASON: &str =
    "relay-backed order event watch is not implemented";
const ORDERS_DIR: &str = "orders/drafts";

static ORDER_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraftDocument {
    version: u32,
    kind: String,
    order: OrderDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    listing_lookup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    buyer_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraft {
    order_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    listing_addr: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    listing_event_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    buyer_pubkey: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    seller_pubkey: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<OrderDraftItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    economics: Option<RadrootsTradeOrderEconomics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraftItem {
    bin_id: String,
    bin_count: u32,
}

#[derive(Debug, Clone)]
struct LoadedOrderDraft {
    file: PathBuf,
    updated_at_unix: u64,
    document: OrderDraftDocument,
}

#[derive(Debug, Clone)]
struct ResolvedOrderListing {
    listing_addr: String,
    listing_event_id: String,
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
    request_event_id: String,
    listing_event_id: Option<String>,
    order_id: String,
    listing_addr: String,
    buyer_pubkey: String,
    seller_pubkey: String,
    items: Vec<RadrootsTradeOrderItem>,
    economics: RadrootsTradeOrderEconomics,
}

#[derive(Debug, Clone)]
struct ResolvedOrderSubmitRequest {
    request_event_id: String,
    listing_event_id: Option<String>,
    payload: RadrootsTradeOrderRequested,
}

#[derive(Debug, Clone)]
struct ResolvedAccountingRequest {
    listing_event_id: Option<String>,
    record: RadrootsActiveOrderRequestRecord,
}

#[derive(Debug, Clone)]
struct ResolvedInventoryListing {
    event_id: String,
    listing: RadrootsListing,
    bins: Vec<RadrootsListingInventoryBinAvailability>,
}

#[derive(Debug, Clone)]
struct OrderDecisionInventoryPreflight {
    invalid_view: Option<OrderDecisionView>,
    inventory: Option<OrderInventoryView>,
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

    let selected_account = accounts::resolve_account(config)?;
    let buyer_account_id = selected_account
        .as_ref()
        .map(|account| account.record.account_id.to_string());
    let buyer_pubkey = selected_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.clone())
        .unwrap_or_default();

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
            buyer_pubkey,
            seller_pubkey,
            items,
            economics,
        },
        listing_lookup,
        buyer_account_id,
    };
    save_draft(file.as_path(), &document)?;

    let mut view: OrderNewView = view_from_loaded(LoadedOrderDraft {
        file,
        updated_at_unix: now_unix(),
        document,
    })
    .into();
    view.actions
        .insert(0, format!("radroots order get {}", view.order_id));

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

    let selected_account = accounts::resolve_account(config)?;
    let buyer_account_id = selected_account
        .as_ref()
        .map(|account| account.record.account_id.to_string());
    let buyer_pubkey = selected_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.clone())
        .unwrap_or_default();

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
            buyer_pubkey,
            seller_pubkey,
            items,
            economics,
        },
        listing_lookup,
        buyer_account_id,
    };

    let mut view: OrderNewView = view_from_loaded(LoadedOrderDraft {
        file,
        updated_at_unix: now_unix(),
        document,
    })
    .into();
    view.state = "dry_run".to_owned();
    view.actions
        .insert(0, format!("radroots order get {}", view.order_id));

    Ok(view)
}

pub fn get(config: &RuntimeConfig, args: &RecordLookupArgs) -> Result<OrderGetView, RuntimeError> {
    let lookup = args.key.clone();
    let file = draft_lookup_path(config, lookup.as_str());
    if !file.exists() {
        return Ok(OrderGetView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup: lookup.clone(),
            order_id: None,
            file: Some(file.display().to_string()),
            listing_lookup: None,
            listing_addr: None,
            listing_event_id: None,
            buyer_account_id: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            ready_for_submit: false,
            items: Vec::new(),
            economics: None,
            updated_at_unix: None,
            job: None,
            workflow: None,
            reason: Some(format!("order draft `{lookup}` was not found")),
            issues: Vec::new(),
            actions: vec![
                "radroots order list".to_owned(),
                "radroots basket create".to_owned(),
            ],
        });
    }

    match load_draft(file.as_path()) {
        Ok(loaded) => Ok(view_from_loaded(loaded)),
        Err(reason) => Ok(OrderGetView {
            state: "error".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup,
            order_id: None,
            file: Some(file.display().to_string()),
            listing_lookup: None,
            listing_addr: None,
            listing_event_id: None,
            buyer_account_id: None,
            buyer_pubkey: None,
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
    if !dir.exists() {
        return Ok(OrderListView {
            state: "empty".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            count: 0,
            orders: Vec::new(),
            actions: vec!["radroots basket create".to_owned()],
        });
    }

    let mut orders = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        match load_draft(path.as_path()) {
            Ok(loaded) => orders.push(summary_from_loaded(&loaded)),
            Err(reason) => orders.push(summary_for_invalid_file(path.as_path(), reason)),
        }
    }

    orders.sort_by(|left, right| {
        right
            .updated_at_unix
            .cmp(&left.updated_at_unix)
            .then_with(|| left.id.cmp(&right.id))
    });

    let state = if orders.is_empty() {
        "empty"
    } else if orders.iter().any(|order| order.state == "error") {
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

pub fn submit(
    config: &RuntimeConfig,
    args: &OrderSubmitArgs,
) -> Result<OrderSubmitView, RuntimeError> {
    let file = draft_lookup_path(config, args.key.as_str());
    if !file.exists() {
        return Ok(OrderSubmitView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            order_id: args.key.clone(),
            file: file.display().to_string(),
            listing_lookup: None,
            listing_addr: None,
            listing_event_id: None,
            buyer_account_id: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            event_id: None,
            event_kind: None,
            dry_run: config.output.dry_run,
            deduplicated: false,
            target_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: None,
            reason: Some(format!("order draft `{}` was not found", args.key)),
            job: None,
            issues: Vec::new(),
            actions: vec![
                "radroots order list".to_owned(),
                "radroots basket create".to_owned(),
            ],
        });
    }

    let loaded = match load_draft(file.as_path()) {
        Ok(loaded) => loaded,
        Err(reason) => {
            return Ok(OrderSubmitView {
                state: "error".to_owned(),
                source: ORDER_SOURCE.to_owned(),
                order_id: args.key.clone(),
                file: file.display().to_string(),
                listing_lookup: None,
                listing_addr: None,
                listing_event_id: None,
                buyer_account_id: None,
                buyer_pubkey: None,
                seller_pubkey: None,
                event_id: None,
                event_kind: None,
                dry_run: config.output.dry_run,
                deduplicated: false,
                target_relays: Vec::new(),
                acknowledged_relays: Vec::new(),
                failed_relays: Vec::new(),
                idempotency_key: args.idempotency_key.clone(),
                signer_mode: None,
                signer_session_id: None,
                requested_signer_session_id: None,
                reason: Some(reason),
                job: None,
                issues: Vec::new(),
                actions: Vec::new(),
            });
        }
    };

    let issues = collect_issues(&loaded.document);
    if !issues.is_empty() {
        let mut actions = actions_for_document(&loaded.document, loaded.file.as_path(), &issues);
        actions.push(format!(
            "radroots order get {}",
            loaded.document.order.order_id
        ));
        return Ok(OrderSubmitView {
            state: "unconfigured".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
            buyer_account_id: loaded.document.buyer_account_id.clone(),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            event_id: None,
            event_kind: None,
            dry_run: config.output.dry_run,
            deduplicated: false,
            target_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: None,
            reason: Some("order draft is not ready for submit".to_owned()),
            job: None,
            issues,
            actions,
        });
    }

    if let Some(view) = order_submit_listing_freshness_view(config, &loaded, args)? {
        return Ok(view);
    }
    if let Some(view) = order_submit_quantity_preflight_view(config, &loaded, args)? {
        return Ok(view);
    }

    let signing = match resolve_local_order_signing_identity(
        config,
        loaded.document.order.buyer_pubkey.as_str(),
    ) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => return Ok(order_binding_error_view(config, &loaded, args, error)),
    };
    let payload = canonical_order_request_payload_from_loaded(
        &loaded,
        signing
            .account
            .record
            .public_identity
            .public_key_hex
            .as_str(),
    )?;

    if config.relay.urls.is_empty() {
        return Err(RuntimeError::Network(
            "order submit requires at least one configured relay before publish preflight"
                .to_owned(),
        ));
    }

    if let Some(view) =
        order_submit_existing_request_preflight_view(config, &loaded, args, &payload)?
    {
        return Ok(view);
    }

    if config.output.dry_run {
        return Ok(order_submit_dry_run_view(config, &loaded, args));
    }

    match publish_order_request(config, &loaded, args, signing, payload) {
        Ok(view) => Ok(view),
        Err(error) => Err(error),
    }
}

pub fn watch(
    config: &RuntimeConfig,
    args: &OrderWatchArgs,
) -> Result<OrderWatchView, RuntimeError> {
    if args.frames == Some(0) {
        return Err(RuntimeError::Config(
            "--frames must be greater than zero when provided".to_owned(),
        ));
    }

    let file = draft_lookup_path(config, args.key.as_str());
    if !file.exists() {
        return Ok(OrderWatchView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            order_id: args.key.clone(),
            job_id: None,
            interval_ms: args.interval_ms,
            reason: Some(format!("order draft `{}` was not found", args.key)),
            workflow: None,
            frames: Vec::new(),
            actions: vec!["radroots order list".to_owned()],
        });
    }

    let loaded = match load_draft(file.as_path()) {
        Ok(loaded) => loaded,
        Err(reason) => {
            return Ok(OrderWatchView {
                state: "error".to_owned(),
                source: ORDER_SOURCE.to_owned(),
                order_id: args.key.clone(),
                job_id: None,
                interval_ms: args.interval_ms,
                reason: Some(reason),
                workflow: None,
                frames: Vec::new(),
                actions: Vec::new(),
            });
        }
    };

    Ok(OrderWatchView {
        state: "unavailable".to_owned(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        job_id: None,
        interval_ms: args.interval_ms,
        reason: Some(ORDER_EVENT_WATCH_UNAVAILABLE_REASON.to_owned()),
        workflow: None,
        frames: Vec::new(),
        actions: vec![format!(
            "radroots order get {}",
            loaded.document.order.order_id
        )],
    })
}

pub fn history(
    config: &RuntimeConfig,
    order_id: Option<&str>,
) -> Result<OrderHistoryView, RuntimeError> {
    if config.relay.urls.is_empty() {
        return Ok(order_history_unconfigured(
            None,
            "order event list requires at least one configured relay".to_owned(),
            Vec::new(),
        ));
    }

    let seller = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            return Ok(order_history_unconfigured(
                None,
                "order event list requires a selected seller account".to_owned(),
                config.relay.urls.clone(),
            ));
        }
    };
    let seller_pubkey = seller.record.public_identity.public_key_hex;
    let filter = order_request_filter(seller_pubkey.as_str(), order_id)?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            return Ok(order_history_unavailable(
                seller_pubkey,
                reason,
                target_relays,
                failed_relays,
            ));
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    Ok(order_history_from_receipt(seller_pubkey, order_id, receipt))
}

pub fn decide(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
) -> Result<OrderDecisionView, RuntimeError> {
    if config.relay.urls.is_empty() {
        let mut view =
            order_decision_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason = Some(format!(
            "order {} requires at least one configured relay",
            args.decision.command()
        ));
        return Ok(view);
    }

    let seller = match accounts::resolve_account(config)? {
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
    let seller_pubkey = seller.record.public_identity.public_key_hex;
    let filter = order_request_filter(seller_pubkey.as_str(), Some(args.key.as_str()))?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_decision_base_view(config, args, "unavailable", config.output.dry_run);
            view.seller_pubkey = Some(seller_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let resolution = seller_order_request_resolution_from_receipt(
        seller_pubkey.as_str(),
        args.key.as_str(),
        receipt,
    )?;
    if !resolution.candidate_issues.is_empty() {
        return Ok(order_decision_view_from_resolution(
            config,
            args,
            seller_pubkey,
            resolution,
        ));
    }
    if resolution.requests.len() == 1 {
        let request = resolution.requests[0].clone();
        let status_view = status(
            config,
            &OrderStatusArgs {
                key: args.key.clone(),
            },
        )?;
        if let Some(view) = order_decision_preflight_view_from_status(
            config,
            args,
            &request,
            &resolution,
            &status_view,
        ) {
            return Ok(view);
        }
        let inventory_preflight = order_accept_inventory_preflight_view(
            config,
            args,
            &request,
            &resolution,
            &status_view,
        )?;
        if let Some(view) = inventory_preflight.invalid_view {
            return Ok(view);
        }
        let signing = match resolve_local_order_decision_signing_identity(
            config,
            request.seller_pubkey.as_str(),
            args.decision,
        ) {
            Ok(signing) => signing,
            Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
            Err(error) => {
                return Ok(order_decision_binding_error_view(
                    config, args, request, resolution, error,
                ));
            }
        };
        let payload = {
            let signer_pubkey = signing
                .account
                .record
                .public_identity
                .public_key_hex
                .as_str();
            canonical_order_decision_payload(args, &request, signer_pubkey)?
        };
        if config.output.dry_run {
            return Ok(order_decision_dry_run_view(
                config,
                args,
                &request,
                &status_view,
                inventory_preflight.inventory,
            ));
        }
        return publish_order_decision(
            config,
            args,
            request,
            resolution,
            signing,
            payload,
            inventory_preflight.inventory,
        );
    }
    Ok(order_decision_view_from_resolution(
        config,
        args,
        seller_pubkey,
        resolution,
    ))
}

pub fn revision_propose(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
) -> Result<OrderRevisionProposalView, RuntimeError> {
    if let Some(view) = order_revision_args_preflight_view(config, args) {
        return Ok(view);
    }
    if config.relay.urls.is_empty() {
        let mut view =
            order_revision_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason =
            Some("order revision propose requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let seller = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_revision_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason =
                Some("order revision propose requires a selected seller account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = seller.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_revision_base_view(config, args, "unavailable", config.output.dry_run);
            view.seller_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let revision_candidates =
        order_revision_proposals_from_events(args.key.as_str(), receipt.events.as_slice());
    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let mut status_view = reduction.view;
    enrich_order_status_inventory(config, &mut status_view)?;
    if let Some(view) = order_revision_preflight_view_from_status(
        config,
        args,
        &status_view,
        selected_pubkey.as_str(),
        &revision_candidates,
    ) {
        return Ok(view);
    }

    let seller_pubkey = status_view.seller_pubkey.as_deref().ok_or_else(|| {
        RuntimeError::Config("accepted order is missing seller_pubkey".to_owned())
    })?;
    let signing = match resolve_local_order_fulfillment_signing_identity(config, seller_pubkey) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => {
            return Ok(order_revision_binding_error_view(
                config,
                args,
                &status_view,
                error,
            ));
        }
    };
    let payload = match order_revision_payload_from_status(args, &status_view) {
        Ok(payload) => payload,
        Err(error) => {
            return Ok(order_revision_invalid_view(
                config,
                args,
                &status_view,
                format!(
                    "order revision propose inputs for `{}` are invalid",
                    args.key
                ),
                vec![issue_with_code(
                    "revision_payload_invalid",
                    "revision",
                    error.to_string(),
                )],
            ));
        }
    };
    if let Some(view) =
        order_revision_inventory_preflight_view(config, args, &status_view, &payload)
    {
        return Ok(view);
    }
    let _ = order_revision_event_parts(&status_view, &payload)?;
    if config.output.dry_run {
        return Ok(order_revision_dry_run_view(
            config,
            args,
            &status_view,
            &payload,
        ));
    }
    publish_order_revision(config, args, status_view, signing, payload)
}

pub fn revision_decide(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
) -> Result<OrderRevisionDecisionView, RuntimeError> {
    if let Some(view) = order_revision_decision_args_preflight_view(config, args) {
        return Ok(view);
    }
    if config.relay.urls.is_empty() {
        let mut view =
            order_revision_decision_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason =
            Some("order revision decision requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let buyer = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view = order_revision_decision_base_view(
                config,
                args,
                "unconfigured",
                config.output.dry_run,
            );
            view.reason =
                Some("order revision decision requires a selected buyer account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = buyer.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view = order_revision_decision_base_view(
                config,
                args,
                "unavailable",
                config.output.dry_run,
            );
            view.buyer_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let revision_candidates =
        order_revision_proposals_from_events(args.key.as_str(), receipt.events.as_slice());
    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let mut status_view = reduction.view;
    enrich_order_status_inventory(config, &mut status_view)?;
    if let Some(view) = order_revision_decision_preflight_view_from_status(
        config,
        args,
        &status_view,
        selected_pubkey.as_str(),
        &revision_candidates,
    ) {
        return Ok(view);
    }

    let proposal = pending_revision_proposal_candidate(&status_view, &revision_candidates)
        .ok_or_else(|| {
            RuntimeError::Config("accepted order is missing pending revision proposal".to_owned())
        })?;
    if proposal.payload.revision_id != args.revision_id.trim() {
        let mut view = order_revision_decision_invalid_view(
            config,
            args,
            &status_view,
            format!(
                "order revision {} refused because revision `{}` is not the latest pending proposal",
                args.decision.command(),
                args.revision_id.trim()
            ),
            vec![issue_with_events(
                "revision_id_not_pending",
                "revision_id",
                format!(
                    "latest pending revision is `{}`",
                    proposal.payload.revision_id
                ),
                vec![proposal.event_id.clone()],
            )],
        );
        apply_order_revision_decision_proposal(&mut view, proposal);
        return Ok(view);
    }

    let buyer_pubkey = status_view
        .buyer_pubkey
        .as_deref()
        .ok_or_else(|| RuntimeError::Config("accepted order is missing buyer_pubkey".to_owned()))?;
    let signing =
        match resolve_local_order_revision_decision_signing_identity(config, buyer_pubkey, args) {
            Ok(signing) => signing,
            Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
            Err(error) => {
                return Ok(order_revision_decision_binding_error_view(
                    config,
                    args,
                    &status_view,
                    error,
                ));
            }
        };
    if args.decision == OrderRevisionDecisionArg::Accept {
        let issues = order_revision_inventory_issues(&status_view, &proposal.payload);
        if !issues.is_empty() {
            let mut view = order_revision_decision_invalid_view(
                config,
                args,
                &status_view,
                "order revision accept refused because visible inventory is unavailable for the revised items",
                issues,
            );
            apply_order_revision_decision_proposal(&mut view, proposal);
            return Ok(view);
        }
    }
    let payload = order_revision_decision_payload_from_proposal(args, proposal)?;
    let _ = order_revision_decision_event_parts(&payload)?;
    if config.output.dry_run {
        return Ok(order_revision_decision_dry_run_view(
            config,
            args,
            &status_view,
            proposal,
            &payload,
        ));
    }
    publish_order_revision_decision(config, args, status_view, proposal, signing, payload)
}

pub fn fulfillment_update(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
) -> Result<OrderFulfillmentView, RuntimeError> {
    if config.relay.urls.is_empty() {
        let mut view =
            order_fulfillment_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason =
            Some("order fulfillment update requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let fulfillment_state = match parse_fulfillment_state(args.state.as_str()) {
        Ok(state) if state.is_publishable_update() => state,
        Ok(_) => {
            let mut view =
                order_fulfillment_base_view(config, args, "invalid", config.output.dry_run);
            view.fulfillment_state =
                fulfillment_state_name(RadrootsActiveTradeFulfillmentState::AcceptedNotFulfilled)
                    .to_owned();
            view.reason = Some(
                "`accepted_not_fulfilled` is derived from an accepted order and cannot be published"
                    .to_owned(),
            );
            view.issues = vec![issue_with_code(
                "fulfillment_state_not_publishable",
                "fulfillment_state",
                "accepted_not_fulfilled cannot be published as a fulfillment update",
            )];
            return Ok(view);
        }
        Err(reason) => {
            let mut view =
                order_fulfillment_base_view(config, args, "invalid", config.output.dry_run);
            view.reason = Some(reason);
            view.issues = vec![issue_with_code(
                "unsupported_fulfillment_state",
                "fulfillment_state",
                "fulfillment state is not part of the active protocol set",
            )];
            return Ok(view);
        }
    };

    let selected_account = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_fulfillment_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason =
                Some("order fulfillment update requires a selected seller account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = selected_account.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_fulfillment_base_view(config, args, "unavailable", config.output.dry_run);
            view.seller_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let status_view = reduction.view;
    if let Some(view) = order_fulfillment_preflight_view_from_status(
        config,
        args,
        &status_view,
        reduction.fulfillment_status,
        reduction.fulfillment_event_id.as_deref(),
    ) {
        return Ok(view);
    }

    let seller_pubkey = status_view.seller_pubkey.as_deref().ok_or_else(|| {
        RuntimeError::Config("accepted order is missing seller_pubkey".to_owned())
    })?;
    let signing = match resolve_local_order_fulfillment_signing_identity(config, seller_pubkey) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => {
            return Ok(order_fulfillment_binding_error_view(
                config,
                args,
                &status_view,
                error,
            ));
        }
    };
    let payload = order_fulfillment_payload_from_status(&status_view, fulfillment_state)?;
    let _ = order_fulfillment_event_parts(&status_view, &payload)?;
    if config.output.dry_run {
        return Ok(order_fulfillment_dry_run_view(
            config,
            args,
            &status_view,
            fulfillment_state,
        ));
    }
    publish_order_fulfillment(config, args, status_view, signing, payload)
}

pub fn cancel(
    config: &RuntimeConfig,
    args: &OrderCancelArgs,
) -> Result<OrderCancellationView, RuntimeError> {
    if config.relay.urls.is_empty() {
        let mut view =
            order_cancellation_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason = Some("order cancel requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let selected_account = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_cancellation_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason = Some("order cancel requires a selected buyer account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = selected_account.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_cancellation_base_view(config, args, "unavailable", config.output.dry_run);
            view.buyer_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let status_view = reduction.view;
    if let Some(view) = order_cancellation_preflight_view_from_status(
        config,
        args,
        &status_view,
        selected_pubkey.as_str(),
    ) {
        return Ok(view);
    }

    let buyer_pubkey = status_view
        .buyer_pubkey
        .as_deref()
        .ok_or_else(|| RuntimeError::Config("order is missing buyer_pubkey".to_owned()))?;
    let signing = match resolve_local_order_cancellation_signing_identity(config, buyer_pubkey) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => {
            return Ok(order_cancellation_binding_error_view(
                config,
                args,
                &status_view,
                error,
            ));
        }
    };
    let payload = order_cancellation_payload_from_status(args, &status_view)?;
    let _ = order_cancellation_event_parts(&status_view, &payload)?;
    if config.output.dry_run {
        return Ok(order_cancellation_dry_run_view(config, args, &status_view));
    }
    publish_order_cancellation(config, args, status_view, signing, payload)
}

pub fn receipt_record(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
) -> Result<OrderReceiptView, RuntimeError> {
    if let Some(view) = order_receipt_args_preflight_view(config, args) {
        return Ok(view);
    }
    if config.relay.urls.is_empty() {
        let mut view = order_receipt_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason =
            Some("order receipt record requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let selected_account = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_receipt_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason = Some("order receipt record requires a selected buyer account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = selected_account.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_receipt_base_view(config, args, "unavailable", config.output.dry_run);
            view.buyer_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let status_view = reduction.view;
    if let Some(view) = order_receipt_preflight_view_from_status(
        config,
        args,
        &status_view,
        selected_pubkey.as_str(),
    ) {
        return Ok(view);
    }

    let buyer_pubkey = status_view.buyer_pubkey.as_deref().ok_or_else(|| {
        RuntimeError::Config("receiptable order is missing buyer_pubkey".to_owned())
    })?;
    let signing = match resolve_local_order_receipt_signing_identity(config, buyer_pubkey) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => {
            return Ok(order_receipt_binding_error_view(
                config,
                args,
                &status_view,
                error,
            ));
        }
    };
    let payload = order_receipt_payload_from_status(args, &status_view)?;
    let _ = order_receipt_event_parts(&status_view, &payload)?;
    if config.output.dry_run {
        return Ok(order_receipt_dry_run_view(
            config,
            args,
            &status_view,
            &payload,
        ));
    }
    publish_order_receipt(config, args, status_view, signing, payload)
}

pub fn payment_record(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
) -> Result<OrderPaymentView, RuntimeError> {
    if let Some(view) = order_payment_args_preflight_view(config, args) {
        return Ok(view);
    }
    if config.relay.urls.is_empty() {
        let mut view = order_payment_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason =
            Some("order payment record requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let selected_account = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_payment_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason = Some("order payment record requires a selected buyer account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = selected_account.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_payment_base_view(config, args, "unavailable", config.output.dry_run);
            view.buyer_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let status_view = reduction.view;
    if let Some(view) = order_payment_preflight_view_from_status(
        config,
        args,
        &status_view,
        selected_pubkey.as_str(),
    ) {
        return Ok(view);
    }

    let buyer_pubkey = status_view
        .buyer_pubkey
        .as_deref()
        .ok_or_else(|| RuntimeError::Config("payable order is missing buyer_pubkey".to_owned()))?;
    let signing = match resolve_local_order_payment_signing_identity(config, buyer_pubkey) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => {
            return Ok(order_payment_binding_error_view(
                config,
                args,
                &status_view,
                error,
            ));
        }
    };
    let payload = order_payment_payload_from_status(args, &status_view)?;
    let _ = order_payment_event_parts(&status_view, &payload)?;
    if config.output.dry_run {
        return Ok(order_payment_dry_run_view(
            config,
            args,
            &status_view,
            &payload,
        ));
    }
    publish_order_payment(config, args, status_view, signing, payload)
}

pub fn settlement_decision(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
) -> Result<OrderSettlementView, RuntimeError> {
    if let Some(view) = order_settlement_args_preflight_view(config, args) {
        return Ok(view);
    }
    if config.relay.urls.is_empty() {
        let mut view =
            order_settlement_base_view(config, args, "unconfigured", config.output.dry_run);
        view.reason =
            Some("order settlement decision requires at least one configured relay".to_owned());
        return Ok(view);
    }

    let selected_account = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            let mut view =
                order_settlement_base_view(config, args, "unconfigured", config.output.dry_run);
            view.reason =
                Some("order settlement decision requires a selected seller account".to_owned());
            view.actions = vec!["radroots account create".to_owned()];
            return Ok(view);
        }
    };
    let selected_pubkey = selected_account.record.public_identity.public_key_hex;
    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_settlement_base_view(config, args, "unavailable", config.output.dry_run);
            view.seller_pubkey = Some(selected_pubkey);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(view);
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let reduction = order_status_reduction_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey: Some(selected_pubkey.as_str()),
        },
        receipt,
    );
    let status_view = reduction.view;
    if let Some(view) = order_settlement_preflight_view_from_status(
        config,
        args,
        &status_view,
        selected_pubkey.as_str(),
    ) {
        return Ok(view);
    }

    let seller_pubkey = status_view.seller_pubkey.as_deref().ok_or_else(|| {
        RuntimeError::Config("settleable order is missing seller_pubkey".to_owned())
    })?;
    let signing = match resolve_local_order_settlement_signing_identity(config, seller_pubkey) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
        Err(error) => {
            return Ok(order_settlement_binding_error_view(
                config,
                args,
                &status_view,
                error,
            ));
        }
    };
    let payload = order_settlement_payload_from_status(args, &status_view)?;
    let _ = order_settlement_event_parts(&status_view, &payload)?;
    if config.output.dry_run {
        return Ok(order_settlement_dry_run_view(
            config,
            args,
            &status_view,
            &payload,
        ));
    }
    publish_order_settlement(config, args, status_view, signing, payload)
}

pub fn status(
    config: &RuntimeConfig,
    args: &OrderStatusArgs,
) -> Result<OrderStatusView, RuntimeError> {
    if config.relay.urls.is_empty() {
        return Ok(OrderStatusView {
            state: "unconfigured".to_owned(),
            source: ORDER_STATUS_SOURCE.to_owned(),
            order_id: args.key.clone(),
            request_event_id: None,
            decision_event_id: None,
            agreement_event_id: None,
            listing_event_id: None,
            listing_addr: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            economics: None,
            last_event_id: None,
            revision: None,
            inventory: None,
            fulfillment: None,
            lifecycle: None,
            payment: None,
            reducer_issues: Vec::new(),
            target_relays: Vec::new(),
            connected_relays: Vec::new(),
            failed_relays: Vec::new(),
            fetched_count: 0,
            decoded_count: 0,
            skipped_count: 0,
            reason: Some("order status get requires at least one configured relay".to_owned()),
            actions: Vec::new(),
        });
    }

    let filter = order_status_filter(args.key.as_str())?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            return Ok(OrderStatusView {
                state: "unavailable".to_owned(),
                source: ORDER_STATUS_SOURCE.to_owned(),
                order_id: args.key.clone(),
                request_event_id: None,
                decision_event_id: None,
                agreement_event_id: None,
                listing_event_id: None,
                listing_addr: None,
                buyer_pubkey: None,
                seller_pubkey: None,
                economics: None,
                last_event_id: None,
                revision: None,
                inventory: None,
                fulfillment: None,
                lifecycle: None,
                payment: None,
                reducer_issues: Vec::new(),
                target_relays,
                connected_relays: Vec::new(),
                failed_relays: relay_failures(failed_relays),
                fetched_count: 0,
                decoded_count: 0,
                skipped_count: 0,
                reason: Some(format!("direct relay connection failed: {reason}")),
                actions: Vec::new(),
            });
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let selected_account = accounts::resolve_account(config)?;
    let selected_account_pubkey = selected_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.as_str());
    let mut view = order_status_from_receipt_with_context(
        OrderStatusContext {
            order_id: args.key.as_str(),
            selected_account_pubkey,
        },
        receipt,
    );
    enrich_order_status_inventory(config, &mut view)?;
    Ok(view)
}

enum OrderStatusRecord {
    Request {
        listing_event_id: Option<String>,
        record: RadrootsActiveOrderRequestRecord,
    },
    Decision(RadrootsActiveOrderDecisionRecord),
    RevisionProposal(OrderRevisionProposalRecord),
    RevisionDecision(OrderRevisionDecisionRecord),
    Fulfillment(RadrootsActiveOrderFulfillmentRecord),
    Cancellation(RadrootsActiveOrderCancellationRecord),
    Receipt(RadrootsActiveOrderReceiptRecord),
    Payment(RadrootsActiveOrderPaymentRecord),
    Settlement(RadrootsActiveOrderSettlementRecord),
}

type OrderRevisionProposalRecord = RadrootsActiveOrderRevisionProposalRecord;
type OrderRevisionDecisionRecord = RadrootsActiveOrderRevisionDecisionRecord;

#[derive(Debug, Clone)]
struct OrderRevisionProposalCandidates {
    records: Vec<OrderRevisionProposalRecord>,
    issues: Vec<OrderIssueView>,
}

#[derive(Debug, Clone)]
struct OrderStatusReduction {
    view: OrderStatusView,
    fulfillment_event_id: Option<String>,
    fulfillment_status: Option<RadrootsActiveTradeFulfillmentState>,
}

#[derive(Debug, Clone, Copy)]
struct OrderRequestCandidateContext<'a> {
    order_id: &'a str,
    seller_pubkey: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
struct OrderStatusContext<'a> {
    order_id: &'a str,
    selected_account_pubkey: Option<&'a str>,
}

#[cfg(test)]
fn order_status_from_receipt(order_id: &str, receipt: DirectRelayFetchReceipt) -> OrderStatusView {
    order_status_from_receipt_with_context(
        OrderStatusContext {
            order_id,
            selected_account_pubkey: None,
        },
        receipt,
    )
}

#[cfg(test)]
fn order_status_from_receipt_with_deferred_payment(
    order_id: &str,
    receipt: DirectRelayFetchReceipt,
) -> OrderStatusView {
    order_status_reduction_from_receipt_inner(
        OrderStatusContext {
            order_id,
            selected_account_pubkey: None,
        },
        receipt,
        true,
    )
    .view
}

fn order_status_from_receipt_with_context(
    context: OrderStatusContext<'_>,
    receipt: DirectRelayFetchReceipt,
) -> OrderStatusView {
    order_status_reduction_from_receipt_with_context(context, receipt).view
}

fn order_status_reduction_from_receipt_with_context(
    context: OrderStatusContext<'_>,
    receipt: DirectRelayFetchReceipt,
) -> OrderStatusReduction {
    order_status_reduction_from_receipt_inner(context, receipt, false)
}

fn order_status_reduction_from_receipt_inner(
    context: OrderStatusContext<'_>,
    receipt: DirectRelayFetchReceipt,
    include_deferred_payment: bool,
) -> OrderStatusReduction {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        events,
    } = receipt;
    let fetched_count = events.len();
    let mut decoded_count = 0usize;
    let mut skipped_count = 0usize;
    let mut requests = Vec::new();
    let mut decisions = Vec::new();
    let mut revision_proposals = Vec::new();
    let mut revision_decisions = Vec::new();
    let mut fulfillments = Vec::new();
    let mut cancellations = Vec::new();
    let mut receipts = Vec::new();
    let mut payments = Vec::new();
    let mut settlements = Vec::new();
    let mut request_listing_events = Vec::new();
    let mut candidate_issues = Vec::new();

    for event in events {
        if !include_deferred_payment && deferred_payment_status_event(&event) {
            skipped_count += 1;
            continue;
        }
        match order_status_record_from_event(&event) {
            Ok(OrderStatusRecord::Request {
                listing_event_id,
                record,
            }) => {
                if !order_status_request_matches_context(&record, context) {
                    skipped_count += 1;
                    continue;
                }
                decoded_count += 1;
                request_listing_events.push((record.event_id.clone(), listing_event_id));
                requests.push(record);
            }
            Ok(OrderStatusRecord::Decision(record)) => {
                decoded_count += 1;
                decisions.push(record);
            }
            Ok(OrderStatusRecord::RevisionProposal(record)) => {
                decoded_count += 1;
                revision_proposals.push(record);
            }
            Ok(OrderStatusRecord::RevisionDecision(record)) => {
                decoded_count += 1;
                revision_decisions.push(record);
            }
            Ok(OrderStatusRecord::Fulfillment(record)) => {
                decoded_count += 1;
                fulfillments.push(record);
            }
            Ok(OrderStatusRecord::Cancellation(record)) => {
                decoded_count += 1;
                cancellations.push(record);
            }
            Ok(OrderStatusRecord::Receipt(record)) => {
                decoded_count += 1;
                receipts.push(record);
            }
            Ok(OrderStatusRecord::Payment(record)) => {
                decoded_count += 1;
                payments.push(record);
            }
            Ok(OrderStatusRecord::Settlement(record)) => {
                decoded_count += 1;
                settlements.push(record);
            }
            Err(error) => {
                skipped_count += 1;
                if order_status_request_candidate(&event, context) {
                    let event_id = event.id.to_string();
                    candidate_issues.push(issue_with_events(
                        "invalid_request_candidate",
                        "request_event_id",
                        format!(
                            "request event `{event_id}` failed order status validation: {error}"
                        ),
                        vec![event_id],
                    ));
                }
            }
        }
    }
    candidate_issues.sort_by(|left, right| {
        left.event_ids
            .cmp(&right.event_ids)
            .then_with(|| left.message.cmp(&right.message))
    });

    let order_id = context.order_id;
    let revision_proposal_records = revision_proposals.clone();
    let revision_decision_records = revision_decisions.clone();
    let fulfillment_records = fulfillments.clone();
    let cancellation_records = cancellations.clone();
    let receipt_records = receipts.clone();
    let projection = reduce_active_order_events(
        order_id,
        requests,
        decisions.clone(),
        revision_proposals,
        revision_decisions,
        fulfillments,
        cancellations,
        receipts,
        payments,
        settlements,
    );
    let fulfillment_event_id = projection.fulfillment_event_id.clone();
    let fulfillment_status = projection.fulfillment_status;
    let fulfillment_root_event_id = fulfillment_event_id.as_ref().and_then(|event_id| {
        fulfillment_records
            .iter()
            .find(|record| &record.event_id == event_id)
            .map(|record| record.root_event_id.clone())
    });
    let fulfillment_prev_event_id = fulfillment_event_id.as_ref().and_then(|event_id| {
        fulfillment_records
            .iter()
            .find(|record| &record.event_id == event_id)
            .map(|record| record.prev_event_id.clone())
    });
    let cancellation_root_event_id =
        projection
            .cancellation_event_id
            .as_ref()
            .and_then(|event_id| {
                cancellation_records
                    .iter()
                    .find(|record| &record.event_id == event_id)
                    .map(|record| record.root_event_id.clone())
            });
    let cancellation_prev_event_id =
        projection
            .cancellation_event_id
            .as_ref()
            .and_then(|event_id| {
                cancellation_records
                    .iter()
                    .find(|record| &record.event_id == event_id)
                    .map(|record| record.prev_event_id.clone())
            });
    let cancellation_reason = projection
        .cancellation_event_id
        .as_ref()
        .and_then(|event_id| {
            cancellation_records
                .iter()
                .find(|record| &record.event_id == event_id)
                .map(|record| record.payload.reason.clone())
        });
    let receipt_root_event_id = projection.receipt_event_id.as_ref().and_then(|event_id| {
        receipt_records
            .iter()
            .find(|record| &record.event_id == event_id)
            .map(|record| record.root_event_id.clone())
    });
    let receipt_prev_event_id = projection.receipt_event_id.as_ref().and_then(|event_id| {
        receipt_records
            .iter()
            .find(|record| &record.event_id == event_id)
            .map(|record| record.prev_event_id.clone())
    });
    let listing_event_id = projection
        .request_event_id
        .as_ref()
        .and_then(|request_event_id| {
            request_listing_events
                .iter()
                .find(|(event_id, _)| event_id == request_event_id)
                .and_then(|(_, listing_event_id)| listing_event_id.clone())
        });
    let mut state = active_order_status_state(&projection.status).to_owned();
    let mut reason = active_order_status_reason(&projection.status, order_id);
    let mut reducer_issues = projection
        .issues
        .into_iter()
        .map(active_order_reducer_issue_view)
        .collect::<Vec<_>>();
    if !candidate_issues.is_empty() {
        state = "invalid".to_owned();
        reason = Some(format!(
            "active order request candidates for `{order_id}` failed status validation"
        ));
        reducer_issues.extend(candidate_issues);
    }
    let inventory = order_status_inventory_view(
        &projection.status,
        listing_event_id.clone(),
        projection.decision_event_id.as_deref(),
        &decisions,
        reducer_issues.as_slice(),
    );
    let fulfillment = order_status_fulfillment_view(
        &projection.status,
        projection.request_event_id.clone(),
        projection.decision_event_id.clone(),
        fulfillment_event_id.clone(),
        fulfillment_root_event_id.clone(),
        fulfillment_prev_event_id.clone(),
        fulfillment_status,
        reducer_issues.as_slice(),
    );
    let lifecycle = order_status_lifecycle_view(
        &projection.status,
        projection.request_event_id.clone(),
        projection.last_event_id.clone(),
        projection.fulfillment_status,
        projection.cancellation_event_id.clone(),
        cancellation_root_event_id,
        cancellation_prev_event_id,
        cancellation_reason,
        false,
        None,
        projection.receipt_event_id.clone(),
        receipt_root_event_id,
        receipt_prev_event_id,
        projection.receipt_received.map(|received| {
            (
                received,
                projection.receipt_issue.clone(),
                projection.receipt_received_at,
            )
        }),
        reducer_issues.as_slice(),
    );
    let revision = order_status_revision_view(
        projection.last_event_id.as_deref(),
        projection.agreement_event_id.as_deref(),
        &revision_proposal_records,
        &revision_decision_records,
    );
    let payment = include_deferred_payment
        .then(|| order_status_payment_view(projection.payment, reducer_issues.as_slice()));

    let view = OrderStatusView {
        state,
        source: ORDER_STATUS_SOURCE.to_owned(),
        order_id: projection.order_id,
        request_event_id: projection.request_event_id,
        decision_event_id: projection.decision_event_id,
        agreement_event_id: projection.agreement_event_id,
        listing_event_id,
        listing_addr: projection.listing_addr,
        buyer_pubkey: projection.buyer_pubkey,
        seller_pubkey: projection.seller_pubkey,
        economics: projection.economics,
        last_event_id: projection.last_event_id,
        revision,
        inventory,
        fulfillment,
        lifecycle: Some(lifecycle),
        payment,
        reducer_issues,
        target_relays,
        connected_relays,
        failed_relays: relay_failures(failed_relays),
        fetched_count,
        decoded_count,
        skipped_count,
        reason,
        actions: Vec::new(),
    };
    OrderStatusReduction {
        view,
        fulfillment_event_id,
        fulfillment_status,
    }
}

fn order_status_request_matches_context(
    record: &RadrootsActiveOrderRequestRecord,
    context: OrderStatusContext<'_>,
) -> bool {
    if record.payload.order_id != context.order_id {
        return false;
    }
    context.selected_account_pubkey.is_none_or(|pubkey| {
        record.payload.buyer_pubkey == pubkey || record.payload.seller_pubkey == pubkey
    })
}

fn enrich_order_status_inventory(
    config: &RuntimeConfig,
    view: &mut OrderStatusView,
) -> Result<(), RuntimeError> {
    let Some(listing_addr) = view.listing_addr.clone() else {
        return Ok(());
    };
    let Some(listing_event_id) = view.listing_event_id.clone() else {
        return Ok(());
    };
    let Some(seller_pubkey) = view.seller_pubkey.clone() else {
        return Ok(());
    };
    let Some(decision_event_id) = view.decision_event_id.clone() else {
        return Ok(());
    };

    let Some(listing) = fetch_current_inventory_listing_for_status(config, listing_addr.as_str())?
    else {
        return Ok(());
    };
    if listing.event_id != listing_event_id {
        return Ok(());
    }

    let mut requests = fetch_listing_accounting_requests_for_status(
        config,
        seller_pubkey.as_str(),
        listing_addr.as_str(),
        listing.event_id.as_str(),
    )?;
    let mut request_order_ids = requests
        .iter()
        .map(|record| record.payload.order_id.clone())
        .collect::<Vec<_>>();
    request_order_ids.sort();
    request_order_ids.dedup();
    requests.sort_by(|left, right| left.event_id.cmp(&right.event_id));

    let decisions = fetch_listing_accounting_decisions_for_status(config, listing_addr.as_str())?
        .into_iter()
        .filter(|record| request_order_ids.contains(&record.payload.order_id))
        .collect::<Vec<_>>();
    let revision_proposals =
        fetch_listing_accounting_revision_proposals_for_status(config, listing_addr.as_str())?
            .into_iter()
            .filter(|record| request_order_ids.contains(&record.payload.order_id))
            .collect::<Vec<_>>();
    let revision_decisions =
        fetch_listing_accounting_revision_decisions_for_status(config, listing_addr.as_str())?
            .into_iter()
            .filter(|record| request_order_ids.contains(&record.payload.order_id))
            .collect::<Vec<_>>();
    let fulfillments =
        fetch_listing_accounting_fulfillments_for_status(config, listing_addr.as_str())?
            .into_iter()
            .filter(|record| request_order_ids.contains(&record.payload.order_id))
            .collect::<Vec<_>>();
    let cancellations =
        fetch_listing_accounting_cancellations_for_status(config, listing_addr.as_str())?
            .into_iter()
            .filter(|record| request_order_ids.contains(&record.payload.order_id))
            .collect::<Vec<_>>();
    let projection = reduce_listing_inventory_accounting(
        listing_addr.as_str(),
        listing.event_id.as_str(),
        listing.bins,
        requests,
        decisions,
        revision_proposals,
        revision_decisions,
        fulfillments,
        cancellations,
        Vec::<RadrootsActiveOrderReceiptRecord>::new(),
    );
    let mut relevant_event_ids = Vec::new();
    relevant_event_ids.push(decision_event_id);
    relevant_event_ids.extend(view.agreement_event_id.clone());
    relevant_event_ids.extend(view.last_event_id.clone());
    relevant_event_ids.sort();
    relevant_event_ids.dedup();
    let relevant_issues = projection
        .issues
        .iter()
        .filter(|issue| {
            listing_inventory_issue_involves_order(
                issue,
                view.order_id.as_str(),
                relevant_event_ids.as_slice(),
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if relevant_issues.is_empty() {
        if matches!(
            view.state.as_str(),
            "accepted" | "cancelled" | "completed" | "disputed"
        ) {
            let inventory_state = if view
                .fulfillment
                .as_ref()
                .is_some_and(|fulfillment| fulfillment.inventory_released)
                || view.state == "cancelled"
            {
                "released"
            } else {
                "reserved"
            };
            view.inventory = Some(order_inventory_view_from_listing_projection(
                &projection,
                inventory_state,
                true,
            ));
        }
        return Ok(());
    }

    let mut inventory = order_inventory_view_from_listing_projection(&projection, "invalid", false);
    inventory.issues = relevant_issues
        .iter()
        .cloned()
        .map(listing_inventory_accounting_issue_view)
        .collect();
    view.reducer_issues.extend(inventory.issues.clone());
    view.inventory = Some(inventory);
    view.state = "invalid".to_owned();
    view.reason = Some(format!(
        "listing inventory accounting for order `{}` failed reducer validation",
        view.order_id
    ));
    Ok(())
}

fn fetch_current_inventory_listing_for_status(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Option<ResolvedInventoryListing>, RuntimeError> {
    let parsed = parse_listing_addr(listing_addr).map_err(|error| {
        RuntimeError::Config(format!("order status listing_addr is invalid: {error}"))
    })?;
    let filter = listing_event_filter(&parsed)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    current_inventory_listing_from_parts(parsed, receipt)
}

fn fetch_listing_accounting_requests_for_status(
    config: &RuntimeConfig,
    seller_pubkey: &str,
    listing_addr: &str,
    listing_event_id: &str,
) -> Result<Vec<RadrootsActiveOrderRequestRecord>, RuntimeError> {
    let filter = order_listing_request_filter(seller_pubkey, listing_addr)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_ORDER_REQUEST
            || !event_matches_tag_value(&event, "a", listing_addr)
        {
            continue;
        }
        if let Ok(record) = listing_accounting_request_from_event(&event)
            && record.listing_event_id.as_deref() == Some(listing_event_id)
        {
            records.push(record.record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_decisions_for_status(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Vec<RadrootsActiveOrderDecisionRecord>, RuntimeError> {
    let filter = order_listing_decision_filter(listing_addr)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_ORDER_DECISION
            || !event_matches_tag_value(&event, "a", listing_addr)
        {
            continue;
        }
        if let Ok(OrderStatusRecord::Decision(record)) = order_status_record_from_event(&event) {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_revision_proposals_for_status(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Vec<RadrootsActiveOrderRevisionProposalRecord>, RuntimeError> {
    let filter = order_listing_revision_proposal_filter(listing_addr)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_ORDER_REVISION
            || !event_matches_tag_value(&event, "a", listing_addr)
        {
            continue;
        }
        if let Ok(OrderStatusRecord::RevisionProposal(record)) =
            order_status_record_from_event(&event)
        {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_revision_decisions_for_status(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Vec<RadrootsActiveOrderRevisionDecisionRecord>, RuntimeError> {
    let filter = order_listing_revision_decision_filter(listing_addr)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_ORDER_REVISION_RESPONSE
            || !event_matches_tag_value(&event, "a", listing_addr)
        {
            continue;
        }
        if let Ok(OrderStatusRecord::RevisionDecision(record)) =
            order_status_record_from_event(&event)
        {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_fulfillments_for_status(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Vec<RadrootsActiveOrderFulfillmentRecord>, RuntimeError> {
    let filter = order_listing_fulfillment_filter(listing_addr)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_FULFILLMENT_UPDATE
            || !event_matches_tag_value(&event, "a", listing_addr)
        {
            continue;
        }
        if let Ok(OrderStatusRecord::Fulfillment(record)) = order_status_record_from_event(&event) {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_cancellations_for_status(
    config: &RuntimeConfig,
    listing_addr: &str,
) -> Result<Vec<RadrootsActiveOrderCancellationRecord>, RuntimeError> {
    let filter = order_listing_cancellation_filter(listing_addr)?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_CANCEL
            || !event_matches_tag_value(&event, "a", listing_addr)
        {
            continue;
        }
        if let Ok(OrderStatusRecord::Cancellation(record)) = order_status_record_from_event(&event)
        {
            records.push(record);
        }
    }
    Ok(records)
}

fn listing_inventory_issue_involves_order(
    issue: &RadrootsListingInventoryAccountingIssue,
    order_id: &str,
    event_ids: &[String],
) -> bool {
    match issue {
        RadrootsListingInventoryAccountingIssue::InvalidActiveOrder {
            order_id: issue_order_id,
            event_ids: issue_event_ids,
        } => issue_order_id == order_id || issue_event_ids.iter().any(|id| event_ids.contains(id)),
        RadrootsListingInventoryAccountingIssue::ArithmeticOverflow {
            event_ids: issue_event_ids,
            ..
        }
        | RadrootsListingInventoryAccountingIssue::UnknownInventoryBin {
            event_ids: issue_event_ids,
            ..
        }
        | RadrootsListingInventoryAccountingIssue::OverReserved {
            event_ids: issue_event_ids,
            ..
        } => issue_event_ids.iter().any(|id| event_ids.contains(id)),
    }
}

fn order_status_request_candidate(
    event: &RadrootsNostrEvent,
    context: OrderStatusContext<'_>,
) -> bool {
    if event_kind_u32(event) != KIND_TRADE_ORDER_REQUEST
        || !event_matches_tag_value(event, "d", context.order_id)
    {
        return false;
    }
    context.selected_account_pubkey.is_none_or(|pubkey| {
        event.pubkey.to_string() == pubkey || event_matches_tag_value(event, "p", pubkey)
    })
}

fn order_request_candidate_matches(
    event: &RadrootsNostrEvent,
    context: OrderRequestCandidateContext<'_>,
) -> bool {
    if event_kind_u32(event) != KIND_TRADE_ORDER_REQUEST
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
        KIND_TRADE_ORDER_REQUEST => {
            let event = radroots_event_from_nostr(event);
            let envelope = active_trade_envelope_from_event::<RadrootsTradeOrderRequested>(&event)
                .map_err(|error| {
                    RuntimeError::Config(format!("decode active order request event: {error}"))
                })?;
            if envelope.message_type != RadrootsActiveTradeMessageType::TradeOrderRequested {
                return Err(RuntimeError::Config(
                    "active order request event used the wrong message type".to_owned(),
                ));
            }
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeOrderRequested,
                &event.tags,
            )
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
            if listing_addr.seller_pubkey != envelope.payload.seller_pubkey {
                return Err(RuntimeError::Config(
                    "active order request listing_addr is outside seller authority".to_owned(),
                ));
            }
            Ok(OrderStatusRecord::Request {
                listing_event_id: context.listing_event.as_ref().map(|event| event.id.clone()),
                record: RadrootsActiveOrderRequestRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    payload: envelope.payload,
                },
            })
        }
        KIND_TRADE_ORDER_DECISION => {
            let event = radroots_event_from_nostr(event);
            let envelope =
                active_trade_envelope_from_event::<RadrootsTradeOrderDecisionEvent>(&event)
                    .map_err(|error| {
                        RuntimeError::Config(format!("decode active order decision event: {error}"))
                    })?;
            if envelope.message_type != RadrootsActiveTradeMessageType::TradeOrderDecision {
                return Err(RuntimeError::Config(
                    "active order decision event used the wrong message type".to_owned(),
                ));
            }
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeOrderDecision,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!("decode active order decision tags: {error}"))
            })?;
            Ok(OrderStatusRecord::Decision(
                RadrootsActiveOrderDecisionRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_ORDER_REVISION => {
            let event = radroots_event_from_nostr(event);
            let envelope =
                active_trade_order_revision_proposal_from_event(&event).map_err(|error| {
                    RuntimeError::Config(format!(
                        "decode active order revision proposal event: {error}"
                    ))
                })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeOrderRevisionProposed,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!(
                    "decode active order revision proposal tags: {error}"
                ))
            })?;
            Ok(OrderStatusRecord::RevisionProposal(
                RadrootsActiveOrderRevisionProposalRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_ORDER_REVISION_RESPONSE => {
            let event = radroots_event_from_nostr(event);
            let envelope =
                active_trade_order_revision_decision_from_event(&event).map_err(|error| {
                    RuntimeError::Config(format!(
                        "decode active order revision decision event: {error}"
                    ))
                })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeOrderRevisionDecision,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!(
                    "decode active order revision decision tags: {error}"
                ))
            })?;
            Ok(OrderStatusRecord::RevisionDecision(
                RadrootsActiveOrderRevisionDecisionRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_FULFILLMENT_UPDATE => {
            let event = radroots_event_from_nostr(event);
            let envelope = active_trade_fulfillment_update_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!("decode active fulfillment update event: {error}"))
            })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeFulfillmentUpdated,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!("decode active fulfillment update tags: {error}"))
            })?;
            Ok(OrderStatusRecord::Fulfillment(
                RadrootsActiveOrderFulfillmentRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_CANCEL => {
            let event = radroots_event_from_nostr(event);
            let envelope = active_trade_order_cancel_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!("decode active order cancellation event: {error}"))
            })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeOrderCancelled,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!("decode active order cancellation tags: {error}"))
            })?;
            Ok(OrderStatusRecord::Cancellation(
                RadrootsActiveOrderCancellationRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_RECEIPT => {
            let event = radroots_event_from_nostr(event);
            let envelope = active_trade_buyer_receipt_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!("decode active buyer receipt event: {error}"))
            })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeBuyerReceipt,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!("decode active buyer receipt tags: {error}"))
            })?;
            Ok(OrderStatusRecord::Receipt(
                RadrootsActiveOrderReceiptRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_PAYMENT_RECORDED => {
            let event = radroots_event_from_nostr(event);
            let envelope = active_trade_payment_recorded_from_event(&event).map_err(|error| {
                RuntimeError::Config(format!("decode active payment recorded event: {error}"))
            })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradePaymentRecorded,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!("decode active payment recorded tags: {error}"))
            })?;
            Ok(OrderStatusRecord::Payment(
                RadrootsActiveOrderPaymentRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
                    payload: envelope.payload,
                },
            ))
        }
        KIND_TRADE_SETTLEMENT_DECISION => {
            let event = radroots_event_from_nostr(event);
            let envelope =
                active_trade_settlement_decision_from_event(&event).map_err(|error| {
                    RuntimeError::Config(format!(
                        "decode active settlement decision event: {error}"
                    ))
                })?;
            let context = active_trade_event_context_from_tags(
                RadrootsActiveTradeMessageType::TradeSettlementDecision,
                &event.tags,
            )
            .map_err(|error| {
                RuntimeError::Config(format!("decode active settlement decision tags: {error}"))
            })?;
            Ok(OrderStatusRecord::Settlement(
                RadrootsActiveOrderSettlementRecord {
                    event_id: event.id,
                    author_pubkey: event.author,
                    counterparty_pubkey: context.counterparty_pubkey,
                    root_event_id: context.root_event_id.unwrap_or_default(),
                    prev_event_id: context.prev_event_id.unwrap_or_default(),
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
        if event_kind_u32(event) != KIND_TRADE_ORDER_REVISION
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

fn active_order_status_state(status: &RadrootsActiveOrderStatus) -> &'static str {
    match status {
        RadrootsActiveOrderStatus::Missing => "missing",
        RadrootsActiveOrderStatus::Requested => "requested",
        RadrootsActiveOrderStatus::Accepted => "accepted",
        RadrootsActiveOrderStatus::Declined => "declined",
        RadrootsActiveOrderStatus::Cancelled => "cancelled",
        RadrootsActiveOrderStatus::Completed => "completed",
        RadrootsActiveOrderStatus::Disputed => "disputed",
        RadrootsActiveOrderStatus::Invalid => "invalid",
    }
}

fn active_order_payment_state(status: &RadrootsActiveOrderPaymentState) -> &'static str {
    match status {
        RadrootsActiveOrderPaymentState::NotRecorded => "not_recorded",
        RadrootsActiveOrderPaymentState::Recorded => "recorded",
        RadrootsActiveOrderPaymentState::Settled => "settled",
        RadrootsActiveOrderPaymentState::Rejected => "rejected",
        RadrootsActiveOrderPaymentState::Invalid => "invalid",
    }
}

fn active_order_settlement_state(status: &RadrootsActiveOrderSettlementState) -> &'static str {
    match status {
        RadrootsActiveOrderSettlementState::NotRequired => "not_required",
        RadrootsActiveOrderSettlementState::Pending => "pending",
        RadrootsActiveOrderSettlementState::Accepted => "accepted",
        RadrootsActiveOrderSettlementState::Rejected => "rejected",
        RadrootsActiveOrderSettlementState::Invalid => "invalid",
    }
}

fn parse_payment_method(value: &str) -> Result<RadrootsTradePaymentMethod, RuntimeError> {
    match value.trim() {
        "cash" => Ok(RadrootsTradePaymentMethod::Cash),
        "manual_transfer" => Ok(RadrootsTradePaymentMethod::ManualTransfer),
        "other" => Ok(RadrootsTradePaymentMethod::Other),
        other => Err(RuntimeError::Config(format!(
            "unsupported payment method `{other}`"
        ))),
    }
}

fn parse_payment_amount(value: &str) -> Result<RadrootsCoreDecimal, RuntimeError> {
    let parsed = value
        .trim()
        .parse::<RadrootsCoreDecimal>()
        .map_err(|error| RuntimeError::Config(format!("payment amount is invalid: {error}")))?;
    if parsed.is_zero() || parsed.is_sign_negative() {
        return Err(RuntimeError::Config(
            "payment amount must be greater than zero".to_owned(),
        ));
    }
    Ok(parsed)
}

fn parse_payment_currency(value: &str) -> Result<RadrootsCoreCurrency, RuntimeError> {
    value
        .parse::<RadrootsCoreCurrency>()
        .map_err(|error| RuntimeError::Config(format!("payment currency is invalid: {error}")))
}

fn active_order_status_reason(
    status: &RadrootsActiveOrderStatus,
    order_id: &str,
) -> Option<String> {
    match status {
        RadrootsActiveOrderStatus::Missing => {
            Some(format!("no active order events matched `{order_id}`"))
        }
        RadrootsActiveOrderStatus::Invalid => Some(format!(
            "active order events for `{order_id}` failed reducer validation"
        )),
        _ => None,
    }
}

fn order_status_inventory_view(
    status: &RadrootsActiveOrderStatus,
    listing_event_id: Option<String>,
    decision_event_id: Option<&str>,
    decisions: &[RadrootsActiveOrderDecisionRecord],
    reducer_issues: &[OrderIssueView],
) -> Option<OrderInventoryView> {
    let inventory_issues = reducer_issues
        .iter()
        .filter(|issue| {
            matches!(
                issue.code.as_str(),
                "missing_decision_inventory_commitments"
                    | "decision_inventory_commitment_mismatch"
                    | "decision_counterparty_mismatch"
                    | "listing_inventory_arithmetic_overflow"
                    | "unknown_inventory_bin"
                    | "listing_inventory_over_reserved"
                    | "invalid_inventory_order"
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    match status {
        RadrootsActiveOrderStatus::Accepted
        | RadrootsActiveOrderStatus::Completed
        | RadrootsActiveOrderStatus::Disputed => {
            let bins = decision_event_id
                .and_then(|event_id| {
                    decisions
                        .iter()
                        .find(|decision| decision.event_id == event_id)
                })
                .map(|decision| inventory_bins_from_decision(&decision.payload.decision))
                .unwrap_or_default();
            Some(OrderInventoryView {
                state: if inventory_issues.is_empty() {
                    "reserved".to_owned()
                } else {
                    "invalid".to_owned()
                },
                listing_event_id,
                commitment_valid: inventory_issues.is_empty(),
                bins,
                issues: inventory_issues,
            })
        }
        RadrootsActiveOrderStatus::Cancelled => Some(OrderInventoryView {
            state: if decision_event_id.is_some() {
                "released".to_owned()
            } else {
                "not_reserved".to_owned()
            },
            listing_event_id,
            commitment_valid: inventory_issues.is_empty(),
            bins: Vec::new(),
            issues: inventory_issues,
        }),
        RadrootsActiveOrderStatus::Declined => Some(OrderInventoryView {
            state: "not_reserved".to_owned(),
            listing_event_id,
            commitment_valid: true,
            bins: Vec::new(),
            issues: inventory_issues,
        }),
        RadrootsActiveOrderStatus::Invalid if !inventory_issues.is_empty() => {
            Some(OrderInventoryView {
                state: "invalid".to_owned(),
                listing_event_id,
                commitment_valid: false,
                bins: Vec::new(),
                issues: inventory_issues,
            })
        }
        _ => None,
    }
}

fn order_status_fulfillment_view(
    status: &RadrootsActiveOrderStatus,
    request_event_id: Option<String>,
    decision_event_id: Option<String>,
    fulfillment_event_id: Option<String>,
    fulfillment_root_event_id: Option<String>,
    fulfillment_prev_event_id: Option<String>,
    fulfillment_status: Option<RadrootsActiveTradeFulfillmentState>,
    reducer_issues: &[OrderIssueView],
) -> Option<OrderStatusFulfillmentView> {
    let issues = reducer_issues
        .iter()
        .filter(|issue| fulfillment_issue_code(issue.code.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !issues.is_empty() {
        return Some(OrderStatusFulfillmentView {
            state: "invalid".to_owned(),
            event_id: fulfillment_event_id,
            root_event_id: fulfillment_root_event_id.or(request_event_id),
            prev_event_id: fulfillment_prev_event_id,
            terminal: false,
            inventory_released: false,
            issues,
        });
    }
    if !matches!(
        status,
        RadrootsActiveOrderStatus::Accepted
            | RadrootsActiveOrderStatus::Completed
            | RadrootsActiveOrderStatus::Disputed
    ) {
        return None;
    }
    let fulfillment_status = fulfillment_status?;
    let terminal = matches!(
        fulfillment_status,
        RadrootsActiveTradeFulfillmentState::Delivered
            | RadrootsActiveTradeFulfillmentState::SellerCancelled
    );
    let inventory_released = matches!(
        fulfillment_status,
        RadrootsActiveTradeFulfillmentState::SellerCancelled
    );
    let prev_event_id = fulfillment_prev_event_id.or_else(|| {
        if fulfillment_event_id.is_none() {
            decision_event_id
        } else {
            None
        }
    });
    Some(OrderStatusFulfillmentView {
        state: fulfillment_state_name(fulfillment_status).to_owned(),
        event_id: fulfillment_event_id,
        root_event_id: fulfillment_root_event_id.or(request_event_id),
        prev_event_id,
        terminal,
        inventory_released,
        issues,
    })
}

fn order_status_payment_view(
    projection: RadrootsActiveOrderPaymentProjection,
    reducer_issues: &[OrderIssueView],
) -> OrderStatusPaymentView {
    OrderStatusPaymentView {
        state: active_order_payment_state(&projection.state).to_owned(),
        settlement_state: active_order_settlement_state(&projection.settlement_state).to_owned(),
        payment_event_id: projection.payment_event_id,
        settlement_event_id: projection.settlement_event_id,
        agreement_event_id: projection.agreement_event_id,
        quote_id: projection.quote_id,
        quote_version: projection.quote_version,
        economics_digest: projection.economics_digest,
        amount: projection.amount,
        currency: projection.currency,
        method: projection.method,
        reference: projection.reference,
        paid_at: projection.paid_at,
        reason: projection.reason,
        issues: reducer_issues.to_vec(),
    }
}

fn order_status_lifecycle_view(
    status: &RadrootsActiveOrderStatus,
    request_event_id: Option<String>,
    last_event_id: Option<String>,
    fulfillment_status: Option<RadrootsActiveTradeFulfillmentState>,
    cancellation_event_id: Option<String>,
    cancellation_root_event_id: Option<String>,
    cancellation_prev_event_id: Option<String>,
    cancellation_reason: Option<String>,
    settlement_required: bool,
    settlement_reason: Option<String>,
    receipt_event_id: Option<String>,
    receipt_root_event_id: Option<String>,
    receipt_prev_event_id: Option<String>,
    receipt: Option<(bool, Option<String>, Option<u64>)>,
    reducer_issues: &[OrderIssueView],
) -> OrderStatusLifecycleView {
    let phase = order_status_lifecycle_phase(status, fulfillment_status).to_owned();
    let terminal = matches!(
        status,
        RadrootsActiveOrderStatus::Cancelled
            | RadrootsActiveOrderStatus::Completed
            | RadrootsActiveOrderStatus::Disputed
            | RadrootsActiveOrderStatus::Invalid
    );
    let cancellation =
        cancellation_event_id
            .as_ref()
            .map(|event_id| OrderStatusLifecycleCancellationView {
                event_id: event_id.clone(),
                root_event_id: cancellation_root_event_id
                    .clone()
                    .or(request_event_id.clone()),
                prev_event_id: cancellation_prev_event_id.clone(),
                reason: cancellation_reason.clone(),
            });
    let receipt_view = receipt_event_id.as_ref().map(|event_id| {
        let (received, issue, received_at) = receipt.clone().unwrap_or((false, None, None));
        OrderStatusLifecycleReceiptView {
            event_id: event_id.clone(),
            root_event_id: receipt_root_event_id.clone().or(request_event_id.clone()),
            prev_event_id: receipt_prev_event_id.clone(),
            received,
            issue,
            received_at,
        }
    });
    let event_id = receipt_event_id.or(cancellation_event_id);
    let prev_event_id = receipt_prev_event_id
        .or(cancellation_prev_event_id)
        .or(last_event_id);
    OrderStatusLifecycleView {
        phase,
        terminal,
        event_id,
        root_event_id: request_event_id,
        prev_event_id,
        cancellation,
        receipt: receipt_view,
        settlement_required,
        settlement_reason,
        issues: reducer_issues.to_vec(),
    }
}

fn order_status_revision_view(
    last_event_id: Option<&str>,
    agreement_event_id: Option<&str>,
    proposals: &[RadrootsActiveOrderRevisionProposalRecord],
    decisions: &[RadrootsActiveOrderRevisionDecisionRecord],
) -> Option<OrderStatusRevisionView> {
    if let Some(proposal) = last_event_id
        .and_then(|event_id| proposals.iter().find(|record| record.event_id == event_id))
    {
        return Some(OrderStatusRevisionView {
            state: "pending".to_owned(),
            revision_id: Some(proposal.payload.revision_id.clone()),
            proposal_event_id: Some(proposal.event_id.clone()),
            decision_event_id: None,
            root_event_id: Some(proposal.root_event_id.clone()),
            prev_event_id: Some(proposal.prev_event_id.clone()),
            agreement_event_id: None,
            reason: Some(proposal.payload.reason.clone()),
        });
    }

    if let Some(decision) = last_event_id
        .and_then(|event_id| decisions.iter().find(|record| record.event_id == event_id))
    {
        return Some(order_status_revision_view_from_decision(
            decision,
            agreement_event_id,
        ));
    }

    agreement_event_id
        .and_then(|event_id| decisions.iter().find(|record| record.event_id == event_id))
        .map(|decision| order_status_revision_view_from_decision(decision, agreement_event_id))
}

fn order_status_revision_view_from_decision(
    decision: &RadrootsActiveOrderRevisionDecisionRecord,
    agreement_event_id: Option<&str>,
) -> OrderStatusRevisionView {
    let (state, reason) = match &decision.payload.decision {
        RadrootsTradeOrderRevisionDecision::Accepted => ("accepted", None),
        RadrootsTradeOrderRevisionDecision::Declined { reason } => {
            ("declined", Some(reason.clone()))
        }
    };
    OrderStatusRevisionView {
        state: state.to_owned(),
        revision_id: Some(decision.payload.revision_id.clone()),
        proposal_event_id: Some(decision.prev_event_id.clone()),
        decision_event_id: Some(decision.event_id.clone()),
        root_event_id: Some(decision.root_event_id.clone()),
        prev_event_id: Some(decision.prev_event_id.clone()),
        agreement_event_id: agreement_event_id.map(str::to_owned),
        reason,
    }
}

fn order_status_lifecycle_phase(
    status: &RadrootsActiveOrderStatus,
    fulfillment_status: Option<RadrootsActiveTradeFulfillmentState>,
) -> &'static str {
    match status {
        RadrootsActiveOrderStatus::Missing => "missing",
        RadrootsActiveOrderStatus::Requested => "requested",
        RadrootsActiveOrderStatus::Accepted => match fulfillment_status {
            Some(RadrootsActiveTradeFulfillmentState::Preparing)
            | Some(RadrootsActiveTradeFulfillmentState::OutForDelivery) => {
                "fulfillment_in_progress"
            }
            Some(
                RadrootsActiveTradeFulfillmentState::ReadyForPickup
                | RadrootsActiveTradeFulfillmentState::Delivered
                | RadrootsActiveTradeFulfillmentState::SellerCancelled,
            ) => "fulfilled",
            Some(RadrootsActiveTradeFulfillmentState::AcceptedNotFulfilled) | None => "accepted",
        },
        RadrootsActiveOrderStatus::Declined => "declined",
        RadrootsActiveOrderStatus::Cancelled => "cancelled",
        RadrootsActiveOrderStatus::Completed => "completed",
        RadrootsActiveOrderStatus::Disputed => "disputed",
        RadrootsActiveOrderStatus::Invalid => "invalid",
    }
}

fn fulfillment_issue_code(code: &str) -> bool {
    matches!(
        code,
        "fulfillment_without_accepted_decision"
            | "invalid_fulfillment_payload"
            | "fulfillment_order_id_mismatch"
            | "fulfillment_author_mismatch"
            | "fulfillment_counterparty_mismatch"
            | "fulfillment_buyer_mismatch"
            | "fulfillment_seller_mismatch"
            | "invalid_fulfillment_listing_address"
            | "fulfillment_listing_mismatch"
            | "fulfillment_root_mismatch"
            | "fulfillment_previous_mismatch"
            | "fulfillment_status_not_publishable"
            | "fulfillment_unsupported_transition"
            | "forked_fulfillments"
    )
}

fn inventory_bins_from_decision(
    decision: &RadrootsTradeOrderDecision,
) -> Vec<OrderInventoryBinView> {
    match decision {
        RadrootsTradeOrderDecision::Accepted {
            inventory_commitments,
        } => {
            let mut bins = inventory_commitments
                .iter()
                .map(|commitment| OrderInventoryBinView {
                    bin_id: commitment.bin_id.clone(),
                    committed_count: u64::from(commitment.bin_count),
                    available_count: None,
                    remaining_count: None,
                    over_reserved: false,
                })
                .collect::<Vec<_>>();
            bins.sort_by(|left, right| left.bin_id.cmp(&right.bin_id));
            bins
        }
        RadrootsTradeOrderDecision::Declined { .. } => Vec::new(),
    }
}

fn active_order_reducer_issue_view(issue_value: RadrootsActiveOrderReducerIssue) -> OrderIssueView {
    match issue_value {
        RadrootsActiveOrderReducerIssue::MissingRequest => issue_with_code(
            "missing_request",
            "request_event_id",
            "active order reducer reported missing request",
        ),
        RadrootsActiveOrderReducerIssue::MultipleRequests { event_ids } => issue_with_events(
            "multiple_requests",
            "request_event_id",
            "active order reducer reported multiple request events",
            event_ids,
        ),
        RadrootsActiveOrderReducerIssue::RequestPayloadInvalid { event_id } => issue_with_events(
            "invalid_request_payload",
            "request_payload",
            "active order reducer reported invalid request payload",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::RequestOrderIdMismatch { event_id } => issue_with_events(
            "request_order_id_mismatch",
            "order_id",
            "active order reducer reported request order id mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::RequestAuthorMismatch { event_id } => issue_with_events(
            "request_author_mismatch",
            "buyer_pubkey",
            "active order reducer reported request author mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::RequestListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_request_listing_address",
                "listing_addr",
                "active order reducer reported invalid request listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RequestSellerListingMismatch { event_id } => {
            issue_with_events(
                "request_seller_listing_mismatch",
                "seller_pubkey",
                "active order reducer reported request seller/listing mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DecisionPayloadInvalid { event_id } => issue_with_events(
            "invalid_decision_payload",
            "decision_payload",
            "active order reducer reported invalid decision payload",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionOrderIdMismatch { event_id } => issue_with_events(
            "decision_order_id_mismatch",
            "order_id",
            "active order reducer reported decision order id mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionAuthorMismatch { event_id } => issue_with_events(
            "decision_author_mismatch",
            "seller_pubkey",
            "active order reducer reported decision author mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionCounterpartyMismatch { event_id } => {
            issue_with_events(
                "decision_counterparty_mismatch",
                "buyer_pubkey",
                "active order reducer reported decision counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DecisionBuyerMismatch { event_id } => issue_with_events(
            "decision_buyer_mismatch",
            "buyer_pubkey",
            "active order reducer reported decision buyer mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionSellerMismatch { event_id } => issue_with_events(
            "decision_seller_mismatch",
            "seller_pubkey",
            "active order reducer reported decision seller mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_decision_listing_address",
                "listing_addr",
                "active order reducer reported invalid decision listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DecisionListingMismatch { event_id } => issue_with_events(
            "decision_listing_mismatch",
            "listing_addr",
            "active order reducer reported decision listing mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionRootMismatch { event_id } => issue_with_events(
            "decision_root_mismatch",
            "root_event_id",
            "active order reducer reported decision root mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DecisionPreviousMismatch { event_id } => {
            issue_with_events(
                "decision_previous_mismatch",
                "prev_event_id",
                "active order reducer reported decision previous mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DecisionMissingInventoryCommitments { event_id } => {
            issue_with_events(
                "missing_decision_inventory_commitments",
                "inventory_commitments",
                "active order reducer reported missing decision inventory commitments",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DecisionInventoryCommitmentMismatch { event_id } => {
            issue_with_events(
                "decision_inventory_commitment_mismatch",
                "inventory_commitments",
                "active order reducer reported decision inventory commitment mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DecisionMissingReason { event_id } => issue_with_events(
            "missing_decision_decline_reason",
            "reason",
            "active order reducer reported missing decision decline reason",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ConflictingDecisions { event_ids } => issue_with_events(
            "conflicting_decisions",
            "decision_event_id",
            "active order reducer reported conflicting decisions",
            event_ids,
        ),
        RadrootsActiveOrderReducerIssue::RevisionProposalWithoutAcceptedDecision { event_id } => {
            issue_with_events(
                "revision_proposal_without_accepted_decision",
                "revision_event_id",
                "active order reducer reported revision proposal without accepted decision",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalPayloadInvalid { event_id } => {
            issue_with_events(
                "invalid_revision_proposal_payload",
                "revision_payload",
                "active order reducer reported invalid revision proposal payload",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalOrderIdMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_order_id_mismatch",
                "order_id",
                "active order reducer reported revision proposal order id mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalAuthorMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_author_mismatch",
                "seller_pubkey",
                "active order reducer reported revision proposal author mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalCounterpartyMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_counterparty_mismatch",
                "buyer_pubkey",
                "active order reducer reported revision proposal counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalBuyerMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_buyer_mismatch",
                "buyer_pubkey",
                "active order reducer reported revision proposal buyer mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalSellerMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_seller_mismatch",
                "seller_pubkey",
                "active order reducer reported revision proposal seller mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_revision_proposal_listing_address",
                "listing_addr",
                "active order reducer reported invalid revision proposal listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalListingMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_listing_mismatch",
                "listing_addr",
                "active order reducer reported revision proposal listing mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalRootMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_root_mismatch",
                "root_event_id",
                "active order reducer reported revision proposal root mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionProposalPreviousMismatch { event_id } => {
            issue_with_events(
                "revision_proposal_previous_mismatch",
                "prev_event_id",
                "active order reducer reported revision proposal previous mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionWithoutProposal { event_id } => {
            issue_with_events(
                "revision_decision_without_proposal",
                "revision_decision_event_id",
                "active order reducer reported revision decision without proposal",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionPayloadInvalid { event_id } => {
            issue_with_events(
                "invalid_revision_decision_payload",
                "revision_decision_payload",
                "active order reducer reported invalid revision decision payload",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionOrderIdMismatch { event_id } => {
            issue_with_events(
                "revision_decision_order_id_mismatch",
                "order_id",
                "active order reducer reported revision decision order id mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionAuthorMismatch { event_id } => {
            issue_with_events(
                "revision_decision_author_mismatch",
                "buyer_pubkey",
                "active order reducer reported revision decision author mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionCounterpartyMismatch { event_id } => {
            issue_with_events(
                "revision_decision_counterparty_mismatch",
                "seller_pubkey",
                "active order reducer reported revision decision counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionBuyerMismatch { event_id } => {
            issue_with_events(
                "revision_decision_buyer_mismatch",
                "buyer_pubkey",
                "active order reducer reported revision decision buyer mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionSellerMismatch { event_id } => {
            issue_with_events(
                "revision_decision_seller_mismatch",
                "seller_pubkey",
                "active order reducer reported revision decision seller mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_revision_decision_listing_address",
                "listing_addr",
                "active order reducer reported invalid revision decision listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionListingMismatch { event_id } => {
            issue_with_events(
                "revision_decision_listing_mismatch",
                "listing_addr",
                "active order reducer reported revision decision listing mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionRootMismatch { event_id } => {
            issue_with_events(
                "revision_decision_root_mismatch",
                "root_event_id",
                "active order reducer reported revision decision root mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionPreviousMismatch { event_id } => {
            issue_with_events(
                "revision_decision_previous_mismatch",
                "prev_event_id",
                "active order reducer reported revision decision previous mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionDecisionRevisionIdMismatch { event_id } => {
            issue_with_events(
                "revision_decision_revision_id_mismatch",
                "revision_id",
                "active order reducer reported revision decision revision id mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentWithoutAcceptedDecision { event_id } => {
            issue_with_events(
                "fulfillment_without_accepted_decision",
                "fulfillment_event_id",
                "active order reducer reported fulfillment without accepted decision",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentPayloadInvalid { event_id } => {
            issue_with_events(
                "invalid_fulfillment_payload",
                "fulfillment_payload",
                "active order reducer reported invalid fulfillment payload",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentOrderIdMismatch { event_id } => {
            issue_with_events(
                "fulfillment_order_id_mismatch",
                "order_id",
                "active order reducer reported fulfillment order id mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentAuthorMismatch { event_id } => {
            issue_with_events(
                "fulfillment_author_mismatch",
                "seller_pubkey",
                "active order reducer reported fulfillment author mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentCounterpartyMismatch { event_id } => {
            issue_with_events(
                "fulfillment_counterparty_mismatch",
                "buyer_pubkey",
                "active order reducer reported fulfillment counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentBuyerMismatch { event_id } => {
            issue_with_events(
                "fulfillment_buyer_mismatch",
                "buyer_pubkey",
                "active order reducer reported fulfillment buyer mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentSellerMismatch { event_id } => {
            issue_with_events(
                "fulfillment_seller_mismatch",
                "seller_pubkey",
                "active order reducer reported fulfillment seller mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_fulfillment_listing_address",
                "listing_addr",
                "active order reducer reported invalid fulfillment listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentListingMismatch { event_id } => {
            issue_with_events(
                "fulfillment_listing_mismatch",
                "listing_addr",
                "active order reducer reported fulfillment listing mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentRootMismatch { event_id } => issue_with_events(
            "fulfillment_root_mismatch",
            "root_event_id",
            "active order reducer reported fulfillment root mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::FulfillmentPreviousMismatch { event_id } => {
            issue_with_events(
                "fulfillment_previous_mismatch",
                "prev_event_id",
                "active order reducer reported fulfillment previous mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentStatusNotPublishable { event_id } => {
            issue_with_events(
                "fulfillment_status_not_publishable",
                "fulfillment_state",
                "active order reducer reported non-publishable fulfillment status",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::FulfillmentUnsupportedTransition { event_id } => {
            issue_with_events(
                "fulfillment_unsupported_transition",
                "fulfillment_state",
                "active order reducer reported unsupported fulfillment transition",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::ForkedFulfillments { event_ids } => issue_with_events(
            "forked_fulfillments",
            "fulfillment_event_id",
            "active order reducer reported forked fulfillment updates",
            event_ids,
        ),
        RadrootsActiveOrderReducerIssue::CancellationWithoutCancellableOrder { event_id } => {
            issue_with_events(
                "cancellation_without_cancellable_order",
                "cancellation_event_id",
                "active order reducer reported cancellation without cancellable order",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationPayloadInvalid { event_id } => {
            issue_with_events(
                "invalid_cancellation_payload",
                "cancellation_payload",
                "active order reducer reported invalid cancellation payload",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationOrderIdMismatch { event_id } => {
            issue_with_events(
                "cancellation_order_id_mismatch",
                "order_id",
                "active order reducer reported cancellation order id mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationAuthorMismatch { event_id } => {
            issue_with_events(
                "cancellation_author_mismatch",
                "buyer_pubkey",
                "active order reducer reported cancellation author mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationCounterpartyMismatch { event_id } => {
            issue_with_events(
                "cancellation_counterparty_mismatch",
                "seller_pubkey",
                "active order reducer reported cancellation counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationBuyerMismatch { event_id } => {
            issue_with_events(
                "cancellation_buyer_mismatch",
                "buyer_pubkey",
                "active order reducer reported cancellation buyer mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationSellerMismatch { event_id } => {
            issue_with_events(
                "cancellation_seller_mismatch",
                "seller_pubkey",
                "active order reducer reported cancellation seller mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_cancellation_listing_address",
                "listing_addr",
                "active order reducer reported invalid cancellation listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationListingMismatch { event_id } => {
            issue_with_events(
                "cancellation_listing_mismatch",
                "listing_addr",
                "active order reducer reported cancellation listing mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationRootMismatch { event_id } => {
            issue_with_events(
                "cancellation_root_mismatch",
                "root_event_id",
                "active order reducer reported cancellation root mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationPreviousMismatch { event_id } => {
            issue_with_events(
                "cancellation_previous_mismatch",
                "prev_event_id",
                "active order reducer reported cancellation previous mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::CancellationAfterFulfillment { event_id } => {
            issue_with_events(
                "cancellation_after_fulfillment",
                "fulfillment_event_id",
                "active order reducer reported cancellation after fulfillment",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::ReceiptWithoutEligibleFulfillment { event_id } => {
            issue_with_events(
                "receipt_without_eligible_fulfillment",
                "receipt_event_id",
                "active order reducer reported receipt without eligible fulfillment",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::ReceiptPayloadInvalid { event_id } => issue_with_events(
            "invalid_receipt_payload",
            "receipt_payload",
            "active order reducer reported invalid receipt payload",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptOrderIdMismatch { event_id } => issue_with_events(
            "receipt_order_id_mismatch",
            "order_id",
            "active order reducer reported receipt order id mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptAuthorMismatch { event_id } => issue_with_events(
            "receipt_author_mismatch",
            "buyer_pubkey",
            "active order reducer reported receipt author mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptCounterpartyMismatch { event_id } => {
            issue_with_events(
                "receipt_counterparty_mismatch",
                "seller_pubkey",
                "active order reducer reported receipt counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::ReceiptBuyerMismatch { event_id } => issue_with_events(
            "receipt_buyer_mismatch",
            "buyer_pubkey",
            "active order reducer reported receipt buyer mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptSellerMismatch { event_id } => issue_with_events(
            "receipt_seller_mismatch",
            "seller_pubkey",
            "active order reducer reported receipt seller mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_receipt_listing_address",
                "listing_addr",
                "active order reducer reported invalid receipt listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::ReceiptListingMismatch { event_id } => issue_with_events(
            "receipt_listing_mismatch",
            "listing_addr",
            "active order reducer reported receipt listing mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptRootMismatch { event_id } => issue_with_events(
            "receipt_root_mismatch",
            "root_event_id",
            "active order reducer reported receipt root mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::ReceiptPreviousMismatch { event_id } => issue_with_events(
            "receipt_previous_mismatch",
            "prev_event_id",
            "active order reducer reported receipt previous mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentWithoutAcceptedAgreement { event_id } => {
            issue_with_events(
                "payment_without_accepted_agreement",
                "payment_event_id",
                "active order reducer reported payment without accepted agreement",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::PaymentPayloadInvalid { event_id } => issue_with_events(
            "invalid_payment_payload",
            "payment_payload",
            "active order reducer reported invalid payment payload",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentOrderIdMismatch { event_id } => issue_with_events(
            "payment_order_id_mismatch",
            "order_id",
            "active order reducer reported payment order id mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentAuthorMismatch { event_id } => issue_with_events(
            "payment_author_mismatch",
            "buyer_pubkey",
            "active order reducer reported payment author mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentCounterpartyMismatch { event_id } => {
            issue_with_events(
                "payment_counterparty_mismatch",
                "seller_pubkey",
                "active order reducer reported payment counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::PaymentBuyerMismatch { event_id } => issue_with_events(
            "payment_buyer_mismatch",
            "buyer_pubkey",
            "active order reducer reported payment buyer mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentSellerMismatch { event_id } => issue_with_events(
            "payment_seller_mismatch",
            "seller_pubkey",
            "active order reducer reported payment seller mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_payment_listing_address",
                "listing_addr",
                "active order reducer reported invalid payment listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::PaymentListingMismatch { event_id } => issue_with_events(
            "payment_listing_mismatch",
            "listing_addr",
            "active order reducer reported payment listing mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentRootMismatch { event_id } => issue_with_events(
            "payment_root_mismatch",
            "root_event_id",
            "active order reducer reported payment root mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentPreviousMismatch { event_id } => issue_with_events(
            "payment_previous_mismatch",
            "prev_event_id",
            "active order reducer reported payment previous mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentAgreementMismatch { event_id } => {
            issue_with_events(
                "payment_agreement_mismatch",
                "agreement_event_id",
                "active order reducer reported payment agreement mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::PaymentQuoteMismatch { event_id } => issue_with_events(
            "payment_quote_mismatch",
            "quote_id",
            "active order reducer reported payment quote mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentQuoteVersionMismatch { event_id } => {
            issue_with_events(
                "payment_quote_version_mismatch",
                "quote_version",
                "active order reducer reported payment quote version mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::PaymentEconomicsDigestMismatch { event_id } => {
            issue_with_events(
                "payment_economics_digest_mismatch",
                "economics_digest",
                "active order reducer reported payment economics digest mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::PaymentAmountMismatch { event_id } => issue_with_events(
            "payment_amount_mismatch",
            "amount",
            "active order reducer reported payment amount mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentCurrencyMismatch { event_id } => issue_with_events(
            "payment_currency_mismatch",
            "currency",
            "active order reducer reported payment currency mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::PaymentAfterCancellation { event_id } => {
            issue_with_events(
                "payment_after_cancellation",
                "payment_event_id",
                "active order reducer reported payment after cancellation",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::RevisionAfterPayment { event_id } => issue_with_events(
            "revision_after_payment",
            "revision_event_id",
            "active order reducer reported revision after payment",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::DuplicatePayments { event_ids } => issue_with_events(
            "duplicate_payments",
            "payment_event_id",
            "active order reducer reported duplicate payment events",
            event_ids,
        ),
        RadrootsActiveOrderReducerIssue::SettlementWithoutValidPayment { event_id } => {
            issue_with_events(
                "settlement_without_valid_payment",
                "settlement_event_id",
                "active order reducer reported settlement without valid payment",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementPayloadInvalid { event_id } => {
            issue_with_events(
                "invalid_settlement_payload",
                "settlement_payload",
                "active order reducer reported invalid settlement payload",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementOrderIdMismatch { event_id } => {
            issue_with_events(
                "settlement_order_id_mismatch",
                "order_id",
                "active order reducer reported settlement order id mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementAuthorMismatch { event_id } => {
            issue_with_events(
                "settlement_author_mismatch",
                "seller_pubkey",
                "active order reducer reported settlement author mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementCounterpartyMismatch { event_id } => {
            issue_with_events(
                "settlement_counterparty_mismatch",
                "buyer_pubkey",
                "active order reducer reported settlement counterparty mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementBuyerMismatch { event_id } => issue_with_events(
            "settlement_buyer_mismatch",
            "buyer_pubkey",
            "active order reducer reported settlement buyer mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::SettlementSellerMismatch { event_id } => {
            issue_with_events(
                "settlement_seller_mismatch",
                "seller_pubkey",
                "active order reducer reported settlement seller mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementListingAddressInvalid { event_id } => {
            issue_with_events(
                "invalid_settlement_listing_address",
                "listing_addr",
                "active order reducer reported invalid settlement listing address",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementListingMismatch { event_id } => {
            issue_with_events(
                "settlement_listing_mismatch",
                "listing_addr",
                "active order reducer reported settlement listing mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementRootMismatch { event_id } => issue_with_events(
            "settlement_root_mismatch",
            "root_event_id",
            "active order reducer reported settlement root mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::SettlementPreviousMismatch { event_id } => {
            issue_with_events(
                "settlement_previous_mismatch",
                "prev_event_id",
                "active order reducer reported settlement previous mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementPaymentEventMismatch { event_id } => {
            issue_with_events(
                "settlement_payment_event_mismatch",
                "payment_event_id",
                "active order reducer reported settlement payment event mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementAgreementMismatch { event_id } => {
            issue_with_events(
                "settlement_agreement_mismatch",
                "agreement_event_id",
                "active order reducer reported settlement agreement mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementQuoteMismatch { event_id } => issue_with_events(
            "settlement_quote_mismatch",
            "quote_id",
            "active order reducer reported settlement quote mismatch",
            vec![event_id],
        ),
        RadrootsActiveOrderReducerIssue::SettlementQuoteVersionMismatch { event_id } => {
            issue_with_events(
                "settlement_quote_version_mismatch",
                "quote_version",
                "active order reducer reported settlement quote version mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementEconomicsDigestMismatch { event_id } => {
            issue_with_events(
                "settlement_economics_digest_mismatch",
                "economics_digest",
                "active order reducer reported settlement economics digest mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementAmountMismatch { event_id } => {
            issue_with_events(
                "settlement_amount_mismatch",
                "amount",
                "active order reducer reported settlement amount mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::SettlementCurrencyMismatch { event_id } => {
            issue_with_events(
                "settlement_currency_mismatch",
                "currency",
                "active order reducer reported settlement currency mismatch",
                vec![event_id],
            )
        }
        RadrootsActiveOrderReducerIssue::DuplicateSettlements { event_ids } => issue_with_events(
            "duplicate_settlements",
            "settlement_event_id",
            "active order reducer reported duplicate settlement events",
            event_ids,
        ),
        RadrootsActiveOrderReducerIssue::ForkedLifecycle { event_ids } => issue_with_events(
            "forked_lifecycle",
            "event_id",
            "active order reducer reported forked lifecycle events",
            event_ids,
        ),
    }
}

fn order_history_unconfigured(
    seller_pubkey: Option<String>,
    reason: String,
    target_relays: Vec<String>,
) -> OrderHistoryView {
    OrderHistoryView {
        state: "unconfigured".to_owned(),
        source: ORDER_EVENT_LIST_SOURCE.to_owned(),
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
        actions: vec!["radroots account create".to_owned()],
    }
}

fn order_history_unavailable(
    seller_pubkey: String,
    reason: String,
    target_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> OrderHistoryView {
    OrderHistoryView {
        state: "unavailable".to_owned(),
        source: ORDER_EVENT_LIST_SOURCE.to_owned(),
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

fn order_history_from_receipt(
    seller_pubkey: String,
    order_id: Option<&str>,
    receipt: DirectRelayFetchReceipt,
) -> OrderHistoryView {
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
        match order_history_entry_from_event(&event, seller_pubkey.as_str()) {
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

    OrderHistoryView {
        state: if orders.is_empty() { "empty" } else { "ready" }.to_owned(),
        source: ORDER_EVENT_LIST_SOURCE.to_owned(),
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
    args: &OrderDecisionArgs,
    state: &str,
    dry_run: bool,
) -> OrderDecisionView {
    OrderDecisionView {
        state: state.to_owned(),
        source: ORDER_DECISION_SOURCE.to_owned(),
        order_id: args.key.clone(),
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
    args: &OrderRevisionProposeArgs,
    state: &str,
    dry_run: bool,
) -> OrderRevisionProposalView {
    OrderRevisionProposalView {
        state: state.to_owned(),
        source: ORDER_REVISION_PROPOSAL_SOURCE.to_owned(),
        order_id: args.key.clone(),
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
    args: &OrderRevisionDecisionArgs,
    state: &str,
    dry_run: bool,
) -> OrderRevisionDecisionView {
    OrderRevisionDecisionView {
        state: state.to_owned(),
        source: ORDER_REVISION_DECISION_SOURCE.to_owned(),
        order_id: args.key.clone(),
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

fn order_fulfillment_base_view(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
    state: &str,
    dry_run: bool,
) -> OrderFulfillmentView {
    OrderFulfillmentView {
        state: state.to_owned(),
        source: ORDER_FULFILLMENT_SOURCE.to_owned(),
        order_id: args.key.clone(),
        fulfillment_state: args.state.trim().to_owned(),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        decision_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
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

fn order_cancellation_base_view(
    config: &RuntimeConfig,
    args: &OrderCancelArgs,
    state: &str,
    dry_run: bool,
) -> OrderCancellationView {
    OrderCancellationView {
        state: state.to_owned(),
        source: ORDER_CANCELLATION_SOURCE.to_owned(),
        order_id: args.key.clone(),
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

fn order_receipt_base_view(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
    state: &str,
    dry_run: bool,
) -> OrderReceiptView {
    OrderReceiptView {
        state: state.to_owned(),
        source: ORDER_RECEIPT_SOURCE.to_owned(),
        order_id: args.key.clone(),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        decision_event_id: None,
        fulfillment_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
        received: args.received,
        issue: args.issue.as_ref().map(|issue| issue.trim().to_owned()),
        received_at: None,
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

fn order_payment_base_view(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    state: &str,
    dry_run: bool,
) -> OrderPaymentView {
    OrderPaymentView {
        state: state.to_owned(),
        source: ORDER_PAYMENT_SOURCE.to_owned(),
        order_id: args.key.clone(),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        agreement_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        event_id: None,
        event_kind: None,
        quote_id: None,
        quote_version: None,
        economics_digest: None,
        amount: parse_payment_amount(args.amount.as_str()).ok(),
        currency: parse_payment_currency(args.currency.as_str()).ok(),
        method: parse_payment_method(args.method.as_str()).ok(),
        reference: args
            .reference
            .as_ref()
            .map(|reference| reference.trim().to_owned()),
        paid_at: args.paid_at,
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

fn order_settlement_base_view(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
    state: &str,
    dry_run: bool,
) -> OrderSettlementView {
    OrderSettlementView {
        state: state.to_owned(),
        source: ORDER_SETTLEMENT_SOURCE.to_owned(),
        order_id: args.key.clone(),
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        request_event_id: None,
        agreement_event_id: None,
        root_event_id: None,
        prev_event_id: None,
        payment_event_id: non_empty_ref(args.payment_event_id.as_str()).map(str::to_owned),
        event_id: None,
        event_kind: None,
        quote_id: None,
        quote_version: None,
        economics_digest: None,
        amount: None,
        currency: None,
        decision: Some(settlement_decision_protocol(args.decision)),
        settlement_reason: args.reason.as_ref().map(|reason| reason.trim().to_owned()),
        reason: None,
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
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

const fn settlement_decision_protocol(
    decision: OrderSettlementDecisionArg,
) -> RadrootsTradeSettlementDecision {
    match decision {
        OrderSettlementDecisionArg::Accept => RadrootsTradeSettlementDecision::Accepted,
        OrderSettlementDecisionArg::Reject => RadrootsTradeSettlementDecision::Rejected,
    }
}

const fn settlement_decision_state(decision: OrderSettlementDecisionArg) -> &'static str {
    match decision {
        OrderSettlementDecisionArg::Accept => "accepted",
        OrderSettlementDecisionArg::Reject => "rejected",
    }
}

fn apply_order_fulfillment_status(view: &mut OrderFulfillmentView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.decision_event_id = status.decision_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id = status.last_event_id.clone();
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
}

fn apply_order_cancellation_status(view: &mut OrderCancellationView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
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

fn apply_order_receipt_status(view: &mut OrderReceiptView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.decision_event_id = status.decision_event_id.clone();
    view.fulfillment_event_id = status
        .fulfillment
        .as_ref()
        .and_then(|fulfillment| fulfillment.event_id.clone());
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id =
        order_receipt_prev_event_id(status).or_else(|| status.last_event_id.clone());
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
}

fn apply_order_payment_status(view: &mut OrderPaymentView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.agreement_event_id = status.agreement_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.prev_event_id = order_payment_prev_event_id(status);
    if let Some(economics) = status.economics.as_ref() {
        view.quote_id = Some(economics.quote_id.clone());
        view.quote_version = Some(economics.quote_version);
        view.economics_digest = radroots_trade_order_economics_digest(economics).ok();
        view.amount = Some(economics.total.amount);
        view.currency = Some(economics.total.currency);
    }
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
}

fn apply_order_settlement_status(view: &mut OrderSettlementView, status: &OrderStatusView) {
    view.order_id = status.order_id.clone();
    view.listing_addr = status.listing_addr.clone();
    view.buyer_pubkey = status.buyer_pubkey.clone();
    view.seller_pubkey = status.seller_pubkey.clone();
    view.request_event_id = status.request_event_id.clone();
    view.root_event_id = status.request_event_id.clone();
    view.target_relays = status.target_relays.clone();
    view.connected_relays = status.connected_relays.clone();
    view.failed_relays = status.failed_relays.clone();
    view.fetched_count = status.fetched_count;
    view.decoded_count = status.decoded_count;
    view.skipped_count = status.skipped_count;
    view.issues = status.reducer_issues.clone();
    if let Some(payment) = status.payment.as_ref() {
        view.payment_event_id = payment
            .payment_event_id
            .clone()
            .or_else(|| view.payment_event_id.clone());
        view.event_id = payment.settlement_event_id.clone();
        view.event_kind = payment
            .settlement_event_id
            .as_ref()
            .map(|_| KIND_TRADE_SETTLEMENT_DECISION);
        view.agreement_event_id = payment.agreement_event_id.clone();
        view.prev_event_id = payment.payment_event_id.clone();
        view.quote_id = payment.quote_id.clone();
        view.quote_version = payment.quote_version;
        view.economics_digest = payment.economics_digest.clone();
        view.amount = payment.amount;
        view.currency = payment.currency;
        view.settlement_reason = payment.reason.clone().or(view.settlement_reason.clone());
    }
}

fn order_receipt_prev_event_id(status: &OrderStatusView) -> Option<String> {
    status.fulfillment.as_ref().and_then(|fulfillment| {
        if matches!(fulfillment.state.as_str(), "ready_for_pickup" | "delivered") {
            fulfillment.event_id.clone()
        } else {
            None
        }
    })
}

fn order_payment_prev_event_id(status: &OrderStatusView) -> Option<String> {
    status.payment.as_ref().and_then(|payment| {
        if payment.state == "rejected" {
            payment
                .settlement_event_id
                .clone()
                .or_else(|| status.agreement_event_id.clone())
        } else {
            status.agreement_event_id.clone()
        }
    })
}

fn unrejected_payment_state(status: &OrderStatusView) -> Option<&str> {
    status
        .payment
        .as_ref()
        .map(|payment| payment.state.as_str())
        .filter(|state| matches!(*state, "recorded" | "settled"))
}

fn order_cancellation_prev_event_id(status: &OrderStatusView) -> Option<String> {
    match status.state.as_str() {
        "requested" => status.request_event_id.clone(),
        "accepted" => status
            .last_event_id
            .clone()
            .or(status.decision_event_id.clone()),
        _ => status.last_event_id.clone(),
    }
}

fn order_cancellation_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderCancelArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
) -> Option<OrderCancellationView> {
    let buyer_matches = status
        .buyer_pubkey
        .as_deref()
        .is_some_and(|buyer| buyer.eq_ignore_ascii_case(selected_pubkey));
    let payment_state = unrejected_payment_state(status);
    let state = match status.state.as_str() {
        "requested" if buyer_matches => return None,
        "accepted" if buyer_matches && payment_state.is_some() => "invalid",
        "accepted"
            if buyer_matches
                && status
                    .fulfillment
                    .as_ref()
                    .and_then(|fulfillment| fulfillment.event_id.as_ref())
                    .is_none() =>
        {
            return None;
        }
        "accepted" if buyer_matches => "fulfilled",
        "cancelled" | "completed" | "disputed" => "terminal",
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
        view.event_kind = Some(KIND_TRADE_CANCEL);
    }
    view.reason = Some(match state {
        "missing" => format!("no active order events matched `{}`", args.key),
        "declined" => format!(
            "order cancel refused because order `{}` was declined",
            args.key
        ),
        "terminal" => {
            format!(
                "order cancel refused because order `{}` is already terminal",
                args.key
            )
        }
        "fulfilled" => format!(
            "order cancel refused because order `{}` already has seller fulfillment",
            args.key
        ),
        "invalid" if buyer_matches && payment_state.is_some() => {
            if let Some(payment_state) = payment_state {
                format!(
                    "order cancel refused because order `{}` already has unrejected payment state `{payment_state}`",
                    args.key
                )
            } else {
                status.reason.clone().unwrap_or_else(|| {
                    format!(
                        "order cancel refused because active order events for `{}` are invalid",
                        args.key
                    )
                })
            }
        }
        "invalid" if !buyer_matches && status.buyer_pubkey.is_some() => format!(
            "order cancel refused because selected account is not buyer for order `{}`",
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order cancel refused because active order events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order cancel status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    if state == "invalid" && buyer_matches && payment_state.is_some() {
        view.issues.push(issue_with_code(
            "payment_blocks_cancellation",
            "payment.state",
            "orders with unrejected recorded payment cannot be cancelled",
        ));
    }
    view.actions = vec![format!("radroots order status get {}", args.key)];
    Some(view)
}

fn order_receipt_args_preflight_view(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
) -> Option<OrderReceiptView> {
    let issue = args
        .issue
        .as_deref()
        .map(str::trim)
        .filter(|issue| !issue.is_empty());
    let reason = if args.received && issue.is_some() {
        Some("order receipt record cannot set both received and issue".to_owned())
    } else if !args.received && issue.is_none() {
        Some("order receipt record requires --received or a non-empty --issue".to_owned())
    } else {
        None
    }?;
    let mut view = order_receipt_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(reason);
    view.issues = vec![issue_with_code(
        "invalid_receipt_outcome",
        "receipt",
        "receipt outcome must be either received or issue",
    )];
    Some(view)
}

fn order_receipt_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
) -> Option<OrderReceiptView> {
    let buyer_matches = status
        .buyer_pubkey
        .as_deref()
        .is_some_and(|buyer| buyer.eq_ignore_ascii_case(selected_pubkey));
    let eligible_fulfillment = order_receipt_prev_event_id(status).is_some();
    let state = match status.state.as_str() {
        "accepted" if buyer_matches && eligible_fulfillment => return None,
        "accepted" if buyer_matches => "invalid",
        "completed" | "disputed" => "terminal",
        "missing" | "requested" | "declined" | "cancelled" | "invalid" | "unavailable"
        | "unconfigured" => status.state.as_str(),
        _ => "invalid",
    };
    let mut view = order_receipt_base_view(config, args, state, config.output.dry_run);
    apply_order_receipt_status(&mut view, status);
    if matches!(status.state.as_str(), "completed" | "disputed") {
        view.event_id = status
            .lifecycle
            .as_ref()
            .and_then(|lifecycle| lifecycle.event_id.clone());
        view.event_kind = Some(KIND_TRADE_RECEIPT);
        if let Some(receipt) = status
            .lifecycle
            .as_ref()
            .and_then(|lifecycle| lifecycle.receipt.as_ref())
        {
            view.received = receipt.received;
            view.issue = receipt.issue.clone();
            view.received_at = receipt.received_at;
        }
    }
    view.reason = Some(match state {
        "missing" => format!("no active order events matched `{}`", args.key),
        "requested" => format!(
            "order receipt record refused because order `{}` has no accepted seller decision",
            args.key
        ),
        "declined" => format!(
            "order receipt record refused because order `{}` was declined",
            args.key
        ),
        "cancelled" | "terminal" => {
            format!(
                "order receipt record refused because order `{}` is already terminal",
                args.key
            )
        }
        "invalid" if !buyer_matches && status.buyer_pubkey.is_some() => format!(
            "order receipt record refused because selected account is not buyer for order `{}`",
            args.key
        ),
        "invalid" if status.state == "accepted" => format!(
            "order receipt record refused because order `{}` has no eligible seller fulfillment",
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order receipt record refused because active order events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order receipt record status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    view.actions = vec![format!("radroots order status get {}", args.key)];
    Some(view)
}

fn order_payment_args_preflight_view(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
) -> Option<OrderPaymentView> {
    let (reason, issue) = if args.amount.trim().is_empty() {
        (
            "order payment record requires --amount".to_owned(),
            issue_with_code(
                "missing_payment_amount",
                "amount",
                "payment amount is required",
            ),
        )
    } else if parse_payment_amount(args.amount.as_str()).is_err() {
        (
            format!(
                "order payment record received invalid amount `{}`",
                args.amount
            ),
            issue_with_code(
                "invalid_payment_amount",
                "amount",
                "payment amount must be greater than zero",
            ),
        )
    } else if args.currency.trim().is_empty() {
        (
            "order payment record requires --currency".to_owned(),
            issue_with_code(
                "missing_payment_currency",
                "currency",
                "payment currency is required",
            ),
        )
    } else if parse_payment_currency(args.currency.as_str()).is_err() {
        (
            format!(
                "order payment record received invalid currency `{}`",
                args.currency
            ),
            issue_with_code(
                "invalid_payment_currency",
                "currency",
                "payment currency must be a 3-letter code",
            ),
        )
    } else if args.method.trim().is_empty() {
        (
            "order payment record requires --method".to_owned(),
            issue_with_code(
                "missing_payment_method",
                "method",
                "payment method is required",
            ),
        )
    } else if parse_payment_method(args.method.as_str()).is_err() {
        (
            format!(
                "order payment record received unsupported method `{}`",
                args.method
            ),
            issue_with_code(
                "invalid_payment_method",
                "method",
                "payment method must be cash, manual_transfer, or other",
            ),
        )
    } else {
        return None;
    };
    let mut view = order_payment_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(reason);
    view.issues = vec![issue];
    Some(view)
}

fn order_payment_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
) -> Option<OrderPaymentView> {
    let buyer_matches = status
        .buyer_pubkey
        .as_deref()
        .is_some_and(|buyer| buyer.eq_ignore_ascii_case(selected_pubkey));
    let payment_state = status
        .payment
        .as_ref()
        .map(|payment| payment.state.as_str())
        .unwrap_or("not_recorded");
    let payment_open = matches!(payment_state, "not_recorded" | "rejected");
    let different_existing_payment = matches!(payment_state, "recorded" | "settled")
        && buyer_matches
        && !status
            .payment
            .as_ref()
            .is_some_and(|payment| payment_args_match_existing_payment(args, payment));
    let state = match status.state.as_str() {
        "accepted" | "completed" | "disputed" if buyer_matches && payment_open => {
            if let Some(view) = order_payment_terms_preflight_view_from_status(config, args, status)
            {
                return Some(view);
            }
            return None;
        }
        "accepted" | "completed" | "disputed" if different_existing_payment => "invalid",
        "accepted" | "completed" | "disputed" if buyer_matches => payment_state,
        "missing" | "requested" | "declined" | "cancelled" | "invalid" | "unavailable"
        | "unconfigured" => status.state.as_str(),
        _ => "invalid",
    };
    let mut view = order_payment_base_view(config, args, state, config.output.dry_run);
    apply_order_payment_status(&mut view, status);
    if let Some(payment) = status.payment.as_ref() {
        view.event_id = payment.payment_event_id.clone();
        view.event_kind = payment
            .payment_event_id
            .as_ref()
            .map(|_| KIND_TRADE_PAYMENT_RECORDED);
        view.quote_id = payment.quote_id.clone().or(view.quote_id);
        view.quote_version = payment.quote_version.or(view.quote_version);
        view.economics_digest = payment.economics_digest.clone().or(view.economics_digest);
        view.amount = payment.amount.or(view.amount);
        view.currency = payment.currency.or(view.currency);
        view.method = payment.method.or(view.method);
        view.reference = payment.reference.clone().or(view.reference);
        view.paid_at = payment.paid_at.or(view.paid_at);
    }
    view.reason = Some(match state {
        "missing" => format!("no active order events matched `{}`", args.key),
        "requested" => format!(
            "order payment record refused because order `{}` has no accepted seller decision",
            args.key
        ),
        "declined" => format!(
            "order payment record refused because order `{}` was declined",
            args.key
        ),
        "cancelled" => format!(
            "order payment record refused because order `{}` was cancelled",
            args.key
        ),
        "recorded" | "settled" => format!(
            "order payment record skipped because order `{}` already has payment state `{state}`",
            args.key
        ),
        "invalid" if different_existing_payment => format!(
            "order payment record refused because order `{}` already has a different unrejected payment",
            args.key
        ),
        "invalid" if !buyer_matches && status.buyer_pubkey.is_some() => format!(
            "order payment record refused because selected account is not buyer for order `{}`",
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order payment record refused because active order events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order payment record status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    if different_existing_payment {
        view.issues = vec![issue_with_code(
            "duplicate_payment_attempt",
            "payment",
            "a different payment already exists for this unrejected payment state",
        )];
    }
    view.actions = vec![format!("radroots order status get {}", args.key)];
    Some(view)
}

fn payment_args_match_existing_payment(
    args: &OrderPaymentArgs,
    payment: &OrderStatusPaymentView,
) -> bool {
    let amount_matches = parse_payment_amount(args.amount.as_str())
        .ok()
        .is_some_and(|amount| Some(amount) == payment.amount);
    let currency_matches = parse_payment_currency(args.currency.as_str())
        .ok()
        .is_some_and(|currency| Some(currency) == payment.currency);
    let method_matches = parse_payment_method(args.method.as_str())
        .ok()
        .is_some_and(|method| Some(method) == payment.method);
    let reference = args
        .reference
        .as_deref()
        .map(str::trim)
        .filter(|reference| !reference.is_empty());
    amount_matches
        && currency_matches
        && method_matches
        && reference == payment.reference.as_deref()
        && args.paid_at == payment.paid_at
}

fn order_payment_terms_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    status: &OrderStatusView,
) -> Option<OrderPaymentView> {
    let requested_amount = parse_payment_amount(args.amount.as_str()).ok()?;
    let requested_currency = parse_payment_currency(args.currency.as_str()).ok()?;
    let Some(economics) = status.economics.as_ref() else {
        let mut view = order_payment_base_view(config, args, "invalid", config.output.dry_run);
        apply_order_payment_status(&mut view, status);
        view.reason = Some(format!(
            "order payment record refused because order `{}` has no accepted economics",
            args.key
        ));
        view.issues = vec![issue_with_code(
            "missing_payment_economics",
            "amount",
            "active order has no accepted economics for payment comparison",
        )];
        view.actions = vec![format!("radroots order status get {}", args.key)];
        return Some(view);
    };
    if requested_amount != economics.total.amount {
        let mut view = order_payment_base_view(config, args, "invalid", config.output.dry_run);
        apply_order_payment_status(&mut view, status);
        view.amount = Some(requested_amount);
        view.currency = Some(requested_currency);
        view.reason = Some(format!(
            "order payment record refused because amount `{}` does not match current agreement total `{}`",
            args.amount, economics.total.amount
        ));
        view.issues = vec![issue_with_code(
            "payment_amount_mismatch",
            "amount",
            "payment amount must match the current accepted agreement total",
        )];
        view.actions = vec![format!("radroots order status get {}", args.key)];
        return Some(view);
    }
    if requested_currency != economics.total.currency {
        let mut view = order_payment_base_view(config, args, "invalid", config.output.dry_run);
        apply_order_payment_status(&mut view, status);
        view.amount = Some(requested_amount);
        view.currency = Some(requested_currency);
        view.reason = Some(format!(
            "order payment record refused because currency `{}` does not match current agreement currency `{}`",
            args.currency, economics.total.currency
        ));
        view.issues = vec![issue_with_code(
            "payment_currency_mismatch",
            "currency",
            "payment currency must match the current accepted agreement currency",
        )];
        view.actions = vec![format!("radroots order status get {}", args.key)];
        return Some(view);
    }
    None
}

fn order_settlement_args_preflight_view(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
) -> Option<OrderSettlementView> {
    let (reason, issue) = if args.payment_event_id.trim().is_empty() {
        (
            "order settlement decision requires --payment-event-id".to_owned(),
            issue_with_code(
                "missing_payment_event_id",
                "payment_event_id",
                "payment event id is required",
            ),
        )
    } else if matches!(args.decision, OrderSettlementDecisionArg::Reject)
        && args.reason.as_deref().and_then(non_empty_ref).is_none()
    {
        (
            "order settlement reject requires --reason".to_owned(),
            issue_with_code(
                "missing_settlement_reason",
                "reason",
                "settlement rejection reason is required",
            ),
        )
    } else if matches!(args.decision, OrderSettlementDecisionArg::Accept)
        && args.reason.as_deref().and_then(non_empty_ref).is_some()
    {
        (
            "order settlement accept does not accept --reason".to_owned(),
            issue_with_code(
                "unexpected_settlement_reason",
                "reason",
                "settlement acceptance must not carry a reason",
            ),
        )
    } else {
        return None;
    };
    let mut view = order_settlement_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(reason);
    view.issues = vec![issue];
    Some(view)
}

fn order_settlement_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
) -> Option<OrderSettlementView> {
    let seller_matches = status
        .seller_pubkey
        .as_deref()
        .is_some_and(|seller| seller.eq_ignore_ascii_case(selected_pubkey));
    let payment = status.payment.as_ref();
    let current_payment_id = payment.and_then(|payment| payment.payment_event_id.as_deref());
    if matches!(status.state.as_str(), "accepted" | "completed" | "disputed")
        && seller_matches
        && payment.is_some_and(|payment| {
            payment.state == "recorded" && payment.settlement_state == "pending"
        })
        && current_payment_id == Some(args.payment_event_id.as_str())
    {
        return None;
    }

    let state = match status.state.as_str() {
        "missing" | "requested" | "declined" | "cancelled" | "invalid" | "unavailable"
        | "unconfigured" => status.state.as_str(),
        "accepted" | "completed" | "disputed" if !seller_matches => "invalid",
        "accepted" | "completed" | "disputed" => match payment {
            None => "not_recorded",
            Some(payment) => {
                if payment.payment_event_id.as_deref() != Some(args.payment_event_id.as_str()) {
                    "invalid"
                } else if matches!(payment.settlement_state.as_str(), "accepted" | "rejected") {
                    "already_decided"
                } else if payment.state != "recorded" {
                    payment.state.as_str()
                } else {
                    payment.settlement_state.as_str()
                }
            }
        },
        _ => "invalid",
    };
    let mut view = order_settlement_base_view(config, args, state, config.output.dry_run);
    apply_order_settlement_status(&mut view, status);
    view.reason = Some(match state {
        "missing" => format!("no active order events matched `{}`", args.key),
        "requested" => format!(
            "order settlement decision refused because order `{}` has no accepted seller decision",
            args.key
        ),
        "declined" => format!(
            "order settlement decision refused because order `{}` was declined",
            args.key
        ),
        "cancelled" => format!(
            "order settlement decision refused because order `{}` was cancelled",
            args.key
        ),
        "not_recorded" => format!(
            "order settlement decision refused because order `{}` has no recorded payment",
            args.key
        ),
        "already_decided" => format!(
            "order settlement decision skipped because payment `{}` already has settlement state `{}`",
            args.payment_event_id,
            payment
                .map(|payment| payment.settlement_state.as_str())
                .unwrap_or("unknown")
        ),
        "invalid" if !seller_matches && status.seller_pubkey.is_some() => format!(
            "order settlement decision refused because selected account is not seller for order `{}`",
            args.key
        ),
        "invalid" if current_payment_id.is_some() => format!(
            "order settlement decision refused because payment event `{}` is not the current recorded payment",
            args.payment_event_id
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order settlement decision refused because active order events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order settlement decision status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    if state == "invalid" && current_payment_id.is_some() && seller_matches {
        view.issues = vec![issue_with_code(
            "stale_payment_event",
            "payment_event_id",
            "settlement payment event id must match the current recorded payment",
        )];
    }
    view.actions = vec![format!("radroots order status get {}", args.key)];
    Some(view)
}

fn order_fulfillment_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
    status: &OrderStatusView,
    current_fulfillment_status: Option<RadrootsActiveTradeFulfillmentState>,
    current_fulfillment_event_id: Option<&str>,
) -> Option<OrderFulfillmentView> {
    let state = match status.state.as_str() {
        "accepted" => {
            if matches!(
                current_fulfillment_status,
                Some(
                    RadrootsActiveTradeFulfillmentState::Delivered
                        | RadrootsActiveTradeFulfillmentState::SellerCancelled
                )
            ) {
                "invalid"
            } else {
                return None;
            }
        }
        "missing" | "requested" | "declined" | "invalid" | "unavailable" | "unconfigured" => {
            status.state.as_str()
        }
        _ => return None,
    };
    let mut view = order_fulfillment_base_view(config, args, state, config.output.dry_run);
    apply_order_fulfillment_status(&mut view, status);
    view.reason = Some(match state {
        "missing" => format!("no active order events matched `{}`", args.key),
        "requested" => format!(
            "order fulfillment update refused because order `{}` has no accepted seller decision",
            args.key
        ),
        "declined" => format!(
            "order fulfillment update refused because order `{}` was declined",
            args.key
        ),
        "invalid"
            if matches!(
                current_fulfillment_status,
                Some(
                    RadrootsActiveTradeFulfillmentState::Delivered
                        | RadrootsActiveTradeFulfillmentState::SellerCancelled
                )
            ) =>
        {
            let current = current_fulfillment_status
                .map(fulfillment_state_name)
                .unwrap_or("unknown");
            view.issues.push(issue_with_events(
                "fulfillment_unsupported_transition",
                "fulfillment_state",
                format!(
                    "order `{}` already has terminal fulfillment state `{current}`",
                    args.key
                ),
                current_fulfillment_event_id
                    .map(str::to_owned)
                    .into_iter()
                    .collect(),
            ));
            format!(
                "order fulfillment update refused because order `{}` already has terminal fulfillment state `{current}`",
                args.key
            )
        }
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order fulfillment update refused because active order events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order fulfillment update status preflight failed with state `{}`",
                status.state
            )
        }),
    });
    view.actions = vec![format!("radroots order status get {}", args.key)];
    Some(view)
}

fn order_decision_view_from_resolution(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
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
        view.actions = vec![format!("radroots order status get {}", args.key)];
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
                .map(|request| request.request_event_id.clone())
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
            view.actions = vec![format!("radroots order status get {}", args.key)];
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
    view.order_id = request.order_id.clone();
    view.listing_addr = Some(request.listing_addr.clone());
    view.buyer_pubkey = Some(request.buyer_pubkey.clone());
    view.seller_pubkey = Some(request.seller_pubkey.clone());
    view.request_event_id = Some(request.request_event_id.clone());
    view.listing_event_id = request.listing_event_id.clone();
    view.root_event_id = Some(request.request_event_id.clone());
    view.prev_event_id = Some(request.request_event_id.clone());
}

fn apply_order_decision_status(view: &mut OrderDecisionView, status: &OrderStatusView) {
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
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
) -> Option<OrderDecisionView> {
    let state = match status.state.as_str() {
        "accepted" | "declined" => "already_decided",
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
        view.event_kind = Some(KIND_TRADE_ORDER_DECISION);
    }
    view.reason = Some(match status.state.as_str() {
        "accepted" | "declined" => format!(
            "order {} refused because order `{}` already has a visible `{}` seller decision",
            args.decision.command(),
            request.order_id,
            status.state
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order {} refused because active order events for `{}` are invalid",
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
    view.actions = vec![format!("radroots order status get {}", request.order_id)];
    Some(view)
}

fn order_revision_args_preflight_view(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
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
            "order revision propose requires a bin-count change or revision adjustment",
        ));
    }

    if issues.is_empty() {
        return None;
    }
    let mut view = order_revision_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(format!(
        "order revision propose inputs for `{}` failed validation",
        args.key
    ));
    view.issues = issues;
    Some(view)
}

fn order_revision_decision_args_preflight_view(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
) -> Option<OrderRevisionDecisionView> {
    let mut issues = Vec::new();
    if args.revision_id.trim().is_empty() {
        issues.push(issue_with_code(
            "revision_id_required",
            "revision_id",
            "order revision decision requires --revision-id",
        ));
    }
    if args.decision == OrderRevisionDecisionArg::Decline
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
            "order revision decline requires a non-empty reason",
        ));
    }

    if issues.is_empty() {
        return None;
    }
    let mut view =
        order_revision_decision_base_view(config, args, "invalid", config.output.dry_run);
    view.reason = Some(format!(
        "order revision {} inputs for `{}` failed validation",
        args.decision.command(),
        args.key
    ));
    view.issues = issues;
    Some(view)
}

fn order_revision_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
    status: &OrderStatusView,
    selected_pubkey: &str,
    candidates: &OrderRevisionProposalCandidates,
) -> Option<OrderRevisionProposalView> {
    let pending_revision = pending_revision_proposal_candidate(status, candidates);
    let seller_matches = status
        .seller_pubkey
        .as_deref()
        .is_some_and(|seller| seller.eq_ignore_ascii_case(selected_pubkey));
    let payment_state = unrejected_payment_state(status);
    let state = match status.state.as_str() {
        "accepted"
            if seller_matches
                && payment_state.is_none()
                && status
                    .fulfillment
                    .as_ref()
                    .and_then(|fulfillment| fulfillment.event_id.as_ref())
                    .is_none()
                && candidates.issues.is_empty()
                && pending_revision.is_none() =>
        {
            return None;
        }
        "accepted" if !seller_matches => "invalid",
        "accepted" if payment_state.is_some() => "invalid",
        "accepted"
            if status
                .fulfillment
                .as_ref()
                .and_then(|fulfillment| fulfillment.event_id.as_ref())
                .is_some() =>
        {
            "fulfilled"
        }
        "accepted" if !candidates.issues.is_empty() => "invalid",
        "accepted" if pending_revision.is_some() => "forked",
        "cancelled" | "completed" | "disputed" => "terminal",
        "missing" | "requested" | "declined" | "invalid" | "unavailable" | "unconfigured" => {
            status.state.as_str()
        }
        _ => "invalid",
    };
    let mut view = order_revision_base_view(config, args, state, config.output.dry_run);
    apply_order_revision_status(&mut view, status);
    if let Some(record) = pending_revision {
        view.event_id = Some(record.event_id.clone());
        view.event_kind = Some(KIND_TRADE_ORDER_REVISION);
        view.revision_id = Some(record.payload.revision_id.clone());
    }
    view.reason = Some(match state {
        "missing" => format!("no active order events matched `{}`", args.key),
        "requested" => format!(
            "order revision propose refused because order `{}` has no accepted seller decision",
            args.key
        ),
        "declined" => format!(
            "order revision propose refused because order `{}` was declined",
            args.key
        ),
        "terminal" => format!(
            "order revision propose refused because order `{}` is already terminal",
            args.key
        ),
        "fulfilled" => format!(
            "order revision propose refused because order `{}` already has seller fulfillment",
            args.key
        ),
        "forked" => format!(
            "order revision propose refused because order `{}` already has a pending revision proposal",
            args.key
        ),
        "invalid" if seller_matches && payment_state.is_some() => {
            if let Some(payment_state) = payment_state {
                format!(
                    "order revision propose refused because order `{}` already has unrejected payment state `{payment_state}`",
                    args.key
                )
            } else {
                status.reason.clone().unwrap_or_else(|| {
                    format!(
                        "order revision propose refused because active order events for `{}` are invalid",
                        args.key
                    )
                })
            }
        }
        "invalid" if !seller_matches && status.seller_pubkey.is_some() => format!(
            "order revision propose refused because selected account is not seller for order `{}`",
            args.key
        ),
        "invalid" if !candidates.issues.is_empty() => format!(
            "order revision propose refused because revision proposal candidates for `{}` are invalid",
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order revision propose refused because active order events for `{}` are invalid",
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order revision propose status preflight failed with state `{}`",
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
    if state == "invalid" && seller_matches && payment_state.is_some() {
        view.issues.push(issue_with_code(
            "payment_blocks_revision",
            "payment.state",
            "orders with unrejected recorded payment cannot be economically revised",
        ));
    }
    view.issues.extend(candidates.issues.clone());
    view.actions = vec![format!("radroots order status get {}", args.key)];
    Some(view)
}

fn order_revision_decision_preflight_view_from_status(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
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
        "accepted"
            if buyer_matches
                && status
                    .fulfillment
                    .as_ref()
                    .and_then(|fulfillment| fulfillment.event_id.as_ref())
                    .is_none()
                && candidates.issues.is_empty()
                && pending_revision.is_some() =>
        {
            return None;
        }
        "accepted" if !buyer_matches => "invalid",
        "accepted"
            if status
                .fulfillment
                .as_ref()
                .and_then(|fulfillment| fulfillment.event_id.as_ref())
                .is_some() =>
        {
            "fulfilled"
        }
        "accepted" if !candidates.issues.is_empty() => "invalid",
        "accepted" => "missing",
        "cancelled" | "completed" | "disputed" => "terminal",
        "declined" => "order_declined",
        "missing" | "requested" | "invalid" | "unavailable" | "unconfigured" => {
            status.state.as_str()
        }
        _ => "invalid",
    };
    let mut view = order_revision_decision_base_view(config, args, state, config.output.dry_run);
    apply_order_revision_decision_status(&mut view, status);
    if let Some(record) = pending_revision {
        apply_order_revision_decision_proposal(&mut view, record);
        view.event_id = Some(record.event_id.clone());
        view.event_kind = Some(KIND_TRADE_ORDER_REVISION);
    }
    view.reason = Some(match state {
        "missing" if status.state == "accepted" => format!(
            "order revision {} refused because order `{}` has no pending revision proposal",
            args.decision.command(),
            args.key
        ),
        "missing" => format!("no active order events matched `{}`", args.key),
        "requested" => format!(
            "order revision {} refused because order `{}` has no accepted seller decision",
            args.decision.command(),
            args.key
        ),
        "order_declined" => format!(
            "order revision {} refused because order `{}` was declined",
            args.decision.command(),
            args.key
        ),
        "terminal" => format!(
            "order revision {} refused because order `{}` is already terminal",
            args.decision.command(),
            args.key
        ),
        "fulfilled" => format!(
            "order revision {} refused because order `{}` already has seller fulfillment",
            args.decision.command(),
            args.key
        ),
        "invalid" if !buyer_matches && status.buyer_pubkey.is_some() => format!(
            "order revision {} refused because selected account is not buyer for order `{}`",
            args.decision.command(),
            args.key
        ),
        "invalid" if !candidates.issues.is_empty() => format!(
            "order revision {} refused because revision proposal candidates for `{}` are invalid",
            args.decision.command(),
            args.key
        ),
        "invalid" => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order revision {} refused because active order events for `{}` are invalid",
                args.decision.command(),
                args.key
            )
        }),
        _ => status.reason.clone().unwrap_or_else(|| {
            format!(
                "order revision {} status preflight failed with state `{}`",
                args.decision.command(),
                status.state
            )
        }),
    });
    view.issues.extend(candidates.issues.clone());
    view.actions = vec![format!("radroots order status get {}", args.key)];
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

fn order_accept_inventory_preflight_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
) -> Result<OrderDecisionInventoryPreflight, RuntimeError> {
    if args.decision != OrderDecisionArg::Accept {
        return Ok(OrderDecisionInventoryPreflight {
            invalid_view: None,
            inventory: Some(order_declined_inventory_view(request)),
        });
    }

    let listing = match fetch_current_inventory_listing(config, args, request, resolution, status)?
    {
        Ok(listing) => listing,
        Err(view) => {
            return Ok(OrderDecisionInventoryPreflight {
                invalid_view: Some(view),
                inventory: None,
            });
        }
    };
    if listing.event_id != request.listing_event_id.clone().unwrap_or_default() {
        return Ok(OrderDecisionInventoryPreflight {
            invalid_view: Some(order_decision_inventory_invalid_view(
                config,
                args,
                request,
                resolution,
                status,
                "order accept refused because the request listing event is not current",
                vec![issue_with_events(
                    "stale_request_listing_event",
                    "listing_event_id",
                    format!(
                        "request listing_event_id does not match current listing event `{}`",
                        listing.event_id
                    ),
                    request.listing_event_id.clone().into_iter().collect(),
                )],
            )),
            inventory: None,
        });
    }
    if !listing_is_active(&listing.listing) {
        return Ok(OrderDecisionInventoryPreflight {
            invalid_view: Some(order_decision_inventory_invalid_view(
                config,
                args,
                request,
                resolution,
                status,
                "order accept refused because the listing is not active",
                vec![issue_with_code(
                    "listing_not_active",
                    "listing_addr",
                    "current listing event is not active",
                )],
            )),
            inventory: None,
        });
    }

    let accounting_requests = fetch_listing_accounting_requests(config, request, &listing)?;
    let mut requests = accounting_requests
        .into_iter()
        .filter(|record| record.listing_event_id.as_deref() == Some(listing.event_id.as_str()))
        .map(|record| record.record)
        .collect::<Vec<_>>();
    requests.push(active_request_record_from_resolved(request));
    let mut request_order_ids = requests
        .iter()
        .map(|record| record.payload.order_id.clone())
        .collect::<Vec<_>>();
    request_order_ids.sort();
    request_order_ids.dedup();

    let mut decisions = fetch_listing_accounting_decisions(config, request)?
        .into_iter()
        .filter(|record| request_order_ids.contains(&record.payload.order_id))
        .collect::<Vec<_>>();
    decisions.push(proposed_accept_decision_record(request)?);
    let revision_proposals = fetch_listing_accounting_revision_proposals_for_status(
        config,
        request.listing_addr.as_str(),
    )?
    .into_iter()
    .filter(|record| request_order_ids.contains(&record.payload.order_id))
    .collect::<Vec<_>>();
    let revision_decisions = fetch_listing_accounting_revision_decisions_for_status(
        config,
        request.listing_addr.as_str(),
    )?
    .into_iter()
    .filter(|record| request_order_ids.contains(&record.payload.order_id))
    .collect::<Vec<_>>();
    let fulfillments = fetch_listing_accounting_fulfillments(config, request)?
        .into_iter()
        .filter(|record| request_order_ids.contains(&record.payload.order_id))
        .collect::<Vec<_>>();
    let cancellations = fetch_listing_accounting_cancellations(config, request)?
        .into_iter()
        .filter(|record| request_order_ids.contains(&record.payload.order_id))
        .collect::<Vec<_>>();

    let projection = reduce_listing_inventory_accounting(
        request.listing_addr.as_str(),
        listing.event_id.as_str(),
        listing.bins,
        requests,
        decisions,
        revision_proposals,
        revision_decisions,
        fulfillments,
        cancellations,
        Vec::<RadrootsActiveOrderReceiptRecord>::new(),
    );
    Ok(order_accept_inventory_preflight_view_from_projection(
        config, args, request, resolution, status, projection,
    ))
}

fn order_accept_inventory_preflight_view_from_projection(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
    projection: RadrootsListingInventoryAccountingProjection,
) -> OrderDecisionInventoryPreflight {
    if projection.issues.is_empty() {
        return OrderDecisionInventoryPreflight {
            invalid_view: None,
            inventory: Some(order_inventory_view_from_listing_projection(
                &projection,
                "reserved",
                true,
            )),
        };
    }

    let inventory = order_inventory_view_from_listing_projection(&projection, "invalid", false);
    let issues = projection
        .issues
        .into_iter()
        .map(listing_inventory_accounting_issue_view)
        .collect::<Vec<_>>();
    let mut view = order_decision_inventory_invalid_view(
        config,
        args,
        request,
        resolution,
        status,
        "order accept refused because visible inventory accounting is invalid",
        issues,
    );
    view.inventory = Some(inventory);
    OrderDecisionInventoryPreflight {
        invalid_view: Some(view),
        inventory: None,
    }
}

fn order_inventory_view_from_listing_projection(
    projection: &RadrootsListingInventoryAccountingProjection,
    state: &str,
    commitment_valid: bool,
) -> OrderInventoryView {
    OrderInventoryView {
        state: state.to_owned(),
        listing_event_id: Some(projection.listing_event_id.clone()),
        commitment_valid,
        bins: projection
            .bins
            .iter()
            .map(|bin| OrderInventoryBinView {
                bin_id: bin.bin_id.clone(),
                committed_count: bin.accepted_reserved_count,
                available_count: Some(bin.available_count),
                remaining_count: Some(bin.remaining_count),
                over_reserved: bin.over_reserved,
            })
            .collect(),
        issues: projection
            .issues
            .iter()
            .cloned()
            .map(listing_inventory_accounting_issue_view)
            .collect(),
    }
}

fn order_declined_inventory_view(request: &ResolvedSellerOrderRequest) -> OrderInventoryView {
    OrderInventoryView {
        state: "not_reserved".to_owned(),
        listing_event_id: request.listing_event_id.clone(),
        commitment_valid: true,
        bins: Vec::new(),
        issues: Vec::new(),
    }
}

fn order_decision_inventory_for_view(
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    inventory: Option<OrderInventoryView>,
) -> Option<OrderInventoryView> {
    match args.decision {
        OrderDecisionArg::Accept => inventory,
        OrderDecisionArg::Decline => Some(order_declined_inventory_view(request)),
    }
}

fn fetch_current_inventory_listing(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
) -> Result<Result<ResolvedInventoryListing, OrderDecisionView>, RuntimeError> {
    let parsed = parse_listing_addr(request.listing_addr.as_str()).map_err(|error| {
        RuntimeError::Config(format!("order request listing_addr is invalid: {error}"))
    })?;
    let filter = listing_event_filter(&parsed)?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view =
                order_decision_base_view(config, args, "unavailable", config.output.dry_run);
            apply_order_decision_resolution(&mut view, resolution);
            apply_order_decision_request(&mut view, request);
            apply_order_decision_status(&mut view, status);
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            return Ok(Err(view));
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    let listing = current_inventory_listing_from_receipt(request, receipt)?;
    Ok(match listing {
        Some(listing) => Ok(listing),
        None => Err(order_decision_inventory_invalid_view(
            config,
            args,
            request,
            resolution,
            status,
            "order accept refused because the current listing event was not visible",
            vec![issue_with_code(
                "current_listing_missing",
                "listing_event_id",
                "current listing event was not visible on the configured relays",
            )],
        )),
    })
}

fn current_inventory_listing_from_receipt(
    request: &ResolvedSellerOrderRequest,
    receipt: DirectRelayFetchReceipt,
) -> Result<Option<ResolvedInventoryListing>, RuntimeError> {
    let parsed = parse_listing_addr(request.listing_addr.as_str()).map_err(|error| {
        RuntimeError::Config(format!("order request listing_addr is invalid: {error}"))
    })?;
    current_inventory_listing_from_parts(parsed, receipt)
}

fn current_inventory_listing_from_parts(
    parsed: RadrootsTradeListingAddress,
    receipt: DirectRelayFetchReceipt,
) -> Result<Option<ResolvedInventoryListing>, RuntimeError> {
    let mut candidates = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_LISTING {
            continue;
        }
        let event = radroots_event_from_nostr(&event);
        if event.author != parsed.seller_pubkey {
            continue;
        }
        let listing = listing_from_event(event.kind, &event.tags, &event.content)
            .map_err(|error| RuntimeError::Config(format!("decode listing event: {error}")))?;
        if listing.d_tag != parsed.listing_id {
            continue;
        }
        let bins = listing_inventory_bins(&listing)?;
        candidates.push((event.created_at, event.id, listing, bins));
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, event_id, listing, bins)| ResolvedInventoryListing {
            event_id,
            listing,
            bins,
        }))
}

fn listing_inventory_bins(
    listing: &RadrootsListing,
) -> Result<Vec<RadrootsListingInventoryBinAvailability>, RuntimeError> {
    if !listing
        .bins
        .iter()
        .any(|bin| bin.bin_id == listing.primary_bin_id)
    {
        return Err(RuntimeError::Config(
            "current listing primary bin is missing from listing bins".to_owned(),
        ));
    }
    let available_count = listing
        .inventory_available
        .as_ref()
        .ok_or_else(|| {
            RuntimeError::Config("current listing inventory availability is missing".to_owned())
        })?
        .to_u64_exact()
        .ok_or_else(|| {
            RuntimeError::Config(
                "current listing inventory availability must be a whole number".to_owned(),
            )
        })?;
    Ok(vec![RadrootsListingInventoryBinAvailability {
        bin_id: listing.primary_bin_id.clone(),
        available_count,
    }])
}

fn listing_is_active(listing: &RadrootsListing) -> bool {
    match listing.availability.as_ref() {
        Some(RadrootsListingAvailability::Status { status }) => {
            matches!(status, RadrootsListingStatus::Active)
        }
        Some(RadrootsListingAvailability::Window { .. }) | None => true,
    }
}

fn fetch_listing_accounting_requests(
    config: &RuntimeConfig,
    request: &ResolvedSellerOrderRequest,
    listing: &ResolvedInventoryListing,
) -> Result<Vec<ResolvedAccountingRequest>, RuntimeError> {
    let filter = order_listing_request_filter(
        request.seller_pubkey.as_str(),
        request.listing_addr.as_str(),
    )?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_ORDER_REQUEST
            || !event_matches_tag_value(&event, "a", request.listing_addr.as_str())
        {
            continue;
        }
        if let Ok(record) = listing_accounting_request_from_event(&event)
            && record.listing_event_id.as_deref() == Some(listing.event_id.as_str())
        {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_decisions(
    config: &RuntimeConfig,
    request: &ResolvedSellerOrderRequest,
) -> Result<Vec<RadrootsActiveOrderDecisionRecord>, RuntimeError> {
    let filter = order_listing_decision_filter(request.listing_addr.as_str())?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_ORDER_DECISION
            || !event_matches_tag_value(&event, "a", request.listing_addr.as_str())
        {
            continue;
        }
        if let Ok(OrderStatusRecord::Decision(record)) = order_status_record_from_event(&event) {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_fulfillments(
    config: &RuntimeConfig,
    request: &ResolvedSellerOrderRequest,
) -> Result<Vec<RadrootsActiveOrderFulfillmentRecord>, RuntimeError> {
    let filter = order_listing_fulfillment_filter(request.listing_addr.as_str())?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_FULFILLMENT_UPDATE
            || !event_matches_tag_value(&event, "a", request.listing_addr.as_str())
        {
            continue;
        }
        if let Ok(OrderStatusRecord::Fulfillment(record)) = order_status_record_from_event(&event) {
            records.push(record);
        }
    }
    Ok(records)
}

fn fetch_listing_accounting_cancellations(
    config: &RuntimeConfig,
    request: &ResolvedSellerOrderRequest,
) -> Result<Vec<RadrootsActiveOrderCancellationRecord>, RuntimeError> {
    let filter = order_listing_cancellation_filter(request.listing_addr.as_str())?;
    let receipt = fetch_events_from_relays(&config.relay.urls, filter)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let mut records = Vec::new();
    for event in receipt.events {
        if event_kind_u32(&event) != KIND_TRADE_CANCEL
            || !event_matches_tag_value(&event, "a", request.listing_addr.as_str())
        {
            continue;
        }
        if let Ok(OrderStatusRecord::Cancellation(record)) = order_status_record_from_event(&event)
        {
            records.push(record);
        }
    }
    Ok(records)
}

fn listing_accounting_request_from_event(
    event: &RadrootsNostrEvent,
) -> Result<ResolvedAccountingRequest, RuntimeError> {
    let event = radroots_event_from_nostr(event);
    let envelope = active_trade_order_request_from_event(&event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context = active_trade_event_context_from_tags(
        RadrootsActiveTradeMessageType::TradeOrderRequested,
        &event.tags,
    )
    .map_err(|error| RuntimeError::Config(format!("decode order request tags: {error}")))?;
    Ok(ResolvedAccountingRequest {
        listing_event_id: context.listing_event.as_ref().map(|event| event.id.clone()),
        record: RadrootsActiveOrderRequestRecord {
            event_id: event.id,
            author_pubkey: event.author,
            payload: envelope.payload,
        },
    })
}

fn active_request_record_from_resolved(
    request: &ResolvedSellerOrderRequest,
) -> RadrootsActiveOrderRequestRecord {
    RadrootsActiveOrderRequestRecord {
        event_id: request.request_event_id.clone(),
        author_pubkey: request.buyer_pubkey.clone(),
        payload: RadrootsTradeOrderRequested {
            order_id: request.order_id.clone(),
            listing_addr: request.listing_addr.clone(),
            buyer_pubkey: request.buyer_pubkey.clone(),
            seller_pubkey: request.seller_pubkey.clone(),
            items: request.items.clone(),
            economics: request.economics.clone(),
        },
    }
}

fn proposed_accept_decision_record(
    request: &ResolvedSellerOrderRequest,
) -> Result<RadrootsActiveOrderDecisionRecord, RuntimeError> {
    let payload = accepted_order_decision_payload_from_request(request);
    let payload =
        canonicalize_active_order_decision_for_signer(payload, request.seller_pubkey.as_str())
            .map_err(|error| {
                RuntimeError::Config(format!("canonicalize order decision: {error}"))
            })?;
    Ok(RadrootsActiveOrderDecisionRecord {
        event_id: format!("pending_accept:{}", request.order_id),
        author_pubkey: request.seller_pubkey.clone(),
        counterparty_pubkey: request.buyer_pubkey.clone(),
        root_event_id: request.request_event_id.clone(),
        prev_event_id: request.request_event_id.clone(),
        payload,
    })
}

fn order_decision_inventory_invalid_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderDecisionView {
    let mut view = order_decision_base_view(config, args, "invalid", config.output.dry_run);
    apply_order_decision_resolution(&mut view, resolution);
    apply_order_decision_request(&mut view, request);
    apply_order_decision_status(&mut view, status);
    view.reason = Some(reason.into());
    view.issues.extend(issues);
    view.actions = vec![format!("radroots order status get {}", request.order_id)];
    view
}

fn deferred_payment_status_event(event: &RadrootsNostrEvent) -> bool {
    matches!(
        event_kind_u32(event),
        KIND_TRADE_PAYMENT_RECORDED | KIND_TRADE_SETTLEMENT_DECISION
    )
}

fn listing_inventory_accounting_issue_view(
    issue_value: RadrootsListingInventoryAccountingIssue,
) -> OrderIssueView {
    match issue_value {
        RadrootsListingInventoryAccountingIssue::InvalidActiveOrder {
            order_id,
            event_ids,
        } => issue_with_events(
            "invalid_inventory_order",
            "order_id",
            format!("inventory accounting reported invalid active order `{order_id}`"),
            event_ids,
        ),
        RadrootsListingInventoryAccountingIssue::ArithmeticOverflow { bin_id, event_ids } => {
            issue_with_events(
                "listing_inventory_arithmetic_overflow",
                "inventory.count",
                format!("inventory accounting overflowed for bin `{bin_id}`"),
                event_ids,
            )
        }
        RadrootsListingInventoryAccountingIssue::UnknownInventoryBin { bin_id, event_ids } => {
            issue_with_events(
                "unknown_inventory_bin",
                "inventory.bin_id",
                format!("inventory accounting reported unknown bin `{bin_id}`"),
                event_ids,
            )
        }
        RadrootsListingInventoryAccountingIssue::OverReserved {
            bin_id,
            available_count,
            reserved_count,
            event_ids,
        } => issue_with_events(
            "listing_inventory_over_reserved",
            "inventory.available",
            format!(
                "inventory accounting reported bin `{bin_id}` over-reserved: reserved {reserved_count}, available {available_count}"
            ),
            event_ids,
        ),
    }
}

fn order_decision_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    status: &OrderStatusView,
    inventory: Option<OrderInventoryView>,
) -> OrderDecisionView {
    let decision_reason = args
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    let mut view = order_decision_base_view(config, args, "dry_run", true);
    apply_order_decision_request(&mut view, request);
    apply_order_decision_status(&mut view, status);
    view.inventory = order_decision_inventory_for_view(args, request, inventory);
    view.reason = Some(match decision_reason {
        Some(reason) => format!(
            "dry run requested; seller order decision publication skipped with reason `{reason}`"
        ),
        None => "dry run requested; seller order decision publication skipped".to_owned(),
    });
    view.actions = vec![format!("radroots order status get {}", request.order_id)];
    view
}

fn order_revision_invalid_view(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
    status: &OrderStatusView,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderRevisionProposalView {
    let mut view = order_revision_base_view(config, args, "invalid", config.output.dry_run);
    apply_order_revision_status(&mut view, status);
    view.reason = Some(reason.into());
    view.issues.extend(issues);
    view.actions = vec![format!("radroots order status get {}", args.key)];
    view
}

fn order_revision_decision_invalid_view(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
    status: &OrderStatusView,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderRevisionDecisionView {
    let mut view =
        order_revision_decision_base_view(config, args, "invalid", config.output.dry_run);
    apply_order_revision_decision_status(&mut view, status);
    view.reason = Some(reason.into());
    view.issues.extend(issues);
    view.actions = vec![format!("radroots order status get {}", args.key)];
    view
}

fn order_revision_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeOrderRevisionProposed,
) -> OrderRevisionProposalView {
    let mut view = order_revision_base_view(config, args, "dry_run", true);
    apply_order_revision_status(&mut view, status);
    apply_order_revision_payload(&mut view, payload);
    view.reason =
        Some("dry run requested; seller revision proposal publication skipped".to_owned());
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn order_revision_decision_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
    status: &OrderStatusView,
    proposal: &OrderRevisionProposalRecord,
    payload: &RadrootsTradeOrderRevisionDecisionEvent,
) -> OrderRevisionDecisionView {
    let mut view = order_revision_decision_base_view(config, args, "dry_run", true);
    apply_order_revision_decision_status(&mut view, status);
    apply_order_revision_decision_payload(&mut view, proposal, payload);
    view.reason = Some(format!(
        "dry run requested; buyer revision {} publication skipped",
        args.decision.command()
    ));
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn order_fulfillment_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
    status: &OrderStatusView,
    fulfillment_state: RadrootsActiveTradeFulfillmentState,
) -> OrderFulfillmentView {
    let mut view = order_fulfillment_base_view(config, args, "dry_run", true);
    apply_order_fulfillment_status(&mut view, status);
    view.fulfillment_state = fulfillment_state_name(fulfillment_state).to_owned();
    view.reason =
        Some("dry run requested; seller fulfillment update publication skipped".to_owned());
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn order_revision_payload_from_status(
    args: &OrderRevisionProposeArgs,
    status: &OrderStatusView,
) -> Result<RadrootsTradeOrderRevisionProposed, RuntimeError> {
    let revision_id = next_revision_id();
    let economics = status.economics.clone().ok_or_else(|| {
        RuntimeError::Config("accepted order is missing current agreement economics".to_owned())
    })?;
    let economics = revised_order_economics(args, &revision_id, &economics)?;
    let items = economics
        .items
        .iter()
        .map(|item| RadrootsTradeOrderItem {
            bin_id: item.bin_id.clone(),
            bin_count: item.bin_count,
        })
        .collect::<Vec<_>>();
    Ok(RadrootsTradeOrderRevisionProposed {
        revision_id,
        order_id: status.order_id.clone(),
        listing_addr: status.listing_addr.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing listing_addr".to_owned())
        })?,
        buyer_pubkey: status.buyer_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing buyer_pubkey".to_owned())
        })?,
        seller_pubkey: status.seller_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing seller_pubkey".to_owned())
        })?,
        root_event_id: status.request_event_id.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing request_event_id".to_owned())
        })?,
        prev_event_id: status
            .last_event_id
            .clone()
            .or(status.decision_event_id.clone())
            .ok_or_else(|| {
                RuntimeError::Config("accepted order is missing previous event id".to_owned())
            })?,
        items,
        economics,
        reason: args.reason.trim().to_owned(),
    })
}

fn revised_order_economics(
    args: &OrderRevisionProposeArgs,
    revision_id: &str,
    current: &RadrootsTradeOrderEconomics,
) -> Result<RadrootsTradeOrderEconomics, RuntimeError> {
    let mut current_canonical = current.clone();
    current_canonical.canonicalize();
    let mut economics = current_canonical.clone();
    let mut changed = false;
    economics.quote_id = format!("revision_{revision_id}");
    economics.quote_version = economics
        .quote_version
        .checked_add(1)
        .ok_or_else(|| RuntimeError::Config("revision quote_version overflowed".to_owned()))?;

    if let Some(bin_id) = args.bin_id.as_deref().and_then(non_empty_ref) {
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
            "order revision propose requires a changed item count or adjustment".to_owned(),
        ));
    }
    Ok(economics)
}

fn revision_adjustment_line(
    args: &OrderRevisionProposeArgs,
    expected_currency: RadrootsCoreCurrency,
) -> Result<Option<RadrootsTradeOrderEconomicLine>, RuntimeError> {
    let Some(id) = args.adjustment_id.as_deref().and_then(non_empty_ref) else {
        return Ok(None);
    };
    let effect = match args
        .adjustment_effect
        .as_deref()
        .and_then(non_empty_ref)
        .ok_or_else(|| RuntimeError::Config("revision adjustment effect is required".to_owned()))?
    {
        "increase" => RadrootsTradeEconomicEffect::Increase,
        "decrease" => RadrootsTradeEconomicEffect::Decrease,
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
    Ok(Some(RadrootsTradeOrderEconomicLine {
        id: id.to_owned(),
        kind: RadrootsTradeEconomicLineKind::RevisionAdjustment,
        actor: RadrootsTradeEconomicActor::Seller,
        effect,
        amount: RadrootsCoreMoney::new(amount, currency),
        reason: reason.to_owned(),
    }))
}

fn order_revision_event_parts(
    status: &OrderStatusView,
    payload: &RadrootsTradeOrderRevisionProposed,
) -> Result<WireEventParts, RuntimeError> {
    let root_event_id = status.request_event_id.as_deref().ok_or_else(|| {
        RuntimeError::Config("accepted order is missing request_event_id".to_owned())
    })?;
    let prev_event_id = status
        .last_event_id
        .as_deref()
        .or(status.decision_event_id.as_deref())
        .ok_or_else(|| {
            RuntimeError::Config("accepted order is missing previous event id".to_owned())
        })?;
    if payload.root_event_id != root_event_id || payload.prev_event_id != prev_event_id {
        return Err(RuntimeError::Config(
            "order revision proposal payload chain does not match order status".to_owned(),
        ));
    }
    active_trade_order_revision_proposal_event_build(root_event_id, prev_event_id, payload).map_err(
        |error| RuntimeError::Config(format!("encode order revision proposal event: {error}")),
    )
}

fn order_revision_inventory_preflight_view(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeOrderRevisionProposed,
) -> Option<OrderRevisionProposalView> {
    let issues = order_revision_inventory_issues(status, payload);
    if issues.is_empty() {
        return None;
    }
    let mut view = order_revision_invalid_view(
        config,
        args,
        status,
        "order revision propose refused because visible inventory is unavailable for the revised items",
        issues,
    );
    apply_order_revision_payload(&mut view, payload);
    Some(view)
}

fn order_revision_inventory_issues(
    status: &OrderStatusView,
    payload: &RadrootsTradeOrderRevisionProposed,
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
    payload: &RadrootsTradeOrderRevisionProposed,
) {
    view.revision_id = Some(payload.revision_id.clone());
    view.root_event_id = Some(payload.root_event_id.clone());
    view.prev_event_id = Some(payload.prev_event_id.clone());
    view.items = payload
        .items
        .iter()
        .map(|item| OrderDraftItemView {
            bin_id: item.bin_id.clone(),
            bin_count: item.bin_count,
        })
        .collect();
    view.economics = Some(payload.economics.clone());
}

fn apply_order_revision_decision_proposal(
    view: &mut OrderRevisionDecisionView,
    proposal: &OrderRevisionProposalRecord,
) {
    view.revision_id = Some(proposal.payload.revision_id.clone());
    view.root_event_id = Some(proposal.payload.root_event_id.clone());
    view.prev_event_id = Some(proposal.event_id.clone());
    view.event_id = Some(proposal.event_id.clone());
    view.event_kind = Some(KIND_TRADE_ORDER_REVISION);
    if view.decision.as_deref() == Some("accepted") {
        view.economics = Some(proposal.payload.economics.clone());
    }
}

fn apply_order_revision_decision_payload(
    view: &mut OrderRevisionDecisionView,
    proposal: &OrderRevisionProposalRecord,
    payload: &RadrootsTradeOrderRevisionDecisionEvent,
) {
    view.revision_id = Some(payload.revision_id.clone());
    view.root_event_id = Some(payload.root_event_id.clone());
    view.prev_event_id = Some(payload.prev_event_id.clone());
    view.decision = Some(
        match &payload.decision {
            RadrootsTradeOrderRevisionDecision::Accepted => "accepted",
            RadrootsTradeOrderRevisionDecision::Declined { .. } => "declined",
        }
        .to_owned(),
    );
    if matches!(
        payload.decision,
        RadrootsTradeOrderRevisionDecision::Accepted
    ) {
        view.agreement_event_id = view.event_id.clone();
        view.economics = Some(proposal.payload.economics.clone());
    }
}

fn order_revision_decision_payload_from_proposal(
    args: &OrderRevisionDecisionArgs,
    proposal: &OrderRevisionProposalRecord,
) -> Result<RadrootsTradeOrderRevisionDecisionEvent, RuntimeError> {
    let decision = match args.decision {
        OrderRevisionDecisionArg::Accept => RadrootsTradeOrderRevisionDecision::Accepted,
        OrderRevisionDecisionArg::Decline => {
            let reason = args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| {
                    RuntimeError::Config(
                        "order revision decline requires a non-empty reason".to_owned(),
                    )
                })?;
            RadrootsTradeOrderRevisionDecision::Declined {
                reason: reason.to_owned(),
            }
        }
    };
    Ok(RadrootsTradeOrderRevisionDecisionEvent {
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

fn order_revision_decision_event_parts(
    payload: &RadrootsTradeOrderRevisionDecisionEvent,
) -> Result<WireEventParts, RuntimeError> {
    active_trade_order_revision_decision_event_build(
        payload.root_event_id.as_str(),
        payload.prev_event_id.as_str(),
        payload,
    )
    .map_err(|error| RuntimeError::Config(format!("encode order revision decision event: {error}")))
}

fn order_fulfillment_payload_from_status(
    status: &OrderStatusView,
    fulfillment_state: RadrootsActiveTradeFulfillmentState,
) -> Result<RadrootsTradeFulfillmentUpdated, RuntimeError> {
    Ok(RadrootsTradeFulfillmentUpdated {
        order_id: status.order_id.clone(),
        listing_addr: status.listing_addr.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing listing_addr".to_owned())
        })?,
        buyer_pubkey: status.buyer_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing buyer_pubkey".to_owned())
        })?,
        seller_pubkey: status.seller_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("accepted order is missing seller_pubkey".to_owned())
        })?,
        status: fulfillment_state,
    })
}

fn order_fulfillment_event_parts(
    status: &OrderStatusView,
    payload: &RadrootsTradeFulfillmentUpdated,
) -> Result<WireEventParts, RuntimeError> {
    let root_event_id = status.request_event_id.as_deref().ok_or_else(|| {
        RuntimeError::Config("accepted order is missing request_event_id".to_owned())
    })?;
    let prev_event_id = status
        .last_event_id
        .as_deref()
        .or(status.decision_event_id.as_deref())
        .ok_or_else(|| {
            RuntimeError::Config("accepted order is missing previous event id".to_owned())
        })?;
    active_trade_fulfillment_update_event_build(root_event_id, prev_event_id, payload)
        .map_err(|error| RuntimeError::Config(format!("encode fulfillment update event: {error}")))
}

fn order_cancellation_payload_from_status(
    args: &OrderCancelArgs,
    status: &OrderStatusView,
) -> Result<RadrootsTradeOrderCancelled, RuntimeError> {
    Ok(RadrootsTradeOrderCancelled {
        order_id: status.order_id.clone(),
        listing_addr: status.listing_addr.clone().ok_or_else(|| {
            RuntimeError::Config("cancellable order is missing listing_addr".to_owned())
        })?,
        buyer_pubkey: status.buyer_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("cancellable order is missing buyer_pubkey".to_owned())
        })?,
        seller_pubkey: status.seller_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("cancellable order is missing seller_pubkey".to_owned())
        })?,
        reason: args.reason.trim().to_owned(),
    })
}

fn order_cancellation_event_parts(
    status: &OrderStatusView,
    payload: &RadrootsTradeOrderCancelled,
) -> Result<WireEventParts, RuntimeError> {
    let root_event_id = status.request_event_id.as_deref().ok_or_else(|| {
        RuntimeError::Config("cancellable order is missing request_event_id".to_owned())
    })?;
    let prev_event_id = order_cancellation_prev_event_id(status).ok_or_else(|| {
        RuntimeError::Config("cancellable order is missing previous event id".to_owned())
    })?;
    active_trade_order_cancel_event_build(root_event_id, prev_event_id.as_str(), payload)
        .map_err(|error| RuntimeError::Config(format!("encode order cancellation event: {error}")))
}

fn order_receipt_payload_from_status(
    args: &OrderReceiptArgs,
    status: &OrderStatusView,
) -> Result<RadrootsTradeBuyerReceipt, RuntimeError> {
    Ok(RadrootsTradeBuyerReceipt {
        order_id: status.order_id.clone(),
        listing_addr: status.listing_addr.clone().ok_or_else(|| {
            RuntimeError::Config("receiptable order is missing listing_addr".to_owned())
        })?,
        buyer_pubkey: status.buyer_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("receiptable order is missing buyer_pubkey".to_owned())
        })?,
        seller_pubkey: status.seller_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("receiptable order is missing seller_pubkey".to_owned())
        })?,
        received: args.received,
        issue: if args.received {
            None
        } else {
            Some(
                args.issue
                    .as_deref()
                    .map(str::trim)
                    .filter(|issue| !issue.is_empty())
                    .ok_or_else(|| {
                        RuntimeError::Config(
                            "receipt issue is required when received is false".to_owned(),
                        )
                    })?
                    .to_owned(),
            )
        },
        received_at: now_unix(),
    })
}

fn order_receipt_event_parts(
    status: &OrderStatusView,
    payload: &RadrootsTradeBuyerReceipt,
) -> Result<WireEventParts, RuntimeError> {
    let root_event_id = status.request_event_id.as_deref().ok_or_else(|| {
        RuntimeError::Config("receiptable order is missing request_event_id".to_owned())
    })?;
    let prev_event_id = order_receipt_prev_event_id(status).ok_or_else(|| {
        RuntimeError::Config(
            "receiptable order is missing eligible fulfillment event id".to_owned(),
        )
    })?;
    active_trade_buyer_receipt_event_build(root_event_id, prev_event_id.as_str(), payload)
        .map_err(|error| RuntimeError::Config(format!("encode buyer receipt event: {error}")))
}

fn order_payment_payload_from_status(
    args: &OrderPaymentArgs,
    status: &OrderStatusView,
) -> Result<RadrootsTradePaymentRecorded, RuntimeError> {
    let economics = status
        .economics
        .as_ref()
        .ok_or_else(|| RuntimeError::Config("payable order is missing economics".to_owned()))?;
    let agreement_event_id = status.agreement_event_id.clone().ok_or_else(|| {
        RuntimeError::Config("payable order is missing agreement_event_id".to_owned())
    })?;
    let amount = parse_payment_amount(args.amount.as_str())?;
    let currency = parse_payment_currency(args.currency.as_str())?;
    if amount != economics.total.amount {
        return Err(RuntimeError::Config(
            "payment amount must match accepted agreement total".to_owned(),
        ));
    }
    if currency != economics.total.currency {
        return Err(RuntimeError::Config(
            "payment currency must match accepted agreement currency".to_owned(),
        ));
    }
    Ok(RadrootsTradePaymentRecorded {
        order_id: status.order_id.clone(),
        listing_addr: status.listing_addr.clone().ok_or_else(|| {
            RuntimeError::Config("payable order is missing listing_addr".to_owned())
        })?,
        buyer_pubkey: status.buyer_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("payable order is missing buyer_pubkey".to_owned())
        })?,
        seller_pubkey: status.seller_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("payable order is missing seller_pubkey".to_owned())
        })?,
        root_event_id: status.request_event_id.clone().ok_or_else(|| {
            RuntimeError::Config("payable order is missing request_event_id".to_owned())
        })?,
        previous_event_id: order_payment_prev_event_id(status).ok_or_else(|| {
            RuntimeError::Config("payable order is missing payment previous event id".to_owned())
        })?,
        agreement_event_id,
        quote_id: economics.quote_id.clone(),
        quote_version: economics.quote_version,
        economics_digest: radroots_trade_order_economics_digest(economics)
            .map_err(|error| RuntimeError::Config(error.to_string()))?,
        amount,
        currency,
        method: parse_payment_method(args.method.as_str())?,
        reference: args
            .reference
            .as_deref()
            .map(str::trim)
            .filter(|reference| !reference.is_empty())
            .map(str::to_owned),
        paid_at: args.paid_at,
    })
}

fn order_payment_event_parts(
    status: &OrderStatusView,
    payload: &RadrootsTradePaymentRecorded,
) -> Result<WireEventParts, RuntimeError> {
    let root_event_id = status.request_event_id.as_deref().ok_or_else(|| {
        RuntimeError::Config("payable order is missing request_event_id".to_owned())
    })?;
    let prev_event_id = order_payment_prev_event_id(status).ok_or_else(|| {
        RuntimeError::Config("payable order is missing payment previous event id".to_owned())
    })?;
    active_trade_payment_recorded_event_build(root_event_id, prev_event_id.as_str(), payload)
        .map_err(|error| RuntimeError::Config(format!("encode payment recorded event: {error}")))
}

fn order_settlement_payload_from_status(
    args: &OrderSettlementArgs,
    status: &OrderStatusView,
) -> Result<RadrootsTradeSettlementDecisionEvent, RuntimeError> {
    let payment = status
        .payment
        .as_ref()
        .ok_or_else(|| RuntimeError::Config("settleable order is missing payment".to_owned()))?;
    let payment_event_id = payment.payment_event_id.clone().ok_or_else(|| {
        RuntimeError::Config("settleable order is missing payment_event_id".to_owned())
    })?;
    if payment_event_id != args.payment_event_id {
        return Err(RuntimeError::Config(
            "settlement payment event id must match current recorded payment".to_owned(),
        ));
    }
    if payment.state != "recorded" || payment.settlement_state != "pending" {
        return Err(RuntimeError::Config(
            "settlement requires a recorded payment with pending settlement".to_owned(),
        ));
    }
    let decision = settlement_decision_protocol(args.decision);
    Ok(RadrootsTradeSettlementDecisionEvent {
        order_id: status.order_id.clone(),
        listing_addr: status.listing_addr.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing listing_addr".to_owned())
        })?,
        seller_pubkey: status.seller_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing seller_pubkey".to_owned())
        })?,
        buyer_pubkey: status.buyer_pubkey.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing buyer_pubkey".to_owned())
        })?,
        root_event_id: status.request_event_id.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing request_event_id".to_owned())
        })?,
        previous_event_id: payment_event_id.clone(),
        agreement_event_id: payment.agreement_event_id.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing agreement_event_id".to_owned())
        })?,
        payment_event_id,
        quote_id: payment.quote_id.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing quote_id".to_owned())
        })?,
        quote_version: payment.quote_version.ok_or_else(|| {
            RuntimeError::Config("settleable order is missing quote_version".to_owned())
        })?,
        economics_digest: payment.economics_digest.clone().ok_or_else(|| {
            RuntimeError::Config("settleable order is missing economics_digest".to_owned())
        })?,
        amount: payment
            .amount
            .ok_or_else(|| RuntimeError::Config("settleable order is missing amount".to_owned()))?,
        currency: payment.currency.ok_or_else(|| {
            RuntimeError::Config("settleable order is missing currency".to_owned())
        })?,
        decision,
        reason: if matches!(args.decision, OrderSettlementDecisionArg::Reject) {
            Some(
                args.reason
                    .as_deref()
                    .and_then(non_empty_ref)
                    .ok_or_else(|| {
                        RuntimeError::Config("settlement rejection reason is required".to_owned())
                    })?
                    .to_owned(),
            )
        } else {
            None
        },
    })
}

fn order_settlement_event_parts(
    status: &OrderStatusView,
    payload: &RadrootsTradeSettlementDecisionEvent,
) -> Result<WireEventParts, RuntimeError> {
    let root_event_id = status.request_event_id.as_deref().ok_or_else(|| {
        RuntimeError::Config("settleable order is missing request_event_id".to_owned())
    })?;
    active_trade_settlement_decision_event_build(
        root_event_id,
        payload.payment_event_id.as_str(),
        payload,
    )
    .map_err(|error| RuntimeError::Config(format!("encode settlement decision event: {error}")))
}

fn apply_order_payment_payload(
    view: &mut OrderPaymentView,
    payload: &RadrootsTradePaymentRecorded,
) {
    view.root_event_id = Some(payload.root_event_id.clone());
    view.prev_event_id = Some(payload.previous_event_id.clone());
    view.agreement_event_id = Some(payload.agreement_event_id.clone());
    view.quote_id = Some(payload.quote_id.clone());
    view.quote_version = Some(payload.quote_version);
    view.economics_digest = Some(payload.economics_digest.clone());
    view.amount = Some(payload.amount);
    view.currency = Some(payload.currency);
    view.method = Some(payload.method);
    view.reference = payload.reference.clone();
    view.paid_at = payload.paid_at;
}

fn apply_order_settlement_payload(
    view: &mut OrderSettlementView,
    payload: &RadrootsTradeSettlementDecisionEvent,
) {
    view.root_event_id = Some(payload.root_event_id.clone());
    view.prev_event_id = Some(payload.previous_event_id.clone());
    view.payment_event_id = Some(payload.payment_event_id.clone());
    view.agreement_event_id = Some(payload.agreement_event_id.clone());
    view.quote_id = Some(payload.quote_id.clone());
    view.quote_version = Some(payload.quote_version);
    view.economics_digest = Some(payload.economics_digest.clone());
    view.amount = Some(payload.amount);
    view.currency = Some(payload.currency);
    view.decision = Some(payload.decision);
    view.settlement_reason = payload.reason.clone();
}

fn order_cancellation_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderCancelArgs,
    status: &OrderStatusView,
) -> OrderCancellationView {
    let mut view = order_cancellation_base_view(config, args, "dry_run", true);
    apply_order_cancellation_status(&mut view, status);
    view.reason =
        Some("dry run requested; buyer order cancellation publication skipped".to_owned());
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn order_receipt_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeBuyerReceipt,
) -> OrderReceiptView {
    let mut view = order_receipt_base_view(config, args, "dry_run", true);
    apply_order_receipt_status(&mut view, status);
    view.received = payload.received;
    view.issue = payload.issue.clone();
    view.received_at = Some(payload.received_at);
    view.reason = Some("dry run requested; buyer receipt publication skipped".to_owned());
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn order_payment_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradePaymentRecorded,
) -> OrderPaymentView {
    let mut view = order_payment_base_view(config, args, "dry_run", true);
    apply_order_payment_status(&mut view, status);
    apply_order_payment_payload(&mut view, payload);
    view.reason = Some("dry run requested; buyer payment publication skipped".to_owned());
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn order_settlement_dry_run_view(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeSettlementDecisionEvent,
) -> OrderSettlementView {
    let mut view = order_settlement_base_view(config, args, "dry_run", true);
    apply_order_settlement_status(&mut view, status);
    apply_order_settlement_payload(&mut view, payload);
    view.reason = Some("dry run requested; seller settlement publication skipped".to_owned());
    view.actions = vec![format!("radroots order status get {}", status.order_id)];
    view
}

fn publish_order_revision(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
    status: OrderStatusView,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeOrderRevisionProposed,
) -> Result<OrderRevisionProposalView, RuntimeError> {
    let parts = order_revision_event_parts(&status, &payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_revision_view(
        config, args, &status, &payload, event_kind, receipt,
    ))
}

fn publish_order_revision_decision(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
    status: OrderStatusView,
    proposal: &OrderRevisionProposalRecord,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeOrderRevisionDecisionEvent,
) -> Result<OrderRevisionDecisionView, RuntimeError> {
    let parts = order_revision_decision_event_parts(&payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_revision_decision_view(
        config, args, &status, proposal, &payload, event_kind, receipt,
    ))
}

fn published_order_revision_view(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeOrderRevisionProposed,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderRevisionProposalView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let mut view = order_revision_base_view(config, args, "proposed", false);
    apply_order_revision_status(&mut view, status);
    apply_order_revision_payload(&mut view, payload);
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
}

fn published_order_revision_decision_view(
    config: &RuntimeConfig,
    args: &OrderRevisionDecisionArgs,
    status: &OrderStatusView,
    proposal: &OrderRevisionProposalRecord,
    payload: &RadrootsTradeOrderRevisionDecisionEvent,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderRevisionDecisionView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let state = match payload.decision {
        RadrootsTradeOrderRevisionDecision::Accepted => "accepted",
        RadrootsTradeOrderRevisionDecision::Declined { .. } => "declined",
    };
    let mut view = order_revision_decision_base_view(config, args, state, false);
    apply_order_revision_decision_status(&mut view, status);
    apply_order_revision_decision_payload(&mut view, proposal, payload);
    view.revision_id = Some(payload.revision_id.clone());
    view.root_event_id = Some(payload.root_event_id.clone());
    view.prev_event_id = Some(payload.prev_event_id.clone());
    view.event_id = Some(event_id.clone());
    view.event_kind = Some(event_kind);
    if matches!(
        payload.decision,
        RadrootsTradeOrderRevisionDecision::Accepted
    ) {
        view.agreement_event_id = Some(event_id);
    }
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
}

fn publish_order_fulfillment(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
    status: OrderStatusView,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeFulfillmentUpdated,
) -> Result<OrderFulfillmentView, RuntimeError> {
    let parts = order_fulfillment_event_parts(&status, &payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_fulfillment_view(
        config,
        args,
        &status,
        payload.status,
        event_kind,
        receipt,
    ))
}

fn publish_order_cancellation(
    config: &RuntimeConfig,
    args: &OrderCancelArgs,
    status: OrderStatusView,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeOrderCancelled,
) -> Result<OrderCancellationView, RuntimeError> {
    let parts = order_cancellation_event_parts(&status, &payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_cancellation_view(
        config, args, &status, event_kind, receipt,
    ))
}

fn publish_order_receipt(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
    status: OrderStatusView,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeBuyerReceipt,
) -> Result<OrderReceiptView, RuntimeError> {
    let parts = order_receipt_event_parts(&status, &payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_receipt_view(
        config, args, &status, &payload, event_kind, receipt,
    ))
}

fn publish_order_payment(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    status: OrderStatusView,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradePaymentRecorded,
) -> Result<OrderPaymentView, RuntimeError> {
    let parts = order_payment_event_parts(&status, &payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_payment_view(
        config, args, &status, &payload, event_kind, receipt,
    ))
}

fn publish_order_settlement(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
    status: OrderStatusView,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeSettlementDecisionEvent,
) -> Result<OrderSettlementView, RuntimeError> {
    let parts = order_settlement_event_parts(&status, &payload)?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    Ok(published_order_settlement_view(
        config, args, &status, &payload, event_kind, receipt,
    ))
}

fn published_order_fulfillment_view(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
    status: &OrderStatusView,
    fulfillment_state: RadrootsActiveTradeFulfillmentState,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderFulfillmentView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let state = fulfillment_state_name(fulfillment_state);
    let mut view = order_fulfillment_base_view(config, args, state, false);
    apply_order_fulfillment_status(&mut view, status);
    view.fulfillment_state = state.to_owned();
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
}

fn published_order_cancellation_view(
    config: &RuntimeConfig,
    args: &OrderCancelArgs,
    status: &OrderStatusView,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderCancellationView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let mut view = order_cancellation_base_view(config, args, "cancelled", false);
    apply_order_cancellation_status(&mut view, status);
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
}

fn published_order_receipt_view(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeBuyerReceipt,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderReceiptView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let state = if payload.received {
        "completed"
    } else {
        "disputed"
    };
    let mut view = order_receipt_base_view(config, args, state, false);
    apply_order_receipt_status(&mut view, status);
    view.received = payload.received;
    view.issue = payload.issue.clone();
    view.received_at = Some(payload.received_at);
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
}

fn published_order_payment_view(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradePaymentRecorded,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderPaymentView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let mut view = order_payment_base_view(config, args, "recorded", false);
    apply_order_payment_status(&mut view, status);
    apply_order_payment_payload(&mut view, payload);
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
}

fn published_order_settlement_view(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
    status: &OrderStatusView,
    payload: &RadrootsTradeSettlementDecisionEvent,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderSettlementView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let mut view = order_settlement_base_view(
        config,
        args,
        settlement_decision_state(args.decision),
        false,
    );
    apply_order_settlement_status(&mut view, status);
    apply_order_settlement_payload(&mut view, payload);
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view
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

fn order_fulfillment_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderFulfillmentArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderFulfillmentView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view = order_fulfillment_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_fulfillment_status(&mut view, status);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn order_revision_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderRevisionProposeArgs,
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
    args: &OrderRevisionDecisionArgs,
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
    args: &OrderCancelArgs,
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

fn order_receipt_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderReceiptArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderReceiptView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view = order_receipt_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_receipt_status(&mut view, status);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn order_payment_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderPaymentArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderPaymentView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view = order_payment_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_payment_status(&mut view, status);
    view.reason = Some(reason);
    view.actions = actions;
    view
}

fn order_settlement_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderSettlementArgs,
    status: &OrderStatusView,
    error: ActorWriteBindingError,
) -> OrderSettlementView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);
    let mut view = order_settlement_base_view(config, args, state.as_str(), config.output.dry_run);
    apply_order_settlement_status(&mut view, status);
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
    if event_kind != KIND_TRADE_ORDER_REQUEST {
        return Err(RuntimeError::Config(format!(
            "order decision received unexpected kind `{event_kind}`"
        )));
    }

    let event = radroots_event_from_nostr(event);
    let envelope = active_trade_order_request_from_event(&event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context = active_trade_event_context_from_tags(
        RadrootsActiveTradeMessageType::TradeOrderRequested,
        &event.tags,
    )
    .map_err(|error| RuntimeError::Config(format!("decode order request tags: {error}")))?;

    if envelope.order_id != order_id || envelope.payload.order_id != order_id {
        return Err(RuntimeError::Config(
            "order request does not match requested order id".to_owned(),
        ));
    }
    if context.counterparty_pubkey != seller_pubkey
        || envelope.payload.seller_pubkey != seller_pubkey
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
        request_event_id: event.id,
        listing_event_id,
        order_id: envelope.order_id,
        listing_addr: envelope.payload.listing_addr,
        buyer_pubkey: envelope.payload.buyer_pubkey,
        seller_pubkey: envelope.payload.seller_pubkey,
        items: envelope.payload.items,
        economics: envelope.payload.economics,
    })
}

fn publish_order_decision(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: ResolvedSellerOrderRequest,
    resolution: SellerOrderRequestResolution,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeOrderDecisionEvent,
    inventory: Option<OrderInventoryView>,
) -> Result<OrderDecisionView, RuntimeError> {
    let parts = active_trade_order_decision_event_build(
        request.request_event_id.as_str(),
        request.request_event_id.as_str(),
        &payload,
    )
    .map_err(|error| RuntimeError::Config(format!("encode order decision event: {error}")))?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;

    Ok(published_order_decision_view(
        config, args, request, resolution, event_kind, receipt, inventory,
    ))
}

fn canonical_order_decision_payload(
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    signer_pubkey: &str,
) -> Result<RadrootsTradeOrderDecisionEvent, RuntimeError> {
    let payload = order_decision_payload_from_request(args, request)?;
    canonicalize_active_order_decision_for_signer(payload, signer_pubkey)
        .map_err(|error| RuntimeError::Config(format!("canonicalize order decision: {error}")))
}

fn order_decision_payload_from_request(
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
) -> Result<RadrootsTradeOrderDecisionEvent, RuntimeError> {
    match args.decision {
        OrderDecisionArg::Accept => Ok(accepted_order_decision_payload_from_request(request)),
        OrderDecisionArg::Decline => {
            let reason = args
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| {
                    RuntimeError::Config("order decline requires a non-empty reason".to_owned())
                })?;
            Ok(declined_order_decision_payload_from_request(
                request, reason,
            ))
        }
    }
}

fn accepted_order_decision_payload_from_request(
    request: &ResolvedSellerOrderRequest,
) -> RadrootsTradeOrderDecisionEvent {
    RadrootsTradeOrderDecisionEvent {
        order_id: request.order_id.clone(),
        listing_addr: request.listing_addr.clone(),
        buyer_pubkey: request.buyer_pubkey.clone(),
        seller_pubkey: request.seller_pubkey.clone(),
        decision: RadrootsTradeOrderDecision::Accepted {
            inventory_commitments: request
                .items
                .iter()
                .map(|item| RadrootsTradeInventoryCommitment {
                    bin_id: item.bin_id.clone(),
                    bin_count: item.bin_count,
                })
                .collect(),
        },
    }
}

fn declined_order_decision_payload_from_request(
    request: &ResolvedSellerOrderRequest,
    reason: &str,
) -> RadrootsTradeOrderDecisionEvent {
    RadrootsTradeOrderDecisionEvent {
        order_id: request.order_id.clone(),
        listing_addr: request.listing_addr.clone(),
        buyer_pubkey: request.buyer_pubkey.clone(),
        seller_pubkey: request.seller_pubkey.clone(),
        decision: RadrootsTradeOrderDecision::Declined {
            reason: reason.to_owned(),
        },
    }
}

fn published_order_decision_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: ResolvedSellerOrderRequest,
    resolution: SellerOrderRequestResolution,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
    inventory: Option<OrderInventoryView>,
) -> OrderDecisionView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    let mut view = order_decision_base_view(config, args, args.decision.as_str(), false);
    apply_order_decision_request(&mut view, &request);
    view.event_id = Some(event_id);
    view.event_kind = Some(event_kind);
    view.target_relays = target_relays;
    view.connected_relays = resolution.connected_relays;
    view.acknowledged_relays = acknowledged_relays;
    view.failed_relays = relay_failures(failed_relays);
    view.fetched_count = resolution.fetched_count;
    view.decoded_count = resolution.decoded_count;
    view.skipped_count = resolution.skipped_count;
    view.inventory = order_decision_inventory_for_view(args, &request, inventory);
    view
}

fn order_decision_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
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

fn order_history_entry_from_event(
    event: &RadrootsNostrEvent,
    seller_pubkey: &str,
) -> Result<OrderHistoryEntryView, RuntimeError> {
    let event_kind = event_kind_u32(event);
    if event_kind != KIND_TRADE_ORDER_REQUEST {
        return Err(RuntimeError::Config(format!(
            "order event list received unexpected kind `{event_kind}`"
        )));
    }

    let event = radroots_event_from_nostr(event);
    let envelope = active_trade_order_request_from_event(&event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context = active_trade_event_context_from_tags(
        RadrootsActiveTradeMessageType::TradeOrderRequested,
        &event.tags,
    )
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

    Ok(OrderHistoryEntryView {
        id: envelope.order_id.clone(),
        state: "requested".to_owned(),
        event_id: Some(event.id),
        event_kind: Some(event.kind),
        listing_lookup: None,
        listing_addr: Some(envelope.listing_addr),
        listing_event_id,
        buyer_account_id: None,
        buyer_pubkey: Some(envelope.payload.buyer_pubkey),
        seller_pubkey: Some(envelope.payload.seller_pubkey),
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
        .kind(radroots_nostr_kind(KIND_TRADE_ORDER_REQUEST as u16))
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
    listing_addr: &RadrootsTradeListingAddress,
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
        .kind(radroots_nostr_kind(KIND_TRADE_ORDER_REQUEST as u16))
        .limit(1_000);
    let filter = radroots_nostr_filter_tag(filter, "p", vec![seller_pubkey.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order request filter: {error}")))?;
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order request filter: {error}")))
}

fn order_listing_decision_filter(listing_addr: &str) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_TRADE_ORDER_DECISION as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order decision filter: {error}")))
}

fn order_listing_revision_proposal_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_TRADE_ORDER_REVISION as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build revision proposal filter: {error}")))
}

fn order_listing_revision_decision_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(
            KIND_TRADE_ORDER_REVISION_RESPONSE as u16,
        ))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build revision decision filter: {error}")))
}

fn order_listing_fulfillment_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_TRADE_FULFILLMENT_UPDATE as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build fulfillment filter: {error}")))
}

fn order_listing_cancellation_filter(
    listing_addr: &str,
) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kind(radroots_nostr_kind(KIND_TRADE_CANCEL as u16))
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "a", vec![listing_addr.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build cancellation filter: {error}")))
}

fn order_status_filter(order_id: &str) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kinds([
            radroots_nostr_kind(KIND_TRADE_ORDER_REQUEST as u16),
            radroots_nostr_kind(KIND_TRADE_ORDER_DECISION as u16),
            radroots_nostr_kind(KIND_TRADE_ORDER_REVISION as u16),
            radroots_nostr_kind(KIND_TRADE_ORDER_REVISION_RESPONSE as u16),
            radroots_nostr_kind(KIND_TRADE_FULFILLMENT_UPDATE as u16),
            radroots_nostr_kind(KIND_TRADE_CANCEL as u16),
            radroots_nostr_kind(KIND_TRADE_RECEIPT as u16),
        ])
        .limit(1_000);
    radroots_nostr_filter_tag(filter, "d", vec![order_id.to_owned()])
        .map_err(|error| RuntimeError::Config(format!("build order status filter: {error}")))
}

fn event_kind_u32(event: &RadrootsNostrEvent) -> u32 {
    u32::from(event.kind.as_u16())
}

fn validate_scaffold_args(args: &OrderDraftCreateArgs) -> Result<(), RuntimeError> {
    match (normalize_optional(args.bin_id.as_deref()), args.bin_count) {
        (None, Some(_)) => Err(RuntimeError::Config(
            "`--qty` requires `--bin` when creating an order draft".to_owned(),
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
        let listing_event_id =
            resolve_active_listing_event_id(config, listing_addr, &parsed)?.unwrap_or_default();
        let economics_product = resolve_trade_product_by_listing_addr(config, listing_addr)?;
        return Ok(Some(ResolvedOrderListing {
            listing_addr: listing_addr.to_owned(),
            listing_event_id,
            seller_pubkey: parsed.seller_pubkey,
            economics_product,
        }));
    }

    let Some(listing_lookup) = listing_lookup else {
        return Ok(None);
    };

    if !config.local.replica_db_path.exists() {
        return Err(RuntimeError::Config(format!(
            "order listing lookup `{listing_lookup}` requires local market data; run `radroots store init` and `radroots market refresh` before creating an order from a listing"
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
                    "listing `{listing_lookup}` is missing the latest listing event pointer; run `radroots market refresh` before creating an order from this listing"
                ))
            })?;

            Ok(Some(ResolvedOrderListing {
                listing_addr,
                listing_event_id,
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
    parsed: &RadrootsTradeListingAddress,
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
    let state = nostr_event_state::find_one(
        &executor,
        &INostrEventStateFindOne::On(INostrEventStateFindOneArgs {
            on: NostrEventStateQueryBindValues::Key { key },
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
        notes: None,
    }
}

fn order_economics_from_resolved_listing(
    order_id: &str,
    resolved_listing: Option<&ResolvedOrderListing>,
    items: &[OrderDraftItem],
    adjustments: &[crate::runtime_args::OrderDraftAdjustmentArgs],
) -> Result<Option<RadrootsTradeOrderEconomics>, RuntimeError> {
    let Some(listing) = resolved_listing else {
        return Ok(None);
    };
    let Some(product) = listing.economics_product.as_ref() else {
        return Ok(None);
    };
    let Some(primary_bin_id) = product.primary_bin_id.as_deref().and_then(non_empty_ref) else {
        return Ok(None);
    };
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
        economic_items.push(RadrootsTradeOrderEconomicItem {
            bin_id: item.bin_id.clone(),
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
    let mut economics = RadrootsTradeOrderEconomics {
        quote_id: format!("quote_{order_id}"),
        quote_version: 1,
        pricing_basis: RadrootsTradePricingBasis::ListingEvent,
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
) -> Result<Vec<RadrootsTradeOrderEconomicLine>, RuntimeError> {
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
        lines.push(RadrootsTradeOrderEconomicLine {
            id: format!("listing_discount_{}", index + 1),
            kind: RadrootsTradeEconomicLineKind::ListingDiscount,
            actor: RadrootsTradeEconomicActor::Seller,
            effect: RadrootsTradeEconomicEffect::Decrease,
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
    adjustments: &[crate::runtime_args::OrderDraftAdjustmentArgs],
) -> Result<Vec<RadrootsTradeOrderEconomicLine>, RuntimeError> {
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
                "increase" => RadrootsTradeEconomicEffect::Increase,
                "decrease" => RadrootsTradeEconomicEffect::Decrease,
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
            Ok(RadrootsTradeOrderEconomicLine {
                id: adjustment.id.trim().to_owned(),
                kind: RadrootsTradeEconomicLineKind::BasketAdjustment,
                actor: RadrootsTradeEconomicActor::Buyer,
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

fn view_from_loaded(loaded: LoadedOrderDraft) -> OrderGetView {
    let OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        listing_event_id,
        seller_pubkey,
        issues,
    } = inspect_document(&loaded.document);

    let actions = actions_for_document(&loaded.document, loaded.file.as_path(), issues.as_slice());

    OrderGetView {
        state,
        source: ORDER_SOURCE.to_owned(),
        lookup: loaded.document.order.order_id.clone(),
        order_id: Some(loaded.document.order.order_id.clone()),
        file: Some(loaded.file.display().to_string()),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr,
        listing_event_id,
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
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
    }
}

fn summary_from_loaded(loaded: &LoadedOrderDraft) -> OrderSummaryView {
    let OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        listing_event_id,
        seller_pubkey: _,
        issues,
    } = inspect_document(&loaded.document);

    OrderSummaryView {
        id: loaded.document.order.order_id.clone(),
        state,
        ready_for_submit,
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr,
        listing_event_id,
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        item_count: loaded.document.order.items.len(),
        economics: loaded.document.order.economics.clone(),
        updated_at_unix: loaded.updated_at_unix,
        job: None,
        issues,
    }
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
        buyer_account_id: None,
        item_count: 0,
        economics: None,
        updated_at_unix: modified_unix(path).unwrap_or_default(),
        job: None,
        issues: vec![issue_with_code("invalid_order_draft", "draft", reason)],
    }
}

fn inspect_document(document: &OrderDraftDocument) -> OrderInspection {
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
    let issues = collect_issues(document);
    let ready_for_submit = issues.is_empty();
    let state = if ready_for_submit {
        "ready".to_owned()
    } else {
        "draft".to_owned()
    };

    OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        listing_event_id,
        seller_pubkey,
        issues,
    }
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
            "order_id must look like `ord_<base64url>`",
        ));
    }

    match normalize_optional(Some(document.order.listing_addr.as_str())) {
        Some(listing_addr) => match parse_listing_addr(listing_addr.as_str()) {
            Ok(parsed) => {
                if parsed.kind != KIND_LISTING {
                    issues.push(issue(
                        "order.listing_addr",
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
                "order.listing_addr",
                format!("listing_addr is invalid: {error}"),
            )),
        },
        None => issues.push(issue(
            "order.listing_addr",
            "listing_addr is required before order submit",
        )),
    }

    match normalize_optional(Some(document.order.listing_event_id.as_str())) {
        Some(listing_event_id) => {
            if !is_valid_event_id(listing_event_id.as_str()) {
                issues.push(issue(
                    "order.listing_event_id",
                    "listing_event_id must be a 64-character hex Nostr event id",
                ));
            }
        }
        None => issues.push(issue(
            "order.listing_event_id",
            "latest active listing event id is required before order submit; run `radroots market refresh` and create the order from local market data",
        )),
    }

    if document.order.items.is_empty() {
        issues.push(issue(
            "order.items",
            "at least one order item is required before order submit",
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
            "quote economics is required before order submit; run `radroots basket quote create` from current local market data",
        )),
    }

    if document
        .buyer_account_id
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && document.order.buyer_pubkey.trim().is_empty()
    {
        issues.push(issue(
            "buyer_account_id",
            "buyer account or buyer_pubkey is required before order submit",
        ));
    }

    issues
}

fn order_items_match_economics(
    items: &[OrderDraftItem],
    economics: &RadrootsTradeOrderEconomics,
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
    let mut actions = Vec::new();
    actions.push(format!(
        "edit {} and fill the remaining draft fields",
        file.display()
    ));
    if document.buyer_account_id.is_none() && document.order.buyer_pubkey.trim().is_empty() {
        actions.push("radroots account create".to_owned());
    }
    if document.order.items.is_empty()
        || issues
            .iter()
            .any(|issue| issue.field.starts_with("order.items["))
    {
        actions.push(format!("radroots order get {}", document.order.order_id));
    }
    actions
}

fn order_submit_listing_freshness_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(Some(order_submit_unconfigured_view(
            config,
            loaded,
            args,
            "order submit requires local market data to confirm the listing is still active; run `radroots store init` and `radroots market refresh` before submitting",
            vec![issue(
                "order.listing_addr",
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
        .map_err(|error| RuntimeError::Config(format!("order listing_addr is invalid: {error}")))?;
    let active_event_id = match resolve_active_listing_event_id(config, listing_addr, &parsed)? {
        Some(event_id) => event_id,
        None => {
            return Ok(Some(order_submit_unconfigured_view(
                config,
                loaded,
                args,
                "order listing is not active in the local replica; run `radroots market refresh` and create a new order from current market data",
                vec![issue(
                    "order.listing_addr",
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
            "order listing event is no longer current in the local replica; run `radroots market refresh` and create a new order from current market data",
            vec![issue(
                "order.listing_event_id",
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
    args: &OrderSubmitArgs,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(Some(order_submit_unconfigured_view(
            config,
            loaded,
            args,
            "order submit requires local market data to confirm current listing availability; run `radroots store init` and `radroots market refresh` before submitting",
            vec![issue(
                "order.listing_addr",
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
                "order listing is not active in the local replica; run `radroots market refresh` and create a new order from current market data",
                vec![issue(
                    "order.listing_addr",
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
            "order listing bin identity is missing in the local replica",
            vec![issue_with_code(
                "listing_primary_bin_missing",
                "inventory.primary_bin_id",
                "current local replica listing primary bin is required before submit",
            )],
        )));
    };

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
            "order draft references a bin outside the current local listing",
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
                "order listing availability is invalid in the local replica",
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
                "order listing availability is missing in the local replica",
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
    args: &OrderSubmitArgs,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
    mut actions: Vec<String>,
) -> OrderSubmitView {
    actions.push(format!(
        "radroots order get {}",
        loaded.document.order.order_id
    ));

    OrderSubmitView {
        state: "unconfigured".to_owned(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: None,
        signer_session_id: None,
        requested_signer_session_id: None,
        reason: Some(reason.into()),
        job: None,
        issues,
        actions,
    }
}

fn order_submit_invalid_quantity_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "invalid".to_owned(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: None,
        signer_session_id: None,
        requested_signer_session_id: None,
        reason: Some(reason.into()),
        job: None,
        issues,
        actions: vec![
            "radroots market refresh".to_owned(),
            format!("radroots order get {}", loaded.document.order.order_id),
        ],
    }
}

fn order_submit_existing_request_preflight_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    payload: &RadrootsTradeOrderRequested,
) -> Result<Option<OrderSubmitView>, RuntimeError> {
    let filter = order_request_filter(
        loaded.document.order.seller_pubkey.as_str(),
        Some(loaded.document.order.order_id.as_str()),
    )?;
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays: _,
            failed_relays: _,
        }) => {
            return Err(RuntimeError::Network(format!(
                "direct relay connection failed during submit preflight: {reason}"
            )));
        }
        Err(error) => return Err(RuntimeError::Network(error.to_string())),
    };

    order_submit_existing_request_view_from_receipt(config, loaded, args, payload, receipt)
}

fn order_submit_existing_request_view_from_receipt(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    payload: &RadrootsTradeOrderRequested,
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
                format!("request event `{event_id}` failed order submit preflight: {error}"),
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
            "visible order request event conflicts with the local order draft; refusing to publish a second request for the same order id",
            vec![issue_with_events(
                "existing_request_conflict",
                "request_event_id",
                format!(
                    "request event `{}` does not match the local order draft",
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
    let envelope = active_trade_order_request_from_event(&event)
        .map_err(|error| RuntimeError::Config(format!("decode order request event: {error}")))?;
    let context = active_trade_event_context_from_tags(
        RadrootsActiveTradeMessageType::TradeOrderRequested,
        &event.tags,
    )
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
    let payload =
        canonicalize_active_order_request_for_signer(envelope.payload, event.author.as_str())
            .map_err(|error| {
                RuntimeError::Config(format!("canonicalize order request: {error}"))
            })?;
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
    payload: &RadrootsTradeOrderRequested,
) -> bool {
    request.payload == *payload
        && request.listing_event_id.as_deref()
            == Some(loaded.document.order.listing_event_id.as_str())
}

fn order_submit_deduplicated_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    request: &ResolvedOrderSubmitRequest,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "submitted".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: Some(request.request_event_id.clone()),
        event_kind: Some(KIND_TRADE_ORDER_REQUEST),
        dry_run: config.output.dry_run,
        deduplicated: true,
        target_relays,
        acknowledged_relays: connected_relays,
        failed_relays: relay_failures(failed_relays),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        requested_signer_session_id: None,
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
    args: &OrderSubmitArgs,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "dry_run".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: true,
        deduplicated: false,
        target_relays: config.relay.urls.clone(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        requested_signer_session_id: None,
        reason: Some(
            "dry run requested; relay order publication skipped after submit preflight".to_owned(),
        ),
        job: None,
        issues: Vec::new(),
        actions: vec![format!(
            "radroots order submit {}",
            loaded.document.order.order_id
        )],
    }
}

fn order_submit_invalid_existing_request_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    reason: impl Into<String>,
    issues: Vec<OrderIssueView>,
    target_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> OrderSubmitView {
    OrderSubmitView {
        state: "invalid".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: Some(KIND_TRADE_ORDER_REQUEST),
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays,
        acknowledged_relays: Vec::new(),
        failed_relays: relay_failures(failed_relays),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        requested_signer_session_id: None,
        reason: Some(reason.into()),
        job: None,
        issues,
        actions: vec![format!(
            "radroots order status get {}",
            loaded.document.order.order_id
        )],
    }
}

fn canonical_order_request_payload_from_loaded(
    loaded: &LoadedOrderDraft,
    signer_pubkey: &str,
) -> Result<RadrootsTradeOrderRequested, RuntimeError> {
    let economics =
        loaded.document.order.economics.clone().ok_or_else(|| {
            RuntimeError::Config("order draft is missing quote economics".to_owned())
        })?;
    let payload = RadrootsTradeOrderRequested {
        order_id: loaded.document.order.order_id.clone(),
        listing_addr: loaded.document.order.listing_addr.clone(),
        buyer_pubkey: loaded.document.order.buyer_pubkey.clone(),
        seller_pubkey: loaded.document.order.seller_pubkey.clone(),
        items: loaded
            .document
            .order
            .items
            .iter()
            .map(|item| RadrootsTradeOrderItem {
                bin_id: item.bin_id.clone(),
                bin_count: item.bin_count,
            })
            .collect(),
        economics,
    };
    canonicalize_active_order_request_for_signer(payload, signer_pubkey)
        .map_err(|error| RuntimeError::Config(format!("canonicalize order request: {error}")))
}

fn publish_order_request(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeOrderRequested,
) -> Result<OrderSubmitView, RuntimeError> {
    let listing_event = RadrootsNostrEventPtr {
        id: loaded.document.order.listing_event_id.clone(),
        relays: None,
    };
    let parts = active_trade_order_request_event_build(&listing_event, &payload)
        .map_err(|error| RuntimeError::Config(format!("encode order request event: {error}")))?;
    let event_kind = parts.kind;
    let receipt = publish_parts_with_identity(&signing.identity, &config.relay.urls, parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;

    Ok(published_order_submit_view(
        config, loaded, args, event_kind, receipt,
    ))
}

fn published_order_submit_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    event_kind: u32,
    receipt: DirectRelayPublishReceipt,
) -> OrderSubmitView {
    let DirectRelayPublishReceipt {
        event_id,
        created_at: _,
        signature: _,
        target_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;

    OrderSubmitView {
        state: "submitted".to_owned(),
        source: ORDER_SUBMIT_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: Some(event_id),
        event_kind: Some(event_kind),
        dry_run: false,
        deduplicated: false,
        target_relays,
        acknowledged_relays,
        failed_relays: relay_failures(failed_relays),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        requested_signer_session_id: None,
        reason: None,
        job: None,
        issues: Vec::new(),
        actions: Vec::new(),
    }
}

fn order_binding_error_view(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    error: ActorWriteBindingError,
) -> OrderSubmitView {
    let (state, reason, actions) = order_actor_write_binding_error_parts(error);

    let mut actions = actions;
    actions.push(format!(
        "radroots order get {}",
        loaded.document.order.order_id
    ));

    OrderSubmitView {
        state: state.clone(),
        source: ORDER_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        listing_event_id: non_empty_string(loaded.document.order.listing_event_id.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        event_id: None,
        event_kind: None,
        dry_run: config.output.dry_run,
        deduplicated: false,
        target_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        requested_signer_session_id: None,
        reason: Some(reason),
        job: None,
        issues: Vec::new(),
        actions,
    }
}

fn resolve_local_order_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "order submit requires signer mode `local`".to_owned(),
        ));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_decision_signing_identity(
    config: &RuntimeConfig,
    seller_pubkey: &str,
    decision: OrderDecisionArg,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "order {} requires signer mode `local`",
            decision.command()
        )));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(seller_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order seller_pubkey `{seller_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_fulfillment_signing_identity(
    config: &RuntimeConfig,
    seller_pubkey: &str,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "order fulfillment update requires signer mode `local`".to_owned(),
        ));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(seller_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order seller_pubkey `{seller_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_cancellation_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "order cancel requires signer mode `local`".to_owned(),
        ));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_receipt_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "order receipt record requires signer mode `local`".to_owned(),
        ));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_payment_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "order payment record requires signer mode `local`".to_owned(),
        ));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_settlement_signing_identity(
    config: &RuntimeConfig,
    seller_pubkey: &str,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(
            "order settlement decision requires signer mode `local`".to_owned(),
        ));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(seller_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order seller_pubkey `{seller_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn resolve_local_order_revision_decision_signing_identity(
    config: &RuntimeConfig,
    buyer_pubkey: &str,
    args: &OrderRevisionDecisionArgs,
) -> Result<accounts::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Local) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "order revision {} requires signer mode `local`",
            args.decision.command()
        )));
    }
    let signing = accounts::resolve_local_signing_identity(config)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Account(
            accounts::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
            )),
        ));
    }
    Ok(signing)
}

fn parse_fulfillment_state(state: &str) -> Result<RadrootsActiveTradeFulfillmentState, String> {
    match state.trim() {
        "accepted_not_fulfilled" => Ok(RadrootsActiveTradeFulfillmentState::AcceptedNotFulfilled),
        "preparing" => Ok(RadrootsActiveTradeFulfillmentState::Preparing),
        "ready_for_pickup" => Ok(RadrootsActiveTradeFulfillmentState::ReadyForPickup),
        "out_for_delivery" => Ok(RadrootsActiveTradeFulfillmentState::OutForDelivery),
        "delivered" => Ok(RadrootsActiveTradeFulfillmentState::Delivered),
        "seller_cancelled" => Ok(RadrootsActiveTradeFulfillmentState::SellerCancelled),
        other => Err(format!(
            "unsupported fulfillment state `{other}`; expected preparing, ready_for_pickup, out_for_delivery, delivered, or seller_cancelled"
        )),
    }
}

fn fulfillment_state_name(state: RadrootsActiveTradeFulfillmentState) -> &'static str {
    match state {
        RadrootsActiveTradeFulfillmentState::AcceptedNotFulfilled => "accepted_not_fulfilled",
        RadrootsActiveTradeFulfillmentState::Preparing => "preparing",
        RadrootsActiveTradeFulfillmentState::ReadyForPickup => "ready_for_pickup",
        RadrootsActiveTradeFulfillmentState::OutForDelivery => "out_for_delivery",
        RadrootsActiveTradeFulfillmentState::Delivered => "delivered",
        RadrootsActiveTradeFulfillmentState::SellerCancelled => "seller_cancelled",
    }
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
        .map_err(|error| format!("read order draft {}: {error}", path.display()))?;
    let document = toml::from_str::<OrderDraftDocument>(contents.as_str())
        .map_err(|error| format!("parse order draft {}: {error}", path.display()))?;
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
        .map_err(|error| RuntimeError::Config(format!("render order draft: {error}")))?;
    Ok(format!(
        "# radroots order draft v1\n# fill listing_addr and any missing order items before submit\n\n{toml}"
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

fn parse_listing_addr(raw: &str) -> Result<RadrootsTradeListingAddress, String> {
    RadrootsTradeListingAddress::parse(raw).map_err(|error| error.to_string())
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
    mut event_ids: Vec<String>,
) -> OrderIssueView {
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
    let Some(encoded) = value.strip_prefix("ord_") else {
        return false;
    };
    encoded.len() == 22 && is_d_tag_base64url(encoded)
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
            buyer_account_id: view.buyer_account_id,
            buyer_pubkey: view.buyer_pubkey,
            seller_pubkey: view.seller_pubkey,
            ready_for_submit: view.ready_for_submit,
            items: view.items,
            economics: view.economics,
            issues: view.issues,
            actions: view.actions,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_core::{
        RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreMoney, RadrootsCoreUnit,
    };
    use radroots_events::RadrootsNostrEventPtr;
    use radroots_events::kinds::{
        KIND_TRADE_CANCEL, KIND_TRADE_FULFILLMENT_UPDATE, KIND_TRADE_ORDER_DECISION,
        KIND_TRADE_ORDER_REVISION, KIND_TRADE_ORDER_REVISION_RESPONSE, KIND_TRADE_PAYMENT_RECORDED,
        KIND_TRADE_RECEIPT, KIND_TRADE_SETTLEMENT_DECISION,
    };
    use radroots_events::trade::{
        RadrootsActiveTradeFulfillmentState, RadrootsActiveTradeMessageType,
        RadrootsTradeBuyerReceipt, RadrootsTradeFulfillmentUpdated,
        RadrootsTradeInventoryCommitment, RadrootsTradeOrderCancelled, RadrootsTradeOrderDecision,
        RadrootsTradeOrderDecisionEvent, RadrootsTradeOrderEconomicItem,
        RadrootsTradeOrderEconomics, RadrootsTradeOrderItem, RadrootsTradeOrderRequested,
        RadrootsTradeOrderRevisionDecision, RadrootsTradeOrderRevisionDecisionEvent,
        RadrootsTradeOrderRevisionProposed, RadrootsTradePaymentMethod,
        RadrootsTradePaymentRecorded, RadrootsTradePricingBasis, RadrootsTradeSettlementDecision,
        RadrootsTradeSettlementDecisionEvent,
    };
    use radroots_events_codec::trade::{
        active_trade_buyer_receipt_event_build, active_trade_event_context_from_tags,
        active_trade_fulfillment_update_event_build, active_trade_order_cancel_event_build,
        active_trade_order_decision_event_build, active_trade_order_decision_from_event,
        active_trade_order_request_event_build, active_trade_order_revision_decision_event_build,
        active_trade_order_revision_proposal_event_build,
        active_trade_payment_recorded_event_build, active_trade_settlement_decision_event_build,
    };
    use radroots_identity::RadrootsIdentity;
    use radroots_nostr::prelude::{radroots_event_from_nostr, radroots_nostr_build_event};
    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use radroots_trade::order::{
        RadrootsActiveOrderCancellationRecord, RadrootsActiveOrderDecisionRecord,
        RadrootsActiveOrderFulfillmentRecord, RadrootsActiveOrderReceiptRecord,
        RadrootsActiveOrderRevisionDecisionRecord, RadrootsActiveOrderRevisionProposalRecord,
        RadrootsListingInventoryBinAvailability, canonicalize_active_order_decision_for_signer,
        reduce_listing_inventory_accounting,
    };
    use tempfile::tempdir;

    use super::{
        LoadedOrderDraft, ORDER_DRAFT_KIND, ORDER_SUBMIT_SOURCE, OrderDraft, OrderDraftDocument,
        OrderDraftItem, OrderStatusContext, ResolvedOrderEconomicsProduct, ResolvedOrderListing,
        ResolvedSellerOrderRequest, SellerOrderRequestResolution,
        accepted_order_decision_payload_from_request, active_request_record_from_resolved,
        canonical_order_request_payload_from_loaded, collect_issues,
        declined_order_decision_payload_from_request, inspect_document, next_order_id,
        order_accept_inventory_preflight_view_from_projection, order_cancellation_dry_run_view,
        order_cancellation_event_parts, order_cancellation_payload_from_status,
        order_cancellation_preflight_view_from_status, order_decision_dry_run_view,
        order_decision_preflight_view_from_status, order_decision_view_from_resolution,
        order_economics_from_resolved_listing, order_fulfillment_dry_run_view,
        order_fulfillment_preflight_view_from_status, order_history_entry_from_event,
        order_history_from_receipt, order_payment_dry_run_view, order_payment_event_parts,
        order_payment_payload_from_status, order_payment_preflight_view_from_status,
        order_receipt_dry_run_view, order_receipt_event_parts, order_receipt_payload_from_status,
        order_receipt_preflight_view_from_status, order_request_filter,
        order_revision_decision_event_parts, order_revision_decision_payload_from_proposal,
        order_revision_decision_preflight_view_from_status, order_revision_event_parts,
        order_revision_inventory_preflight_view, order_revision_payload_from_status,
        order_revision_preflight_view_from_status, order_revision_proposals_from_events,
        order_settlement_dry_run_view, order_settlement_event_parts,
        order_settlement_payload_from_status, order_settlement_preflight_view_from_status,
        order_status_filter, order_status_from_receipt, order_status_from_receipt_with_context,
        order_status_from_receipt_with_deferred_payment,
        order_status_reduction_from_receipt_with_context, order_submit_dry_run_view,
        order_submit_existing_request_view_from_receipt, proposed_accept_decision_record,
        resolve_local_order_fulfillment_signing_identity,
        seller_order_request_resolution_from_receipt,
    };
    use crate::runtime::accounts;
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, PublishConfig, PublishMode, PublishModeSource, RelayConfig, RelayConfigSource,
        RelayPublishPolicy, RpcConfig, RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime::direct_relay::DirectRelayFetchReceipt;
    use crate::runtime_args::{
        OrderCancelArgs, OrderDecisionArg, OrderDecisionArgs, OrderDraftAdjustmentArgs,
        OrderFulfillmentArgs, OrderPaymentArgs, OrderReceiptArgs, OrderRevisionDecisionArg,
        OrderRevisionDecisionArgs, OrderRevisionProposeArgs, OrderSettlementArgs,
        OrderSettlementDecisionArg, OrderSubmitArgs,
    };

    #[test]
    fn generated_order_id_uses_stable_prefix() {
        let order_id = next_order_id();
        assert!(order_id.starts_with("ord_"));
        assert_eq!(order_id.len(), 26);
    }

    #[test]
    fn order_draft_kind_constant_is_stable() {
        let document = OrderDraftDocument {
            version: 1,
            kind: ORDER_DRAFT_KIND.to_owned(),
            order: OrderDraft {
                order_id: "ord_AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
                listing_addr: "30402:deadbeef:AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
                listing_event_id: "1".repeat(64),
                buyer_pubkey: "a".repeat(64),
                seller_pubkey: "b".repeat(64),
                items: vec![OrderDraftItem {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
                economics: Some(sample_order_economics(
                    "ord_AAAAAAAAAAAAAAAAAAAAAg",
                    "bin-1",
                    2,
                )),
            },
            listing_lookup: Some("fresh-eggs".to_owned()),
            buyer_account_id: Some("acct_demo".to_owned()),
        };

        let rendered = toml::to_string_pretty(&document).expect("render draft");
        assert!(rendered.contains("kind = \"order_draft_v1\""));
        assert!(rendered.contains("order_id = \"ord_AAAAAAAAAAAAAAAAAAAAAg\""));
        assert!(rendered.contains("listing_event_id"));
    }

    #[test]
    fn order_economics_applies_listing_discounts_and_basket_adjustments() {
        let listing = ResolvedOrderListing {
            listing_addr: "30402:seller:AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
            listing_event_id: "1".repeat(64),
            seller_pubkey: "seller".to_owned(),
            economics_product: Some(ResolvedOrderEconomicsProduct {
                qty_amt_exact: Some("1".to_owned()),
                qty_unit: "each".to_owned(),
                price_amt_exact: Some("10".to_owned()),
                price_currency: "USD".to_owned(),
                price_qty_amt_exact: Some("1".to_owned()),
                price_qty_unit: "each".to_owned(),
                primary_bin_id: Some("bin-1".to_owned()),
                notes: Some(
                    serde_json::json!({
                        "listing_discounts": [{
                            "scope": "bin",
                            "threshold": {
                                "kind": "bin_count",
                                "amount": { "bin_id": "bin-1", "min": 1 }
                            },
                            "value": {
                                "kind": "percent",
                                "amount": { "value": "10" }
                            }
                        }]
                    })
                    .to_string(),
                ),
            }),
        };
        let items = vec![OrderDraftItem {
            bin_id: "bin-1".to_owned(),
            bin_count: 2,
        }];
        let adjustments = vec![OrderDraftAdjustmentArgs {
            id: "adj_delivery".to_owned(),
            effect: "increase".to_owned(),
            amount: "2".to_owned(),
            currency: "USD".to_owned(),
            reason: "delivery".to_owned(),
        }];

        let economics = order_economics_from_resolved_listing(
            "ord_AAAAAAAAAAAAAAAAAAAAAg",
            Some(&listing),
            items.as_slice(),
            adjustments.as_slice(),
        )
        .expect("economics")
        .expect("economics present");

        assert_eq!(
            economics.subtotal,
            RadrootsCoreMoney::new(RadrootsCoreDecimal::from(20), RadrootsCoreCurrency::USD)
        );
        assert_eq!(economics.discounts.len(), 1);
        assert_eq!(
            economics.discounts[0].amount,
            RadrootsCoreMoney::new(RadrootsCoreDecimal::from(2), RadrootsCoreCurrency::USD)
        );
        assert_eq!(economics.adjustments.len(), 1);
        assert_eq!(economics.adjustments[0].id, "adj_delivery");
        assert_eq!(
            economics.adjustments[0].amount,
            RadrootsCoreMoney::new(RadrootsCoreDecimal::from(2), RadrootsCoreCurrency::USD)
        );
        assert_eq!(
            economics.total,
            RadrootsCoreMoney::new(RadrootsCoreDecimal::from(20), RadrootsCoreCurrency::USD)
        );
    }

    #[test]
    fn order_economics_uses_exact_listing_values_over_display_projection() {
        let listing = ResolvedOrderListing {
            listing_addr: "30402:seller:AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
            listing_event_id: "1".repeat(64),
            seller_pubkey: "seller".to_owned(),
            economics_product: Some(ResolvedOrderEconomicsProduct {
                qty_amt_exact: Some("0.5".to_owned()),
                qty_unit: "each".to_owned(),
                price_amt_exact: Some("10.25".to_owned()),
                price_currency: "USD".to_owned(),
                price_qty_amt_exact: Some("1".to_owned()),
                price_qty_unit: "each".to_owned(),
                primary_bin_id: Some("bin-1".to_owned()),
                notes: None,
            }),
        };
        let items = vec![OrderDraftItem {
            bin_id: "bin-1".to_owned(),
            bin_count: 2,
        }];

        let economics = order_economics_from_resolved_listing(
            "ord_AAAAAAAAAAAAAAAAAAAAAg",
            Some(&listing),
            items.as_slice(),
            &[],
        )
        .expect("economics")
        .expect("economics present");

        assert_eq!(
            economics.subtotal,
            RadrootsCoreMoney::new(
                "10.25".parse::<RadrootsCoreDecimal>().unwrap(),
                RadrootsCoreCurrency::USD
            )
        );
    }

    #[test]
    fn order_economics_fails_when_exact_listing_source_is_missing() {
        let listing = ResolvedOrderListing {
            listing_addr: "30402:seller:AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
            listing_event_id: "1".repeat(64),
            seller_pubkey: "seller".to_owned(),
            economics_product: Some(ResolvedOrderEconomicsProduct {
                qty_amt_exact: None,
                qty_unit: "kg".to_owned(),
                price_amt_exact: Some("3.25".to_owned()),
                price_currency: "USD".to_owned(),
                price_qty_amt_exact: Some("1".to_owned()),
                price_qty_unit: "kg".to_owned(),
                primary_bin_id: Some("bin-a".to_owned()),
                notes: None,
            }),
        };
        let items = vec![OrderDraftItem {
            bin_id: "bin-a".to_owned(),
            bin_count: 1,
        }];

        let error = order_economics_from_resolved_listing(
            "ord_AAAAAAAAAAAAAAAAAAAAAg",
            Some(&listing),
            items.as_slice(),
            &[],
        )
        .expect_err("missing exact source should fail");

        assert!(matches!(
            error,
            crate::runtime::RuntimeError::Config(message)
                if message.contains("listing qty_amt_exact exact source is missing")
        ));
    }

    #[test]
    fn order_draft_requires_listing_event_id_for_submit_readiness() {
        let document = OrderDraftDocument {
            version: 1,
            kind: ORDER_DRAFT_KIND.to_owned(),
            order: OrderDraft {
                order_id: "ord_AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
                listing_addr: "30402:deadbeef:AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
                listing_event_id: String::new(),
                buyer_pubkey: "a".repeat(64),
                seller_pubkey: "deadbeef".to_owned(),
                items: vec![OrderDraftItem {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
                economics: Some(sample_order_economics(
                    "ord_AAAAAAAAAAAAAAAAAAAAAg",
                    "bin-1",
                    2,
                )),
            },
            listing_lookup: Some("fresh-eggs".to_owned()),
            buyer_account_id: Some("acct_demo".to_owned()),
        };

        let inspection = inspect_document(&document);
        assert_eq!(inspection.state, "draft");
        assert!(!inspection.ready_for_submit);
        assert!(
            collect_issues(&document)
                .iter()
                .any(|issue| issue.field == "order.listing_event_id")
        );
    }

    #[test]
    fn order_request_event_decodes_to_history_entry() {
        let buyer = RadrootsIdentity::generate();
        let seller = RadrootsIdentity::generate();
        let buyer_pubkey = buyer.public_key_hex();
        let seller_pubkey = seller.public_key_hex();
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let listing_event_id = "1".repeat(64);
        let payload = RadrootsTradeOrderRequested {
            order_id: "ord_AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
            listing_addr: listing_addr.clone(),
            buyer_pubkey: buyer_pubkey.clone(),
            seller_pubkey: seller_pubkey.clone(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count: 2,
            }],
            economics: sample_order_economics("ord_AAAAAAAAAAAAAAAAAAAAAg", "bin-1", 2),
        };
        let parts = active_trade_order_request_event_build(
            &RadrootsNostrEventPtr {
                id: listing_event_id.clone(),
                relays: None,
            },
            &payload,
        )
        .expect("order request parts");
        let event = radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed order request");

        let entry =
            order_history_entry_from_event(&event, seller_pubkey.as_str()).expect("history entry");

        assert_eq!(entry.id, "ord_AAAAAAAAAAAAAAAAAAAAAg");
        assert_eq!(entry.state, "requested");
        assert_eq!(entry.event_kind, Some(3422));
        assert_eq!(entry.listing_addr.as_deref(), Some(listing_addr.as_str()));
        assert_eq!(
            entry.listing_event_id.as_deref(),
            Some(listing_event_id.as_str())
        );
        assert_eq!(entry.buyer_pubkey.as_deref(), Some(buyer_pubkey.as_str()));
        assert_eq!(entry.seller_pubkey.as_deref(), Some(seller_pubkey.as_str()));
        assert_eq!(entry.item_count, Some(1));
    }

    #[test]
    fn order_request_filter_includes_order_id_d_tag_when_provided() {
        let filter = order_request_filter("a", Some("ord_AAAAAAAAAAAAAAAAAAAAAg"))
            .expect("order request filter");
        let value = serde_json::to_value(filter).expect("filter json");

        assert_eq!(value["kinds"][0], 3422);
        assert_eq!(value["#p"][0], "a");
        assert_eq!(value["#d"][0], "ord_AAAAAAAAAAAAAAAAAAAAAg");
    }

    #[test]
    fn order_status_filter_includes_only_initial_active_lifecycle_kinds() {
        let filter = order_status_filter("ord_AAAAAAAAAAAAAAAAAAAAAg").expect("status filter");
        let value = serde_json::to_value(filter).expect("filter json");
        let kinds = value["kinds"].as_array().expect("kinds array");

        assert!(kinds.contains(&serde_json::json!(3422)));
        assert!(kinds.contains(&serde_json::json!(3423)));
        assert!(kinds.contains(&serde_json::json!(3424)));
        assert!(kinds.contains(&serde_json::json!(3425)));
        assert!(kinds.contains(&serde_json::json!(3433)));
        assert!(kinds.contains(&serde_json::json!(3432)));
        assert!(kinds.contains(&serde_json::json!(3434)));
        assert!(!kinds.contains(&serde_json::json!(3435)));
        assert!(!kinds.contains(&serde_json::json!(3436)));
        assert_eq!(value["#d"][0], "ord_AAAAAAAAAAAAAAAAAAAAAg");
    }

    #[test]
    fn order_revision_payload_updates_items_and_economics() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event.clone()],
            },
        );
        let args = revision_args_for_fixture(&fixture, 3);

        let payload =
            order_revision_payload_from_status(&args, &status_view).expect("revision payload");
        let parts =
            order_revision_event_parts(&status_view, &payload).expect("revision event parts");
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeOrderRevisionProposed,
            &parts.tags,
        )
        .expect("revision context");
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();

        assert_eq!(payload.items[0].bin_id, "bin-1");
        assert_eq!(payload.items[0].bin_count, 3);
        assert_eq!(payload.economics.items[0].bin_count, 3);
        assert_eq!(payload.economics.quote_version, 2);
        assert!(payload.economics.quote_id.starts_with("revision_rev_"));
        assert_eq!(payload.reason, "update count");
        assert_eq!(parts.kind, KIND_TRADE_ORDER_REVISION);
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
    }

    #[test]
    fn order_revision_decision_payload_uses_pending_proposal_chain() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let revision_event = signed_order_revision_proposal_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            3,
        );
        let revision_event_id = revision_event.id.to_string();
        let candidates =
            order_revision_proposals_from_events(fixture.order_id.as_str(), &[revision_event]);
        let proposal = candidates.records.first().expect("revision proposal");
        let args = revision_decision_args_for_fixture(
            &fixture,
            proposal.payload.revision_id.as_str(),
            OrderRevisionDecisionArg::Accept,
        );

        let payload = order_revision_decision_payload_from_proposal(&args, proposal)
            .expect("revision decision payload");
        let parts =
            order_revision_decision_event_parts(&payload).expect("revision decision event parts");
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeOrderRevisionDecision,
            &parts.tags,
        )
        .expect("revision decision context");

        assert_eq!(payload.revision_id, proposal.payload.revision_id);
        assert_eq!(payload.prev_event_id, revision_event_id);
        assert_eq!(parts.kind, KIND_TRADE_ORDER_REVISION_RESPONSE);
        let request_event_id = fixture.request_event.id.to_string();
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(revision_event_id.as_str())
        );
    }

    #[test]
    fn order_revision_decision_preflight_allows_selected_buyer_pending_proposal() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let revision_event = signed_order_revision_proposal_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            3,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    revision_event.clone(),
                ],
            },
        );
        let candidates =
            order_revision_proposals_from_events(fixture.order_id.as_str(), &[revision_event]);
        let args = revision_decision_args_for_fixture(
            &fixture,
            "rev_test",
            OrderRevisionDecisionArg::Accept,
        );

        let view = order_revision_decision_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
            &candidates,
        );

        assert!(view.is_none());
    }

    #[test]
    fn order_revision_decision_preflight_rejects_selected_non_buyer_account() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let revision_event = signed_order_revision_proposal_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            3,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    revision_event.clone(),
                ],
            },
        );
        let candidates =
            order_revision_proposals_from_events(fixture.order_id.as_str(), &[revision_event]);
        let args = revision_decision_args_for_fixture(
            &fixture,
            "rev_test",
            OrderRevisionDecisionArg::Accept,
        );

        let view = order_revision_decision_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
            &candidates,
        )
        .expect("non buyer revision decision preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("selected account is not buyer")
        );
    }

    #[test]
    fn order_status_from_receipt_applies_accepted_revision_decision() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let revision_event = signed_order_revision_proposal_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            3,
        );
        let revision_decision_event = signed_order_revision_decision_event(
            &fixture.buyer,
            &revision_event,
            RadrootsTradeOrderRevisionDecision::Accepted,
        );
        let revision_decision_event_id = revision_decision_event.id.to_string();

        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    revision_event,
                    revision_decision_event,
                ],
            },
        );

        assert_eq!(status_view.state, "accepted");
        assert_eq!(
            status_view.last_event_id.as_deref(),
            Some(revision_decision_event_id.as_str())
        );
        assert_eq!(
            status_view.agreement_event_id.as_deref(),
            Some(revision_decision_event_id.as_str())
        );
        assert_eq!(
            status_view
                .economics
                .as_ref()
                .expect("current economics")
                .items[0]
                .bin_count,
            3
        );
    }

    #[test]
    fn order_status_from_receipt_preserves_agreement_after_declined_revision_decision() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let decision_event_id = decision_event.id.to_string();
        let revision_event = signed_order_revision_proposal_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            3,
        );
        let revision_decision_event = signed_order_revision_decision_event(
            &fixture.buyer,
            &revision_event,
            RadrootsTradeOrderRevisionDecision::Declined {
                reason: "keep original order".to_owned(),
            },
        );
        let revision_decision_event_id = revision_decision_event.id.to_string();

        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    revision_event,
                    revision_decision_event,
                ],
            },
        );

        assert_eq!(status_view.state, "accepted");
        assert_eq!(
            status_view.last_event_id.as_deref(),
            Some(revision_decision_event_id.as_str())
        );
        assert_eq!(
            status_view.agreement_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(
            status_view
                .economics
                .as_ref()
                .expect("current economics")
                .items[0]
                .bin_count,
            2
        );
    }

    #[test]
    fn order_revision_preflight_rejects_selected_non_seller_account() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event],
            },
        );
        let args = revision_args_for_fixture(&fixture, 3);
        let candidates = order_revision_proposals_from_events(fixture.order_id.as_str(), &[]);

        let view = order_revision_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
            &candidates,
        )
        .expect("non seller revision preflight");

        assert_eq!(view.state, "invalid");
        assert!(view.event_id.is_none());
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("selected account is not seller")
        );
    }

    #[test]
    fn order_revision_preflight_ignores_deferred_payment_events() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let args = revision_args_for_fixture(&fixture, 3);
        let candidates = order_revision_proposals_from_events(fixture.order_id.as_str(), &[]);

        let view = order_revision_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
            &candidates,
        );

        assert!(view.is_none());
        assert_eq!(status_view.fetched_count, 3);
        assert_eq!(status_view.decoded_count, 2);
        assert_eq!(status_view.skipped_count, 1);
        assert!(status_view.payment.is_none());
    }

    #[test]
    fn order_revision_preflight_rejects_pending_revision_candidate() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let revision_event = signed_order_revision_proposal_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            3,
        );
        let revision_event_id = revision_event.id.to_string();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    revision_event.clone(),
                ],
            },
        );
        let args = revision_args_for_fixture(&fixture, 1);
        let candidates =
            order_revision_proposals_from_events(fixture.order_id.as_str(), &[revision_event]);

        let view = order_revision_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
            &candidates,
        )
        .expect("pending revision preflight");

        assert_eq!(view.state, "forked");
        assert_eq!(view.event_id.as_deref(), Some(revision_event_id.as_str()));
        assert_eq!(view.event_kind, Some(KIND_TRADE_ORDER_REVISION));
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "pending_revision_exists");
        assert_eq!(view.issues[0].event_ids, vec![revision_event_id]);
    }

    #[test]
    fn order_revision_inventory_preflight_rejects_unavailable_increase() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event],
            },
        );
        let args = revision_args_for_fixture(&fixture, 3);
        let payload =
            order_revision_payload_from_status(&args, &status_view).expect("revision payload");

        let view = order_revision_inventory_preflight_view(&config, &args, &status_view, &payload)
            .expect("unavailable inventory preflight");

        assert_eq!(view.state, "invalid");
        assert_eq!(
            view.revision_id.as_deref(),
            Some(payload.revision_id.as_str())
        );
        assert!(
            view.issues
                .iter()
                .any(|issue| issue.code == "revision_inventory_unavailable")
        );
        assert!(view.event_id.is_none());
    }

    #[test]
    fn order_submit_existing_request_preflight_deduplicates_identical_request() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let loaded = loaded_order_draft_for_fixture(&fixture);
        let payload =
            canonical_order_request_payload_from_loaded(&loaded, fixture.buyer_pubkey.as_str())
                .expect("canonical order request payload");
        let event_id = fixture.request_event.id.to_string();
        let args = OrderSubmitArgs {
            key: fixture.order_id.clone(),
            idempotency_key: Some("idem-submit".to_owned()),
        };
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone()],
        };

        let view = order_submit_existing_request_view_from_receipt(
            &config, &loaded, &args, &payload, receipt,
        )
        .expect("submit existing request preflight")
        .expect("deduplicated view");

        assert_eq!(view.state, "submitted");
        assert_eq!(view.deduplicated, true);
        assert_eq!(view.event_id.as_deref(), Some(event_id.as_str()));
        assert_eq!(view.event_kind, Some(3422));
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.acknowledged_relays, vec!["ws://relay.test"]);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem-submit"));
    }

    #[test]
    fn order_submit_dry_run_view_preserves_preflighted_no_publish_fields() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let loaded = loaded_order_draft_for_fixture(&fixture);
        let args = OrderSubmitArgs {
            key: fixture.order_id.clone(),
            idempotency_key: Some("idem-dry-submit".to_owned()),
        };

        let view = order_submit_dry_run_view(&config, &loaded, &args);

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.source, ORDER_SUBMIT_SOURCE);
        assert_eq!(view.dry_run, true);
        assert_eq!(view.deduplicated, false);
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert!(view.acknowledged_relays.is_empty());
        assert!(view.failed_relays.is_empty());
        assert_eq!(view.signer_mode.as_deref(), Some("local"));
        assert_eq!(view.idempotency_key.as_deref(), Some("idem-dry-submit"));
    }

    #[test]
    fn order_submit_dry_run_deduplicates_identical_visible_request() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let loaded = loaded_order_draft_for_fixture(&fixture);
        let payload =
            canonical_order_request_payload_from_loaded(&loaded, fixture.buyer_pubkey.as_str())
                .expect("canonical order request payload");
        let event_id = fixture.request_event.id.to_string();
        let args = OrderSubmitArgs {
            key: fixture.order_id.clone(),
            idempotency_key: Some("idem-dry-dedupe".to_owned()),
        };
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone()],
        };

        let view = order_submit_existing_request_view_from_receipt(
            &config, &loaded, &args, &payload, receipt,
        )
        .expect("submit existing request preflight")
        .expect("deduplicated view");

        assert_eq!(view.state, "submitted");
        assert_eq!(view.dry_run, true);
        assert_eq!(view.deduplicated, true);
        assert_eq!(view.event_id.as_deref(), Some(event_id.as_str()));
        assert_eq!(view.event_kind, Some(3422));
        assert_eq!(view.acknowledged_relays, vec!["ws://relay.test"]);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem-dry-dedupe"));
    }

    #[test]
    fn order_submit_existing_request_preflight_rejects_changed_request() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let loaded = loaded_order_draft_for_fixture(&fixture);
        let payload =
            canonical_order_request_payload_from_loaded(&loaded, fixture.buyer_pubkey.as_str())
                .expect("canonical order request payload");
        let changed_event = signed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let changed_event_id = changed_event.id.to_string();
        let args = OrderSubmitArgs {
            key: fixture.order_id.clone(),
            idempotency_key: None,
        };
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![changed_event],
        };

        let view = order_submit_existing_request_view_from_receipt(
            &config, &loaded, &args, &payload, receipt,
        )
        .expect("submit existing request preflight")
        .expect("invalid view");

        assert_eq!(view.state, "invalid");
        assert_eq!(view.deduplicated, false);
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "existing_request_conflict");
        assert_eq!(view.issues[0].event_ids, vec![changed_event_id]);
        assert_eq!(
            view.actions,
            vec![format!("radroots order status get {}", fixture.order_id)]
        );
    }

    #[test]
    fn order_submit_existing_request_preflight_rejects_changed_economics() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let loaded = loaded_order_draft_for_fixture(&fixture);
        let payload =
            canonical_order_request_payload_from_loaded(&loaded, fixture.buyer_pubkey.as_str())
                .expect("canonical order request payload");
        let changed_event = signed_order_request_event_with_economics(
            &fixture.buyer,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            fixture.listing_event_id.as_str(),
            sample_order_economics_with_unit_price(fixture.order_id.as_str(), "bin-1", 2, 7),
        );
        let changed_event_id = changed_event.id.to_string();
        let args = OrderSubmitArgs {
            key: fixture.order_id.clone(),
            idempotency_key: None,
        };
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![changed_event],
        };

        let view = order_submit_existing_request_view_from_receipt(
            &config, &loaded, &args, &payload, receipt,
        )
        .expect("submit existing request preflight")
        .expect("invalid view");

        assert_eq!(view.state, "invalid");
        assert_eq!(view.deduplicated, false);
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "existing_request_conflict");
        assert_eq!(view.issues[0].event_ids, vec![changed_event_id]);
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("conflicts with the local order draft")
        );
    }

    #[test]
    fn order_history_counts_decoded_before_order_id_narrowing() {
        let seller = RadrootsIdentity::generate();
        let other_seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let other_seller_pubkey = other_seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let first_order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let second_order_id = "ord_AAAAAAAAAAAAAAAAAAAAAw";
        let first_listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let second_listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAw");
        let other_listing_addr = format!("30402:{other_seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let listing_event_id = "1".repeat(64);
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                signed_order_request_event(
                    &buyer,
                    first_order_id,
                    first_listing_addr.as_str(),
                    buyer_pubkey.as_str(),
                    seller_pubkey.as_str(),
                    listing_event_id.as_str(),
                ),
                signed_order_request_event(
                    &buyer,
                    second_order_id,
                    second_listing_addr.as_str(),
                    buyer_pubkey.as_str(),
                    seller_pubkey.as_str(),
                    listing_event_id.as_str(),
                ),
                signed_order_request_event(
                    &buyer,
                    "ord_AAAAAAAAAAAAAAAAAAAABA",
                    other_listing_addr.as_str(),
                    buyer_pubkey.as_str(),
                    other_seller_pubkey.as_str(),
                    listing_event_id.as_str(),
                ),
            ],
        };

        let history = order_history_from_receipt(seller_pubkey, Some(first_order_id), receipt);

        assert_eq!(history.fetched_count, 3);
        assert_eq!(history.decoded_count, 2);
        assert_eq!(history.skipped_count, 1);
        assert_eq!(history.count, 1);
        assert_eq!(history.orders[0].id, first_order_id);
    }

    #[test]
    fn seller_order_request_resolution_matches_selected_seller_order() {
        let seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let event = signed_order_request_event(
            &buyer,
            order_id,
            listing_addr.as_str(),
            buyer_pubkey.as_str(),
            seller_pubkey.as_str(),
            listing_event_id.as_str(),
        );
        let event_id = event.id.to_string();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![event],
        };

        let resolution =
            seller_order_request_resolution_from_receipt(seller_pubkey.as_str(), order_id, receipt)
                .expect("seller order request resolution");

        assert_eq!(resolution.fetched_count, 1);
        assert_eq!(resolution.decoded_count, 1);
        assert_eq!(resolution.skipped_count, 0);
        assert_eq!(resolution.requests.len(), 1);
        assert_eq!(resolution.requests[0].request_event_id, event_id);
        assert_eq!(resolution.requests[0].order_id, order_id);
        assert_eq!(
            resolution.requests[0].listing_event_id.as_deref(),
            Some(listing_event_id.as_str())
        );
        assert_eq!(resolution.requests[0].listing_addr, listing_addr);
        assert_eq!(resolution.requests[0].buyer_pubkey, buyer_pubkey);
        assert_eq!(resolution.requests[0].seller_pubkey, seller_pubkey);
        assert_eq!(resolution.requests[0].items.len(), 1);
    }

    #[test]
    fn accepted_order_decision_payload_derives_inventory_commitments() {
        let seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![signed_order_request_event(
                &buyer,
                order_id,
                listing_addr.as_str(),
                buyer_pubkey.as_str(),
                seller_pubkey.as_str(),
                listing_event_id.as_str(),
            )],
        };
        let resolution =
            seller_order_request_resolution_from_receipt(seller_pubkey.as_str(), order_id, receipt)
                .expect("seller order request resolution");
        let request = resolution
            .requests
            .first()
            .expect("resolved request")
            .clone();

        let payload = accepted_order_decision_payload_from_request(&request);

        assert_eq!(payload.order_id, order_id);
        assert_eq!(payload.listing_addr, listing_addr);
        assert_eq!(payload.buyer_pubkey, buyer_pubkey);
        assert_eq!(payload.seller_pubkey, seller_pubkey);
        let RadrootsTradeOrderDecision::Accepted {
            inventory_commitments,
        } = payload.decision
        else {
            panic!("expected accepted decision");
        };
        assert_eq!(inventory_commitments.len(), 1);
        assert_eq!(inventory_commitments[0].bin_id, "bin-1");
        assert_eq!(inventory_commitments[0].bin_count, 2);
    }

    #[test]
    fn accepted_order_decision_event_uses_request_chain_tags() {
        let seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![signed_order_request_event(
                &buyer,
                order_id,
                listing_addr.as_str(),
                buyer_pubkey.as_str(),
                seller_pubkey.as_str(),
                listing_event_id.as_str(),
            )],
        };
        let resolution =
            seller_order_request_resolution_from_receipt(seller_pubkey.as_str(), order_id, receipt)
                .expect("seller order request resolution");
        let request = resolution
            .requests
            .first()
            .expect("resolved request")
            .clone();
        let payload = accepted_order_decision_payload_from_request(&request);
        let payload =
            canonicalize_active_order_decision_for_signer(payload, seller_pubkey.as_str())
                .expect("canonical decision payload");
        let parts = active_trade_order_decision_event_build(
            request.request_event_id.as_str(),
            request.request_event_id.as_str(),
            &payload,
        )
        .expect("decision event parts");

        assert_eq!(parts.kind, KIND_TRADE_ORDER_DECISION);
        let event = radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed order decision");
        let event = radroots_event_from_nostr(&event);
        let envelope =
            active_trade_order_decision_from_event(&event).expect("decoded decision event");
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeOrderDecision,
            &event.tags,
        )
        .expect("decision event context");

        assert_eq!(envelope.order_id, order_id);
        assert_eq!(envelope.payload.seller_pubkey, seller_pubkey);
        assert_eq!(envelope.payload.buyer_pubkey, buyer_pubkey);
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
    }

    #[test]
    fn declined_order_decision_payload_uses_decline_reason() {
        let seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![signed_order_request_event(
                &buyer,
                order_id,
                listing_addr.as_str(),
                buyer_pubkey.as_str(),
                seller_pubkey.as_str(),
                listing_event_id.as_str(),
            )],
        };
        let resolution =
            seller_order_request_resolution_from_receipt(seller_pubkey.as_str(), order_id, receipt)
                .expect("seller order request resolution");
        let request = resolution
            .requests
            .first()
            .expect("resolved request")
            .clone();

        let payload = declined_order_decision_payload_from_request(&request, "out of stock");

        assert_eq!(payload.order_id, order_id);
        assert_eq!(payload.listing_addr, listing_addr);
        assert_eq!(payload.buyer_pubkey, buyer_pubkey);
        assert_eq!(payload.seller_pubkey, seller_pubkey);
        let RadrootsTradeOrderDecision::Declined { reason } = payload.decision else {
            panic!("expected declined decision");
        };
        assert_eq!(reason, "out of stock");
    }

    #[test]
    fn declined_order_decision_event_uses_request_chain_tags() {
        let seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![signed_order_request_event(
                &buyer,
                order_id,
                listing_addr.as_str(),
                buyer_pubkey.as_str(),
                seller_pubkey.as_str(),
                listing_event_id.as_str(),
            )],
        };
        let resolution =
            seller_order_request_resolution_from_receipt(seller_pubkey.as_str(), order_id, receipt)
                .expect("seller order request resolution");
        let request = resolution
            .requests
            .first()
            .expect("resolved request")
            .clone();
        let payload = declined_order_decision_payload_from_request(&request, " out of stock ");
        let payload =
            canonicalize_active_order_decision_for_signer(payload, seller_pubkey.as_str())
                .expect("canonical decision payload");
        let parts = active_trade_order_decision_event_build(
            request.request_event_id.as_str(),
            request.request_event_id.as_str(),
            &payload,
        )
        .expect("decision event parts");

        assert_eq!(parts.kind, KIND_TRADE_ORDER_DECISION);
        let event = radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed order decision");
        let event = radroots_event_from_nostr(&event);
        let envelope =
            active_trade_order_decision_from_event(&event).expect("decoded decision event");
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeOrderDecision,
            &event.tags,
        )
        .expect("decision event context");

        assert_eq!(envelope.order_id, order_id);
        assert_eq!(envelope.payload.seller_pubkey, seller_pubkey);
        assert_eq!(envelope.payload.buyer_pubkey, buyer_pubkey);
        let RadrootsTradeOrderDecision::Declined { reason } = envelope.payload.decision else {
            panic!("expected declined decision");
        };
        assert_eq!(reason, "out of stock");
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
    }

    #[test]
    fn order_status_from_receipt_reports_missing() {
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: Vec::new(),
        };

        let view = order_status_from_receipt(order_id, receipt);

        assert_eq!(view.state, "missing");
        assert_eq!(view.order_id, order_id);
        assert_eq!(view.fetched_count, 0);
        assert_eq!(view.decoded_count, 0);
        assert_eq!(view.skipped_count, 0);
        assert!(view.request_event_id.is_none());
        assert!(view.economics.is_none());
        assert!(view.fulfillment.is_none());
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_requested() {
        let fixture = order_status_fixture();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone()],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let request_event_id = fixture.request_event.id.to_string();

        assert_eq!(view.state, "requested");
        assert_eq!(
            view.request_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert!(view.decision_event_id.is_none());
        assert_eq!(
            view.listing_addr.as_deref(),
            Some(fixture.listing_addr.as_str())
        );
        assert_eq!(
            view.listing_event_id.as_deref(),
            Some(fixture.listing_event_id.as_str())
        );
        assert_eq!(
            view.buyer_pubkey.as_deref(),
            Some(fixture.buyer_pubkey.as_str())
        );
        assert_eq!(
            view.seller_pubkey.as_deref(),
            Some(fixture.seller_pubkey.as_str())
        );
        assert_eq!(
            view.economics,
            Some(sample_order_economics(
                fixture.order_id.as_str(),
                "bin-1",
                2
            ))
        );
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 0);
        assert!(view.fulfillment.is_none());
    }

    #[test]
    fn order_status_with_selected_seller_skips_wrong_seller_same_order_request() {
        let fixture = order_status_fixture();
        let other_seller = RadrootsIdentity::generate();
        let other_seller_pubkey = other_seller.public_key_hex();
        let other_listing_addr = format!("30402:{other_seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAw");
        let other_request_event = signed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            other_listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            other_seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), other_request_event],
        };
        let expected_request_event_id = fixture.request_event.id.to_string();

        let view = order_status_from_receipt_with_context(
            OrderStatusContext {
                order_id: fixture.order_id.as_str(),
                selected_account_pubkey: Some(fixture.seller_pubkey.as_str()),
            },
            receipt,
        );

        assert_eq!(view.state, "requested");
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 1);
        assert_eq!(
            view.request_event_id.as_deref(),
            Some(expected_request_event_id.as_str())
        );
        assert_eq!(
            view.economics,
            Some(sample_order_economics(
                fixture.order_id.as_str(),
                "bin-1",
                2
            ))
        );
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_with_selected_seller_ignores_malformed_wrong_seller_candidate() {
        let fixture = order_status_fixture();
        let other_seller = RadrootsIdentity::generate();
        let other_seller_pubkey = other_seller.public_key_hex();
        let other_listing_addr = format!("30402:{other_seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAw");
        let invalid_event = signed_malformed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            other_listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            other_seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), invalid_event],
        };

        let view = order_status_from_receipt_with_context(
            OrderStatusContext {
                order_id: fixture.order_id.as_str(),
                selected_account_pubkey: Some(fixture.seller_pubkey.as_str()),
            },
            receipt,
        );

        assert_eq!(view.state, "requested");
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 1);
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_fulfillment_preflight_uses_selected_seller_context() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let other_seller = RadrootsIdentity::generate();
        let other_seller_pubkey = other_seller.public_key_hex();
        let other_listing_addr = format!("30402:{other_seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAw");
        let other_request_event = signed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            other_listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            other_seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let unscoped_reduction = order_status_reduction_from_receipt_with_context(
            OrderStatusContext {
                order_id: fixture.order_id.as_str(),
                selected_account_pubkey: None,
            },
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    other_request_event.clone(),
                    decision_event.clone(),
                ],
            },
        );
        let scoped_reduction = order_status_reduction_from_receipt_with_context(
            OrderStatusContext {
                order_id: fixture.order_id.as_str(),
                selected_account_pubkey: Some(fixture.seller_pubkey.as_str()),
            },
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    other_request_event,
                    decision_event,
                ],
            },
        );
        let args = fulfillment_args_for_fixture(&fixture, "ready_for_pickup");

        assert_eq!(unscoped_reduction.view.state, "invalid");
        assert_eq!(scoped_reduction.view.state, "accepted");
        assert_eq!(scoped_reduction.view.decoded_count, 2);
        assert_eq!(scoped_reduction.view.skipped_count, 1);
        assert!(
            order_fulfillment_preflight_view_from_status(
                &config,
                &args,
                &scoped_reduction.view,
                scoped_reduction.fulfillment_status,
                scoped_reduction.fulfillment_event_id.as_deref(),
            )
            .is_none()
        );
    }

    #[test]
    fn order_decision_dry_run_view_preserves_ready_preflight_without_publish_fields() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let resolution = request_resolution_for_fixture(&fixture);
        let request = resolution.requests[0].clone();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Accept,
            reason: None,
            idempotency_key: Some("idem_dry_run".to_owned()),
        };

        let view = order_decision_dry_run_view(&config, &args, &request, &status_view, None);

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.dry_run, true);
        assert_eq!(view.order_id, fixture.order_id);
        assert_eq!(
            view.request_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            view.root_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            view.listing_addr.as_deref(),
            Some(fixture.listing_addr.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert!(view.acknowledged_relays.is_empty());
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 1);
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 0);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem_dry_run"));
        assert_eq!(
            view.actions,
            vec![format!("radroots order status get {}", fixture.order_id)]
        );
    }

    #[test]
    fn order_decline_dry_run_view_preserves_ready_preflight_without_publish_fields() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let resolution = request_resolution_for_fixture(&fixture);
        let request = resolution.requests[0].clone();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Decline,
            reason: Some(" out of stock ".to_owned()),
            idempotency_key: Some("idem_decline_dry_run".to_owned()),
        };

        let view = order_decision_dry_run_view(&config, &args, &request, &status_view, None);

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.decision, "declined");
        assert_eq!(view.dry_run, true);
        assert_eq!(
            view.reason.as_deref(),
            Some(
                "dry run requested; seller order decision publication skipped with reason `out of stock`"
            )
        );
        assert_eq!(
            view.request_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            view.root_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(
            view.listing_addr.as_deref(),
            Some(fixture.listing_addr.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert!(view.acknowledged_relays.is_empty());
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 1);
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 0);
        assert_eq!(
            view.idempotency_key.as_deref(),
            Some("idem_decline_dry_run")
        );
    }

    #[test]
    fn order_status_from_receipt_reports_accepted() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), decision_event.clone()],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();

        assert_eq!(view.state, "accepted");
        assert_eq!(
            view.economics,
            Some(sample_order_economics(
                fixture.order_id.as_str(),
                "bin-1",
                2
            ))
        );
        assert_eq!(
            view.decision_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(
            view.last_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(
            view.listing_event_id.as_deref(),
            Some(fixture.listing_event_id.as_str())
        );
        let inventory = view.inventory.as_ref().expect("inventory view");
        assert_eq!(inventory.state, "reserved");
        assert_eq!(inventory.commitment_valid, true);
        assert_eq!(
            inventory.listing_event_id.as_deref(),
            Some(fixture.listing_event_id.as_str())
        );
        assert_eq!(inventory.bins.len(), 1);
        assert_eq!(inventory.bins[0].bin_id, "bin-1");
        assert_eq!(inventory.bins[0].committed_count, 2);
        let fulfillment = view.fulfillment.as_ref().expect("fulfillment view");
        assert_eq!(fulfillment.state, "accepted_not_fulfilled");
        assert_eq!(fulfillment.event_id, None);
        assert_eq!(
            fulfillment.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            fulfillment.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(fulfillment.terminal, false);
        assert_eq!(fulfillment.inventory_released, false);
        assert!(fulfillment.issues.is_empty());
        assert!(view.reducer_issues.is_empty());
        assert_eq!(view.decoded_count, 2);
    }

    #[test]
    fn order_status_from_receipt_reports_requested_cancellation() {
        let fixture = order_status_fixture();
        let cancellation_event = signed_order_cancellation_event(
            &fixture.buyer,
            &fixture.request_event,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "buyer cancelled",
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), cancellation_event.clone()],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let request_event_id = fixture.request_event.id.to_string();
        let cancellation_event_id = cancellation_event.id.to_string();
        let lifecycle = view.lifecycle.as_ref().expect("lifecycle view");
        let cancellation = lifecycle.cancellation.as_ref().expect("cancellation view");
        let inventory = view.inventory.as_ref().expect("inventory view");

        assert_eq!(
            u32::from(cancellation_event.kind.as_u16()),
            KIND_TRADE_CANCEL
        );
        assert_eq!(view.state, "cancelled");
        assert_eq!(
            view.last_event_id.as_deref(),
            Some(cancellation_event_id.as_str())
        );
        assert_eq!(lifecycle.phase, "cancelled");
        assert_eq!(lifecycle.terminal, true);
        assert_eq!(
            lifecycle.event_id.as_deref(),
            Some(cancellation_event_id.as_str())
        );
        assert_eq!(
            lifecycle.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            lifecycle.prev_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(lifecycle.settlement_required, false);
        assert_eq!(lifecycle.settlement_reason, None);
        assert_eq!(cancellation.event_id, cancellation_event_id);
        assert_eq!(
            cancellation.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            cancellation.prev_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(cancellation.reason.as_deref(), Some("buyer cancelled"));
        assert_eq!(inventory.state, "not_reserved");
        assert_eq!(inventory.commitment_valid, true);
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_accepted_cancellation() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let cancellation_event = signed_order_cancellation_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "buyer cannot collect",
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                decision_event.clone(),
                cancellation_event.clone(),
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let decision_event_id = decision_event.id.to_string();
        let cancellation_event_id = cancellation_event.id.to_string();
        let lifecycle = view.lifecycle.as_ref().expect("lifecycle view");
        let inventory = view.inventory.as_ref().expect("inventory view");

        assert_eq!(view.state, "cancelled");
        assert_eq!(
            view.decision_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(inventory.state, "released");
        assert_eq!(inventory.commitment_valid, true);
        assert_eq!(lifecycle.phase, "cancelled");
        assert_eq!(lifecycle.terminal, true);
        assert_eq!(
            lifecycle.event_id.as_deref(),
            Some(cancellation_event_id.as_str())
        );
        assert_eq!(
            lifecycle.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(lifecycle.settlement_required, false);
        assert_eq!(lifecycle.settlement_reason, None);
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_request_cancellation_decision_fork() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let cancellation_event = signed_order_cancellation_event(
            &fixture.buyer,
            &fixture.request_event,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "buyer cancelled",
        );
        let mut expected_event_ids = vec![
            decision_event.id.to_string(),
            cancellation_event.id.to_string(),
        ];
        expected_event_ids.sort();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                decision_event,
                cancellation_event,
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let lifecycle = view.lifecycle.as_ref().expect("lifecycle view");

        assert_eq!(view.state, "invalid");
        assert_eq!(lifecycle.phase, "invalid");
        assert_eq!(lifecycle.terminal, true);
        assert_eq!(view.reducer_issues.len(), 1);
        assert_eq!(view.reducer_issues[0].code, "forked_lifecycle");
        assert_eq!(view.reducer_issues[0].event_ids, expected_event_ids);
        assert_eq!(lifecycle.issues[0].code, "forked_lifecycle");
    }

    #[test]
    fn order_cancellation_event_parts_chain_from_request_or_decision() {
        let fixture = order_status_fixture();
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let args = cancel_args_for_fixture(&fixture, "buyer cancelled");
        let requested_status = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );

        assert!(
            order_cancellation_preflight_view_from_status(
                &config,
                &args,
                &requested_status,
                fixture.buyer_pubkey.as_str()
            )
            .is_none()
        );
        let requested_payload = order_cancellation_payload_from_status(&args, &requested_status)
            .expect("requested cancellation payload");
        let requested_parts = order_cancellation_event_parts(&requested_status, &requested_payload)
            .expect("requested cancellation parts");
        let request_event_id = fixture.request_event.id.to_string();
        let requested_context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeOrderCancelled,
            &requested_parts.tags,
        )
        .expect("requested cancellation context");

        assert_eq!(requested_parts.kind, KIND_TRADE_CANCEL);
        assert_eq!(
            requested_context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            requested_context.prev_event_id.as_deref(),
            Some(request_event_id.as_str())
        );

        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let accepted_status = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event.clone()],
            },
        );

        assert!(
            order_cancellation_preflight_view_from_status(
                &config,
                &args,
                &accepted_status,
                fixture.buyer_pubkey.as_str()
            )
            .is_none()
        );
        let accepted_payload = order_cancellation_payload_from_status(&args, &accepted_status)
            .expect("accepted cancellation payload");
        let accepted_parts = order_cancellation_event_parts(&accepted_status, &accepted_payload)
            .expect("accepted cancellation parts");
        let decision_event_id = decision_event.id.to_string();
        let accepted_context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeOrderCancelled,
            &accepted_parts.tags,
        )
        .expect("accepted cancellation context");

        assert_eq!(accepted_parts.kind, KIND_TRADE_CANCEL);
        assert_eq!(
            accepted_context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            accepted_context.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
    }

    #[test]
    fn order_cancellation_dry_run_view_preserves_preflight_without_publish_fields() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event.clone()],
            },
        );
        let args = OrderCancelArgs {
            key: fixture.order_id.clone(),
            reason: "buyer cancelled".to_owned(),
            idempotency_key: Some("idem_cancel".to_owned()),
        };

        let view = order_cancellation_dry_run_view(&config, &args, &status_view);
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.dry_run, true);
        assert_eq!(
            view.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 2);
        assert_eq!(view.decoded_count, 2);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem_cancel"));
    }

    #[test]
    fn order_cancellation_preflight_rejects_fulfilled_order() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event.clone(),
                ],
            },
        );
        let args = cancel_args_for_fixture(&fixture, "buyer cancelled");

        let view = order_cancellation_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("fulfilled cancellation preflight");

        assert_eq!(view.state, "fulfilled");
        assert_eq!(view.event_id, None);
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already has seller fulfillment")
        );
    }

    #[test]
    fn order_cancellation_preflight_ignores_deferred_payment_events() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let args = cancel_args_for_fixture(&fixture, "buyer cancelled");

        let view = order_cancellation_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        );

        assert!(view.is_none());
        assert_eq!(status_view.fetched_count, 3);
        assert_eq!(status_view.decoded_count, 2);
        assert_eq!(status_view.skipped_count, 1);
        assert!(status_view.payment.is_none());
    }

    #[test]
    fn order_cancellation_preflight_rejects_selected_non_buyer_account() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );
        let args = cancel_args_for_fixture(&fixture, "buyer cancelled");

        let view = order_cancellation_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
        )
        .expect("non buyer cancellation preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("selected account is not buyer")
        );
        assert!(view.event_id.is_none());
    }

    #[test]
    fn order_cancellation_preflight_rejects_completed_order_as_terminal() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let receipt_event = signed_buyer_receipt_event(
            &fixture.buyer,
            &fixture.request_event,
            &fulfillment_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            true,
            None,
        );
        let receipt_event_id = receipt_event.id.to_string();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event,
                    receipt_event,
                ],
            },
        );
        let args = cancel_args_for_fixture(&fixture, "buyer cancelled");

        let view = order_cancellation_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("completed cancellation preflight");

        assert_eq!(view.state, "terminal");
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(receipt_event_id.as_str())
        );
        assert_eq!(view.event_id, None);
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already terminal")
        );
    }

    #[test]
    fn order_status_from_receipt_reports_completed_buyer_receipt() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let receipt_event = signed_buyer_receipt_event(
            &fixture.buyer,
            &fixture.request_event,
            &fulfillment_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            true,
            None,
        );
        let view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event.clone(),
                    receipt_event.clone(),
                ],
            },
        );
        let receipt_event_id = receipt_event.id.to_string();
        let fulfillment_event_id = fulfillment_event.id.to_string();
        let lifecycle = view.lifecycle.as_ref().expect("lifecycle view");
        let receipt = lifecycle.receipt.as_ref().expect("receipt view");
        let inventory = view.inventory.as_ref().expect("inventory view");
        let fulfillment = view.fulfillment.as_ref().expect("fulfillment view");

        assert_eq!(u32::from(receipt_event.kind.as_u16()), KIND_TRADE_RECEIPT);
        assert_eq!(view.state, "completed");
        assert_eq!(
            view.last_event_id.as_deref(),
            Some(receipt_event_id.as_str())
        );
        assert_eq!(inventory.state, "reserved");
        assert_eq!(fulfillment.state, "delivered");
        assert_eq!(
            fulfillment.event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(lifecycle.phase, "completed");
        assert_eq!(lifecycle.terminal, true);
        assert_eq!(
            lifecycle.event_id.as_deref(),
            Some(receipt_event_id.as_str())
        );
        assert_eq!(
            lifecycle.prev_event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(lifecycle.settlement_required, false);
        assert_eq!(lifecycle.settlement_reason, None);
        assert_eq!(receipt.event_id, receipt_event_id);
        assert_eq!(
            receipt.prev_event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(receipt.received, true);
        assert_eq!(receipt.issue, None);
        assert_eq!(receipt.received_at, Some(1_777_665_600));
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_disputed_buyer_receipt() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let receipt_event = signed_buyer_receipt_event(
            &fixture.buyer,
            &fixture.request_event,
            &fulfillment_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            false,
            Some("damaged items"),
        );
        let view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event.clone(),
                    receipt_event.clone(),
                ],
            },
        );
        let receipt_event_id = receipt_event.id.to_string();
        let fulfillment_event_id = fulfillment_event.id.to_string();
        let lifecycle = view.lifecycle.as_ref().expect("lifecycle view");
        let receipt = lifecycle.receipt.as_ref().expect("receipt view");
        let fulfillment = view.fulfillment.as_ref().expect("fulfillment view");

        assert_eq!(view.state, "disputed");
        assert_eq!(fulfillment.state, "ready_for_pickup");
        assert_eq!(
            fulfillment.event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(lifecycle.phase, "disputed");
        assert_eq!(lifecycle.terminal, true);
        assert_eq!(
            lifecycle.event_id.as_deref(),
            Some(receipt_event_id.as_str())
        );
        assert_eq!(lifecycle.settlement_required, false);
        assert_eq!(lifecycle.settlement_reason, None);
        assert_eq!(receipt.received, false);
        assert_eq!(receipt.issue.as_deref(), Some("damaged items"));
        assert_eq!(receipt.received_at, Some(1_777_665_600));
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_receipt_fulfillment_fork() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let ready_fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let delivered_fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &ready_fulfillment_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let receipt_event = signed_buyer_receipt_event(
            &fixture.buyer,
            &fixture.request_event,
            &ready_fulfillment_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            true,
            None,
        );
        let mut expected_event_ids = vec![
            delivered_fulfillment_event.id.to_string(),
            receipt_event.id.to_string(),
        ];
        expected_event_ids.sort();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                decision_event,
                ready_fulfillment_event,
                delivered_fulfillment_event,
                receipt_event,
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let lifecycle = view.lifecycle.as_ref().expect("lifecycle view");

        assert_eq!(view.state, "invalid");
        assert_eq!(lifecycle.phase, "invalid");
        assert_eq!(lifecycle.terminal, true);
        assert_eq!(view.reducer_issues.len(), 1);
        assert_eq!(view.reducer_issues[0].code, "forked_lifecycle");
        assert_eq!(view.reducer_issues[0].event_ids, expected_event_ids);
        assert_eq!(lifecycle.issues[0].code, "forked_lifecycle");
    }

    #[test]
    fn order_receipt_event_parts_chain_from_latest_eligible_fulfillment() {
        let fixture = order_status_fixture();
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event.clone(),
                ],
            },
        );
        let args = receipt_args_for_fixture(&fixture, true, None);

        assert!(
            order_receipt_preflight_view_from_status(
                &config,
                &args,
                &status_view,
                fixture.buyer_pubkey.as_str()
            )
            .is_none()
        );
        let payload =
            order_receipt_payload_from_status(&args, &status_view).expect("receipt payload");
        let parts = order_receipt_event_parts(&status_view, &payload).expect("receipt parts");
        let request_event_id = fixture.request_event.id.to_string();
        let fulfillment_event_id = fulfillment_event.id.to_string();
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeBuyerReceipt,
            &parts.tags,
        )
        .expect("receipt context");

        assert_eq!(payload.received, true);
        assert!(payload.received_at > 0);
        assert_eq!(parts.kind, KIND_TRADE_RECEIPT);
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
    }

    #[test]
    fn order_receipt_dry_run_view_preserves_preflight_without_publish_fields() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event.clone(),
                ],
            },
        );
        let args = OrderReceiptArgs {
            key: fixture.order_id.clone(),
            received: false,
            issue: Some("damaged items".to_owned()),
            idempotency_key: Some("idem_receipt".to_owned()),
        };
        let payload =
            order_receipt_payload_from_status(&args, &status_view).expect("receipt payload");

        let view = order_receipt_dry_run_view(&config, &args, &status_view, &payload);
        let request_event_id = fixture.request_event.id.to_string();
        let fulfillment_event_id = fulfillment_event.id.to_string();

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.dry_run, true);
        assert_eq!(
            view.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert_eq!(view.received, false);
        assert_eq!(view.issue.as_deref(), Some("damaged items"));
        assert_eq!(view.received_at, Some(payload.received_at));
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 3);
        assert_eq!(view.decoded_count, 3);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem_receipt"));
    }

    #[test]
    fn order_payment_event_parts_bind_current_agreement_terms() {
        let fixture = order_status_fixture();
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event.clone()],
            },
        );
        let args = payment_args_for_fixture(&fixture);

        assert!(
            order_payment_preflight_view_from_status(
                &config,
                &args,
                &status_view,
                fixture.buyer_pubkey.as_str()
            )
            .is_none()
        );
        let payload =
            order_payment_payload_from_status(&args, &status_view).expect("payment payload");
        let parts = order_payment_event_parts(&status_view, &payload).expect("payment parts");
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradePaymentRecorded,
            &parts.tags,
        )
        .expect("payment context");

        assert_eq!(parts.kind, KIND_TRADE_PAYMENT_RECORDED);
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(payload.agreement_event_id, decision_event_id);
        assert_eq!(payload.quote_id, format!("quote_{}", fixture.order_id));
        assert_eq!(payload.quote_version, 1);
        assert_eq!(payload.amount, RadrootsCoreDecimal::from(12u32));
        assert_eq!(payload.currency, RadrootsCoreCurrency::USD);
        assert_eq!(payload.method, RadrootsTradePaymentMethod::ManualTransfer);
        assert_eq!(payload.reference.as_deref(), Some("memo-1"));
        assert_eq!(payload.paid_at, Some(1_777_666_000));
    }

    #[test]
    fn order_payment_dry_run_view_preserves_payment_payload_without_event_id() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event.clone()],
            },
        );
        let mut args = payment_args_for_fixture(&fixture);
        args.idempotency_key = Some("idem_payment".to_owned());
        let payload =
            order_payment_payload_from_status(&args, &status_view).expect("payment payload");

        let view = order_payment_dry_run_view(&config, &args, &status_view, &payload);
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.dry_run, true);
        assert_eq!(
            view.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(
            view.agreement_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert_eq!(view.amount, Some(RadrootsCoreDecimal::from(12u32)));
        assert_eq!(view.currency, Some(RadrootsCoreCurrency::USD));
        assert_eq!(
            view.method,
            Some(RadrootsTradePaymentMethod::ManualTransfer)
        );
        assert_eq!(view.reference.as_deref(), Some("memo-1"));
        assert_eq!(view.paid_at, Some(1_777_666_000));
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 2);
        assert_eq!(view.decoded_count, 2);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem_payment"));
    }

    #[test]
    fn order_payment_preflight_rejects_selected_non_buyer_account() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event],
            },
        );
        let args = payment_args_for_fixture(&fixture);

        let view = order_payment_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
        )
        .expect("non buyer payment preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("selected account is not buyer")
        );
    }

    #[test]
    fn order_payment_preflight_rejects_amount_mismatch() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event],
            },
        );
        let mut args = payment_args_for_fixture(&fixture);
        args.amount = "11.99".to_owned();

        let view = order_payment_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("amount mismatch preflight");

        assert_eq!(view.state, "invalid");
        assert_eq!(view.amount, Some("11.99".parse().expect("decimal")));
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "payment_amount_mismatch");
    }

    #[test]
    fn order_payment_preflight_rejects_cancelled_order() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let cancel_event = signed_order_cancellation_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "changed plans",
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, cancel_event],
            },
        );
        let args = payment_args_for_fixture(&fixture);

        let view = order_payment_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("cancelled payment preflight");

        assert_eq!(view.state, "cancelled");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("was cancelled")
        );
    }

    #[test]
    fn order_payment_preflight_skips_existing_recorded_payment() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let payment_event_id = payment_event.id.to_string();
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let args = payment_args_for_fixture(&fixture);

        let view = order_payment_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("recorded payment preflight");

        assert_eq!(view.state, "recorded");
        assert_eq!(view.event_id.as_deref(), Some(payment_event_id.as_str()));
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already has payment state")
        );
    }

    #[test]
    fn order_payment_preflight_rejects_second_different_recorded_payment() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let mut args = payment_args_for_fixture(&fixture);
        args.reference = Some("different memo".to_owned());

        let view = order_payment_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("different payment preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already has a different unrejected payment")
        );
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "duplicate_payment_attempt");
    }

    #[test]
    fn order_status_from_receipt_ignores_recorded_payment_axis() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event.clone(),
                    payment_event,
                ],
            },
        );

        assert_eq!(view.state, "accepted");
        assert_eq!(view.fetched_count, 3);
        assert_eq!(view.decoded_count, 2);
        assert_eq!(view.skipped_count, 1);
        assert!(view.payment.is_none());
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_settlement_event_parts_bind_recorded_payment_terms() {
        let fixture = order_status_fixture();
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let payment_event_id = payment_event.id.to_string();
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event.clone(),
                    payment_event,
                ],
            },
        );
        let args = settlement_args_for_fixture(
            &fixture,
            payment_event_id.as_str(),
            OrderSettlementDecisionArg::Accept,
        );

        assert!(
            order_settlement_preflight_view_from_status(
                &config,
                &args,
                &status_view,
                fixture.seller_pubkey.as_str()
            )
            .is_none()
        );
        let payload =
            order_settlement_payload_from_status(&args, &status_view).expect("settlement payload");
        let parts = order_settlement_event_parts(&status_view, &payload).expect("settlement parts");
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();
        let context = active_trade_event_context_from_tags(
            RadrootsActiveTradeMessageType::TradeSettlementDecision,
            &parts.tags,
        )
        .expect("settlement context");

        assert_eq!(parts.kind, KIND_TRADE_SETTLEMENT_DECISION);
        assert_eq!(
            context.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            context.prev_event_id.as_deref(),
            Some(payment_event_id.as_str())
        );
        assert_eq!(payload.previous_event_id, payment_event_id);
        assert_eq!(payload.agreement_event_id, decision_event_id);
        assert_eq!(payload.payment_event_id, payload.previous_event_id);
        assert_eq!(payload.amount, RadrootsCoreDecimal::from(12u32));
        assert_eq!(payload.currency, RadrootsCoreCurrency::USD);
        assert_eq!(payload.decision, RadrootsTradeSettlementDecision::Accepted);
        assert_eq!(payload.reason, None);
    }

    #[test]
    fn order_settlement_dry_run_view_preserves_rejection_payload_without_event_id() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let payment_event_id = payment_event.id.to_string();
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let mut args = settlement_args_for_fixture(
            &fixture,
            payment_event_id.as_str(),
            OrderSettlementDecisionArg::Reject,
        );
        args.idempotency_key = Some("idem_settlement".to_owned());
        let payload =
            order_settlement_payload_from_status(&args, &status_view).expect("settlement payload");

        let view = order_settlement_dry_run_view(&config, &args, &status_view, &payload);

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.dry_run, true);
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(payment_event_id.as_str())
        );
        assert_eq!(
            view.payment_event_id.as_deref(),
            Some(payment_event_id.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert_eq!(view.amount, Some(RadrootsCoreDecimal::from(12u32)));
        assert_eq!(view.currency, Some(RadrootsCoreCurrency::USD));
        assert_eq!(
            view.decision,
            Some(RadrootsTradeSettlementDecision::Rejected)
        );
        assert_eq!(
            view.settlement_reason.as_deref(),
            Some("reference mismatch")
        );
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 3);
        assert_eq!(view.decoded_count, 3);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem_settlement"));
    }

    #[test]
    fn order_settlement_preflight_rejects_selected_non_seller_account() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let payment_event_id = payment_event.id.to_string();
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let args = settlement_args_for_fixture(
            &fixture,
            payment_event_id.as_str(),
            OrderSettlementDecisionArg::Accept,
        );

        let view = order_settlement_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("non seller settlement preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("selected account is not seller")
        );
    }

    #[test]
    fn order_settlement_preflight_rejects_stale_payment_event_id() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event, payment_event],
            },
        );
        let args = settlement_args_for_fixture(
            &fixture,
            "2".repeat(64).as_str(),
            OrderSettlementDecisionArg::Accept,
        );

        let view = order_settlement_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
        )
        .expect("stale settlement preflight");

        assert_eq!(view.state, "invalid");
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "stale_payment_event");
    }

    #[test]
    fn order_settlement_preflight_rejects_duplicate_decision() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let settlement_event = signed_settlement_decision_event(
            &fixture.seller,
            &fixture.request_event,
            &payment_event,
            RadrootsTradeSettlementDecision::Accepted,
        );
        let payment_event_id = payment_event.id.to_string();
        let status_view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    payment_event,
                    settlement_event,
                ],
            },
        );
        let args = settlement_args_for_fixture(
            &fixture,
            payment_event_id.as_str(),
            OrderSettlementDecisionArg::Accept,
        );

        let view = order_settlement_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
        )
        .expect("duplicate settlement preflight");

        assert_eq!(view.state, "already_decided");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already has settlement state")
        );
    }

    #[test]
    fn order_status_from_receipt_ignores_accepted_settlement_axis() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let settlement_event = signed_settlement_decision_event(
            &fixture.seller,
            &fixture.request_event,
            &payment_event,
            RadrootsTradeSettlementDecision::Accepted,
        );
        let view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    payment_event,
                    settlement_event,
                ],
            },
        );

        assert_eq!(view.state, "accepted");
        assert_eq!(view.fetched_count, 4);
        assert_eq!(view.decoded_count, 2);
        assert_eq!(view.skipped_count, 2);
        assert!(view.payment.is_none());
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn deferred_payment_status_helper_reports_rejected_settlement_axis() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let payment_event = signed_payment_recorded_event(
            &fixture.buyer,
            &fixture.request_event,
            &decision_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
        );
        let settlement_event = signed_settlement_decision_event(
            &fixture.seller,
            &fixture.request_event,
            &payment_event,
            RadrootsTradeSettlementDecision::Rejected,
        );
        let view = order_status_from_receipt_with_deferred_payment(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    payment_event,
                    settlement_event,
                ],
            },
        );
        let payment = view.payment.as_ref().expect("payment view");

        assert_eq!(payment.state, "rejected");
        assert_eq!(payment.settlement_state, "rejected");
        assert_eq!(payment.reason.as_deref(), Some("reference mismatch"));
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_receipt_preflight_rejects_ineligible_fulfillment() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Preparing,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event,
                ],
            },
        );
        let args = receipt_args_for_fixture(&fixture, true, None);

        let view = order_receipt_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("ineligible receipt preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("no eligible seller fulfillment")
        );
        assert!(view.event_id.is_none());
    }

    #[test]
    fn order_receipt_preflight_rejects_selected_non_buyer_account() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event,
                ],
            },
        );
        let args = receipt_args_for_fixture(&fixture, true, None);

        let view = order_receipt_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.seller_pubkey.as_str(),
        )
        .expect("non buyer receipt preflight");

        assert_eq!(view.state, "invalid");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("selected account is not buyer")
        );
        assert!(view.event_id.is_none());
    }

    #[test]
    fn order_receipt_preflight_rejects_existing_terminal_receipt() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let receipt_event = signed_buyer_receipt_event(
            &fixture.buyer,
            &fixture.request_event,
            &fulfillment_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            true,
            None,
        );
        let receipt_event_id = receipt_event.id.to_string();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event,
                    receipt_event,
                ],
            },
        );
        let args = receipt_args_for_fixture(&fixture, true, None);

        let view = order_receipt_preflight_view_from_status(
            &config,
            &args,
            &status_view,
            fixture.buyer_pubkey.as_str(),
        )
        .expect("terminal receipt preflight");

        assert_eq!(view.state, "terminal");
        assert_eq!(view.event_id.as_deref(), Some(receipt_event_id.as_str()));
        assert_eq!(view.event_kind, Some(KIND_TRADE_RECEIPT));
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already terminal")
        );
    }

    #[test]
    fn order_status_from_receipt_reports_latest_fulfillment_as_last_event() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                decision_event.clone(),
                fulfillment_event.clone(),
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let request_event_id = fixture.request_event.id.to_string();
        let decision_event_id = decision_event.id.to_string();
        let fulfillment_event_id = fulfillment_event.id.to_string();

        assert_eq!(
            u32::from(fulfillment_event.kind.as_u16()),
            KIND_TRADE_FULFILLMENT_UPDATE
        );
        assert_eq!(view.state, "accepted");
        assert_eq!(
            view.last_event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        let fulfillment = view.fulfillment.as_ref().expect("fulfillment view");
        assert_eq!(fulfillment.state, "ready_for_pickup");
        assert_eq!(
            fulfillment.event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(
            fulfillment.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            fulfillment.prev_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert_eq!(fulfillment.terminal, false);
        assert_eq!(fulfillment.inventory_released, false);
        assert!(fulfillment.issues.is_empty());
        assert_eq!(view.decoded_count, 3);
        assert!(view.reducer_issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_seller_cancelled_inventory_release_flag() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::SellerCancelled,
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                decision_event,
                fulfillment_event.clone(),
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let fulfillment_event_id = fulfillment_event.id.to_string();
        let fulfillment = view.fulfillment.as_ref().expect("fulfillment view");

        assert_eq!(view.state, "accepted");
        assert_eq!(fulfillment.state, "seller_cancelled");
        assert_eq!(
            fulfillment.event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(fulfillment.terminal, true);
        assert_eq!(fulfillment.inventory_released, true);
        assert!(fulfillment.issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_exposes_forked_fulfillment_issues() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let first_fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Preparing,
        );
        let second_fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let mut expected_event_ids = vec![
            first_fulfillment_event.id.to_string(),
            second_fulfillment_event.id.to_string(),
        ];
        expected_event_ids.sort();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                decision_event,
                first_fulfillment_event,
                second_fulfillment_event,
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let fulfillment = view.fulfillment.as_ref().expect("fulfillment view");

        assert_eq!(view.state, "invalid");
        assert_eq!(fulfillment.state, "invalid");
        assert_eq!(fulfillment.issues.len(), 1);
        assert_eq!(fulfillment.issues[0].code, "forked_fulfillments");
        assert_eq!(fulfillment.issues[0].event_ids, expected_event_ids);
    }

    #[test]
    fn order_fulfillment_dry_run_view_chains_from_latest_visible_event() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Preparing,
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event.clone(),
                ],
            },
        );
        let args = OrderFulfillmentArgs {
            key: fixture.order_id.clone(),
            state: "ready_for_pickup".to_owned(),
            idempotency_key: Some("idem_fulfillment".to_owned()),
        };

        let view = order_fulfillment_dry_run_view(
            &config,
            &args,
            &status_view,
            RadrootsActiveTradeFulfillmentState::ReadyForPickup,
        );
        let request_event_id = fixture.request_event.id.to_string();
        let fulfillment_event_id = fulfillment_event.id.to_string();

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.fulfillment_state, "ready_for_pickup");
        assert_eq!(
            view.root_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert_eq!(
            view.prev_event_id.as_deref(),
            Some(fulfillment_event_id.as_str())
        );
        assert_eq!(view.event_id, None);
        assert_eq!(view.event_kind, None);
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 3);
        assert_eq!(view.decoded_count, 3);
        assert_eq!(view.idempotency_key.as_deref(), Some("idem_fulfillment"));
    }

    #[test]
    fn order_fulfillment_preflight_rejects_terminal_fulfillment_state() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let fulfillment_event = signed_fulfillment_update_event(
            &fixture.seller,
            &fixture.request_event,
            &decision_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsActiveTradeFulfillmentState::Delivered,
        );
        let fulfillment_event_id = fulfillment_event.id.to_string();
        let reduction = order_status_reduction_from_receipt_with_context(
            OrderStatusContext {
                order_id: fixture.order_id.as_str(),
                selected_account_pubkey: None,
            },
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    decision_event,
                    fulfillment_event,
                ],
            },
        );
        let args = OrderFulfillmentArgs {
            key: fixture.order_id.clone(),
            state: "ready_for_pickup".to_owned(),
            idempotency_key: None,
        };

        let view = order_fulfillment_preflight_view_from_status(
            &config,
            &args,
            &reduction.view,
            reduction.fulfillment_status,
            reduction.fulfillment_event_id.as_deref(),
        )
        .expect("terminal fulfillment preflight");

        assert_eq!(view.state, "invalid");
        let fulfillment = reduction
            .view
            .fulfillment
            .as_ref()
            .expect("fulfillment view");
        assert_eq!(fulfillment.state, "delivered");
        assert_eq!(fulfillment.terminal, true);
        assert_eq!(fulfillment.inventory_released, false);
        assert_eq!(view.issues[0].code, "fulfillment_unsupported_transition");
        assert_eq!(view.issues[0].event_ids, vec![fulfillment_event_id]);
        assert!(view.event_id.is_none());
    }

    #[test]
    fn order_fulfillment_preflight_rejects_missing_order() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: Vec::new(),
            },
        );
        let args = fulfillment_args_for_fixture(&fixture, "ready_for_pickup");

        let view =
            order_fulfillment_preflight_view_from_status(&config, &args, &status_view, None, None)
                .expect("missing fulfillment preflight");

        assert_eq!(view.state, "missing");
        assert_eq!(view.event_id, None);
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("no active order events")
        );
        assert_eq!(
            view.actions,
            vec![format!("radroots order status get {}", fixture.order_id)]
        );
    }

    #[test]
    fn order_fulfillment_preflight_rejects_requested_order() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );
        let args = fulfillment_args_for_fixture(&fixture, "ready_for_pickup");

        let view =
            order_fulfillment_preflight_view_from_status(&config, &args, &status_view, None, None)
                .expect("requested fulfillment preflight");

        assert_eq!(view.state, "requested");
        let request_event_id = fixture.request_event.id.to_string();
        assert_eq!(
            view.request_event_id.as_deref(),
            Some(request_event_id.as_str())
        );
        assert!(view.event_id.is_none());
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("has no accepted seller decision")
        );
    }

    #[test]
    fn order_fulfillment_preflight_rejects_declined_order() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Declined {
                reason: "out of stock".to_owned(),
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event.clone()],
            },
        );
        let args = fulfillment_args_for_fixture(&fixture, "ready_for_pickup");

        let view =
            order_fulfillment_preflight_view_from_status(&config, &args, &status_view, None, None)
                .expect("declined fulfillment preflight");

        assert_eq!(view.state, "declined");
        let decision_event_id = decision_event.id.to_string();
        assert_eq!(
            view.decision_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        assert!(view.event_id.is_none());
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("was declined")
        );
    }

    #[test]
    fn order_fulfillment_preflight_rejects_invalid_order_state() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let accepted_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let declined_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Declined {
                reason: "out of stock".to_owned(),
            },
        );
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![
                    fixture.request_event.clone(),
                    accepted_event,
                    declined_event,
                ],
            },
        );
        let args = fulfillment_args_for_fixture(&fixture, "ready_for_pickup");

        let view =
            order_fulfillment_preflight_view_from_status(&config, &args, &status_view, None, None)
                .expect("invalid fulfillment preflight");

        assert_eq!(view.state, "invalid");
        assert!(view.event_id.is_none());
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "conflicting_decisions");
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("failed reducer validation")
        );
    }

    #[test]
    fn order_fulfillment_signing_rejects_selected_non_seller_account() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        accounts::create_or_migrate_default_account(&config).expect("create selected account");
        let fixture = order_status_fixture();

        let error = resolve_local_order_fulfillment_signing_identity(
            &config,
            fixture.seller_pubkey.as_str(),
        )
        .expect_err("non seller account rejected");

        let reason = error.reason();
        assert!(reason.contains("cannot sign order seller_pubkey"));
    }

    #[test]
    fn order_status_from_receipt_rejects_wrong_decision_counterparty() {
        let fixture = order_status_fixture();
        let wrong_buyer = RadrootsIdentity::generate();
        let decision_event = signed_order_decision_event_with_counterparty(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            wrong_buyer.public_key_hex().as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let decision_event_id = decision_event.id.to_string();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), decision_event],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);

        assert_eq!(view.state, "invalid");
        let issue = view
            .reducer_issues
            .iter()
            .find(|issue| issue.code == "decision_counterparty_mismatch")
            .expect("decision counterparty mismatch issue");
        assert_eq!(issue.field, "buyer_pubkey");
        assert_eq!(issue.event_ids, vec![decision_event_id]);
        let inventory = view.inventory.as_ref().expect("inventory view");
        assert_eq!(inventory.state, "invalid");
        assert_eq!(inventory.issues[0].code, "decision_counterparty_mismatch");
    }

    #[test]
    fn order_decision_preflight_rejects_existing_decision() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let resolution = request_resolution_for_fixture(&fixture);
        let request = resolution.requests[0].clone();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let decision_event_id = decision_event.id.to_string();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone(), decision_event],
            },
        );
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Decline,
            reason: Some("out of stock".to_owned()),
            idempotency_key: None,
        };

        let view = order_decision_preflight_view_from_status(
            &config,
            &args,
            &request,
            &resolution,
            &status_view,
        )
        .expect("existing decision preflight view");

        assert_eq!(view.state, "already_decided");
        assert_eq!(view.event_id.as_deref(), Some(decision_event_id.as_str()));
        assert_eq!(view.event_kind, Some(KIND_TRADE_ORDER_DECISION));
        assert_eq!(
            view.request_event_id.as_deref(),
            Some(request.request_event_id.as_str())
        );
        assert_eq!(view.target_relays, vec!["ws://relay.test"]);
        assert_eq!(view.connected_relays, vec!["ws://relay.test"]);
        assert_eq!(view.fetched_count, 2);
        assert_eq!(view.decoded_count, 2);
        assert!(
            view.reason
                .as_deref()
                .expect("reason")
                .contains("already has a visible `accepted` seller decision")
        );
    }

    #[test]
    fn order_accept_inventory_preflight_rejects_over_reserved_projection() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let resolution = request_resolution_for_fixture(&fixture);
        let request = resolution.requests[0].clone();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );
        let existing_order_id = "ord_AAAAAAAAAAAAAAAAAAAAAw";
        let existing_request_event = signed_order_request_event(
            &fixture.buyer,
            existing_order_id,
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            fixture.listing_event_id.as_str(),
        );
        let existing_request = ResolvedSellerOrderRequest {
            request_event_id: existing_request_event.id.to_string(),
            listing_event_id: Some(fixture.listing_event_id.clone()),
            order_id: existing_order_id.to_owned(),
            listing_addr: fixture.listing_addr.clone(),
            buyer_pubkey: fixture.buyer_pubkey.clone(),
            seller_pubkey: fixture.seller_pubkey.clone(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count: 2,
            }],
            economics: sample_order_economics(existing_order_id, "bin-1", 2),
        };
        let existing_decision_payload =
            accepted_order_decision_payload_from_request(&existing_request);
        let existing_decision_payload = canonicalize_active_order_decision_for_signer(
            existing_decision_payload,
            fixture.seller_pubkey.as_str(),
        )
        .expect("canonical existing decision");
        let projection = reduce_listing_inventory_accounting(
            fixture.listing_addr.as_str(),
            fixture.listing_event_id.as_str(),
            vec![RadrootsListingInventoryBinAvailability {
                bin_id: "bin-1".to_owned(),
                available_count: 2,
            }],
            vec![
                active_request_record_from_resolved(&existing_request),
                active_request_record_from_resolved(&request),
            ],
            vec![
                RadrootsActiveOrderDecisionRecord {
                    event_id: "existing_decision".to_owned(),
                    author_pubkey: fixture.seller_pubkey.clone(),
                    counterparty_pubkey: fixture.buyer_pubkey.clone(),
                    root_event_id: existing_request.request_event_id.clone(),
                    prev_event_id: existing_request.request_event_id.clone(),
                    payload: existing_decision_payload,
                },
                proposed_accept_decision_record(&request).expect("proposed accept decision"),
            ],
            Vec::<RadrootsActiveOrderRevisionProposalRecord>::new(),
            Vec::<RadrootsActiveOrderRevisionDecisionRecord>::new(),
            Vec::<RadrootsActiveOrderFulfillmentRecord>::new(),
            Vec::<RadrootsActiveOrderCancellationRecord>::new(),
            Vec::<RadrootsActiveOrderReceiptRecord>::new(),
        );
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Accept,
            reason: None,
            idempotency_key: None,
        };

        let view = order_accept_inventory_preflight_view_from_projection(
            &config,
            &args,
            &request,
            &resolution,
            &status_view,
            projection,
        )
        .invalid_view
        .expect("invalid inventory preflight view");

        assert_eq!(view.state, "invalid");
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "listing_inventory_over_reserved");
        assert!(view.event_id.is_none());
    }

    #[test]
    fn order_accept_inventory_preflight_counts_seller_cancelled_release() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let fixture = order_status_fixture();
        let resolution = request_resolution_for_fixture(&fixture);
        let request = resolution.requests[0].clone();
        let status_view = order_status_from_receipt(
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        );
        let existing_order_id = "ord_AAAAAAAAAAAAAAAAAAAAAw";
        let existing_request_event = signed_order_request_event(
            &fixture.buyer,
            existing_order_id,
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            fixture.listing_event_id.as_str(),
        );
        let existing_request = ResolvedSellerOrderRequest {
            request_event_id: existing_request_event.id.to_string(),
            listing_event_id: Some(fixture.listing_event_id.clone()),
            order_id: existing_order_id.to_owned(),
            listing_addr: fixture.listing_addr.clone(),
            buyer_pubkey: fixture.buyer_pubkey.clone(),
            seller_pubkey: fixture.seller_pubkey.clone(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count: 2,
            }],
            economics: sample_order_economics(existing_order_id, "bin-1", 2),
        };
        let existing_decision_payload =
            accepted_order_decision_payload_from_request(&existing_request);
        let existing_decision_payload = canonicalize_active_order_decision_for_signer(
            existing_decision_payload,
            fixture.seller_pubkey.as_str(),
        )
        .expect("canonical existing decision");
        let existing_decision_event_id = "existing_decision".to_owned();
        let projection = reduce_listing_inventory_accounting(
            fixture.listing_addr.as_str(),
            fixture.listing_event_id.as_str(),
            vec![RadrootsListingInventoryBinAvailability {
                bin_id: "bin-1".to_owned(),
                available_count: 2,
            }],
            vec![
                active_request_record_from_resolved(&existing_request),
                active_request_record_from_resolved(&request),
            ],
            vec![
                RadrootsActiveOrderDecisionRecord {
                    event_id: existing_decision_event_id.clone(),
                    author_pubkey: fixture.seller_pubkey.clone(),
                    counterparty_pubkey: fixture.buyer_pubkey.clone(),
                    root_event_id: existing_request.request_event_id.clone(),
                    prev_event_id: existing_request.request_event_id.clone(),
                    payload: existing_decision_payload,
                },
                proposed_accept_decision_record(&request).expect("proposed accept decision"),
            ],
            Vec::<RadrootsActiveOrderRevisionProposalRecord>::new(),
            Vec::<RadrootsActiveOrderRevisionDecisionRecord>::new(),
            vec![RadrootsActiveOrderFulfillmentRecord {
                event_id: "existing_fulfillment".to_owned(),
                author_pubkey: fixture.seller_pubkey.clone(),
                counterparty_pubkey: fixture.buyer_pubkey.clone(),
                root_event_id: existing_request.request_event_id.clone(),
                prev_event_id: existing_decision_event_id,
                payload: RadrootsTradeFulfillmentUpdated {
                    order_id: existing_request.order_id.clone(),
                    listing_addr: existing_request.listing_addr.clone(),
                    buyer_pubkey: existing_request.buyer_pubkey.clone(),
                    seller_pubkey: existing_request.seller_pubkey.clone(),
                    status: RadrootsActiveTradeFulfillmentState::SellerCancelled,
                },
            }],
            Vec::<RadrootsActiveOrderCancellationRecord>::new(),
            Vec::<RadrootsActiveOrderReceiptRecord>::new(),
        );
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Accept,
            reason: None,
            idempotency_key: None,
        };

        let preflight = order_accept_inventory_preflight_view_from_projection(
            &config,
            &args,
            &request,
            &resolution,
            &status_view,
            projection,
        );
        let inventory = preflight.inventory.expect("valid inventory preflight");

        assert!(preflight.invalid_view.is_none());
        assert_eq!(inventory.state, "reserved");
        assert_eq!(inventory.commitment_valid, true);
        assert_eq!(inventory.bins.len(), 1);
        assert_eq!(inventory.bins[0].committed_count, 2);
        assert_eq!(inventory.bins[0].remaining_count, Some(0));
        assert!(inventory.issues.is_empty());
    }

    #[test]
    fn order_status_from_receipt_reports_mismatched_commitment_inventory_invalid() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 1,
                }],
            },
        );
        let decision_event_id = decision_event.id.to_string();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), decision_event],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);

        assert_eq!(view.state, "invalid");
        let issue = view
            .reducer_issues
            .iter()
            .find(|issue| issue.code == "decision_inventory_commitment_mismatch")
            .expect("commitment mismatch issue");
        assert_eq!(issue.event_ids, vec![decision_event_id]);
        let inventory = view.inventory.as_ref().expect("inventory view");
        assert_eq!(inventory.state, "invalid");
        assert_eq!(inventory.commitment_valid, false);
        assert_eq!(
            inventory.issues[0].code,
            "decision_inventory_commitment_mismatch"
        );
    }

    #[test]
    fn order_status_from_receipt_reports_declined() {
        let fixture = order_status_fixture();
        let decision_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Declined {
                reason: "out of stock".to_owned(),
            },
        );
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), decision_event.clone()],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);
        let decision_event_id = decision_event.id.to_string();

        assert_eq!(view.state, "declined");
        assert_eq!(
            view.decision_event_id.as_deref(),
            Some(decision_event_id.as_str())
        );
        let inventory = view.inventory.as_ref().expect("inventory view");
        assert_eq!(inventory.state, "not_reserved");
        assert_eq!(inventory.commitment_valid, true);
        assert!(inventory.bins.is_empty());
        assert!(view.economics.is_none());
        assert!(view.fulfillment.is_none());
        assert!(view.reducer_issues.is_empty());
        assert_eq!(view.decoded_count, 2);
    }

    #[test]
    fn order_status_from_receipt_reports_conflicting_decisions_invalid() {
        let fixture = order_status_fixture();
        let accepted_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Accepted {
                inventory_commitments: vec![RadrootsTradeInventoryCommitment {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
        );
        let declined_event = signed_order_decision_event(
            &fixture.seller,
            &fixture.request_event,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            RadrootsTradeOrderDecision::Declined {
                reason: "out of stock".to_owned(),
            },
        );
        let mut expected_event_ids =
            vec![accepted_event.id.to_string(), declined_event.id.to_string()];
        expected_event_ids.sort();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![
                fixture.request_event.clone(),
                accepted_event,
                declined_event,
            ],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);

        assert_eq!(view.state, "invalid");
        assert_eq!(view.decoded_count, 3);
        assert!(view.decision_event_id.is_none());
        let issue = view
            .reducer_issues
            .iter()
            .find(|issue| issue.code == "conflicting_decisions")
            .expect("conflicting decision issue");
        assert_eq!(issue.field, "decision_event_id");
        assert_eq!(
            issue.message,
            "active order reducer reported conflicting decisions"
        );
        assert_eq!(issue.event_ids, expected_event_ids);
    }

    #[test]
    fn order_status_from_receipt_reports_invalid_same_order_request_candidate() {
        let fixture = order_status_fixture();
        let invalid_event = signed_malformed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let invalid_event_id = invalid_event.id.to_string();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), invalid_event],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);

        assert_eq!(view.state, "invalid");
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 1);
        assert_eq!(
            view.reason.as_deref(),
            Some(
                "active order request candidates for `ord_AAAAAAAAAAAAAAAAAAAAAg` failed status validation"
            )
        );
        let issue = view
            .reducer_issues
            .iter()
            .find(|issue| issue.code == "invalid_request_candidate")
            .expect("invalid request candidate issue");
        assert_eq!(issue.field, "request_event_id");
        assert_eq!(issue.event_ids, vec![invalid_event_id]);
    }

    #[test]
    fn order_status_from_receipt_reports_multiple_request_candidates_invalid() {
        let fixture = order_status_fixture();
        let second_request_event = signed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let mut expected_event_ids = vec![
            fixture.request_event.id.to_string(),
            second_request_event.id.to_string(),
        ];
        expected_event_ids.sort();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), second_request_event],
        };

        let view = order_status_from_receipt(fixture.order_id.as_str(), receipt);

        assert_eq!(view.state, "invalid");
        assert_eq!(view.decoded_count, 2);
        assert_eq!(view.skipped_count, 0);
        let issue = view
            .reducer_issues
            .iter()
            .find(|issue| issue.code == "multiple_requests")
            .expect("multiple request issue");
        assert_eq!(issue.field, "request_event_id");
        assert_eq!(issue.event_ids, expected_event_ids);
    }

    #[test]
    fn seller_order_request_resolution_skips_wrong_seller_request() {
        let selected_seller = RadrootsIdentity::generate();
        let other_seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let selected_seller_pubkey = selected_seller.public_key_hex();
        let other_seller_pubkey = other_seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{other_seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![signed_order_request_event(
                &buyer,
                order_id,
                listing_addr.as_str(),
                buyer_pubkey.as_str(),
                other_seller_pubkey.as_str(),
                listing_event_id.as_str(),
            )],
        };

        let resolution = seller_order_request_resolution_from_receipt(
            selected_seller_pubkey.as_str(),
            order_id,
            receipt,
        )
        .expect("seller order request resolution");

        assert_eq!(resolution.fetched_count, 1);
        assert_eq!(resolution.decoded_count, 0);
        assert_eq!(resolution.skipped_count, 1);
        assert!(resolution.requests.is_empty());
    }

    #[test]
    fn seller_order_request_resolution_skips_listing_outside_seller_authority() {
        let seller = RadrootsIdentity::generate();
        let listing_seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let listing_seller_pubkey = listing_seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg";
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{listing_seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![signed_order_request_event(
                &buyer,
                order_id,
                listing_addr.as_str(),
                buyer_pubkey.as_str(),
                seller_pubkey.as_str(),
                listing_event_id.as_str(),
            )],
        };

        let resolution =
            seller_order_request_resolution_from_receipt(seller_pubkey.as_str(), order_id, receipt)
                .expect("seller order request resolution");

        assert_eq!(resolution.fetched_count, 1);
        assert_eq!(resolution.decoded_count, 0);
        assert_eq!(resolution.skipped_count, 1);
        assert!(resolution.requests.is_empty());
    }

    #[test]
    fn seller_order_request_resolution_reports_invalid_same_order_candidate() {
        let fixture = order_status_fixture();
        let invalid_event = signed_malformed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let invalid_event_id = invalid_event.id.to_string();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), invalid_event],
        };

        let resolution = seller_order_request_resolution_from_receipt(
            fixture.seller_pubkey.as_str(),
            fixture.order_id.as_str(),
            receipt,
        )
        .expect("seller order request resolution");

        assert_eq!(resolution.fetched_count, 2);
        assert_eq!(resolution.decoded_count, 1);
        assert_eq!(resolution.skipped_count, 1);
        assert_eq!(resolution.requests.len(), 1);
        assert_eq!(resolution.candidate_issues.len(), 1);
        assert_eq!(
            resolution.candidate_issues[0].code,
            "invalid_request_candidate"
        );
        assert_eq!(
            resolution.candidate_issues[0].event_ids,
            vec![invalid_event_id.clone()]
        );

        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Accept,
            reason: None,
            idempotency_key: None,
        };
        let view = order_decision_view_from_resolution(
            &config,
            &args,
            fixture.seller_pubkey.clone(),
            resolution,
        );

        assert_eq!(view.state, "invalid");
        assert_eq!(view.issues[0].code, "invalid_request_candidate");
        assert_eq!(view.issues[0].event_ids, vec![invalid_event_id]);
        assert!(view.event_id.is_none());
    }

    #[test]
    fn seller_order_request_resolution_reports_multiple_same_order_candidates_invalid() {
        let fixture = order_status_fixture();
        let second_request_event = signed_order_request_event(
            &fixture.buyer,
            fixture.order_id.as_str(),
            fixture.listing_addr.as_str(),
            fixture.buyer_pubkey.as_str(),
            fixture.seller_pubkey.as_str(),
            "2".repeat(64).as_str(),
        );
        let mut expected_event_ids = vec![
            fixture.request_event.id.to_string(),
            second_request_event.id.to_string(),
        ];
        expected_event_ids.sort();
        let receipt = DirectRelayFetchReceipt {
            target_relays: vec!["ws://relay.test".to_owned()],
            connected_relays: vec!["ws://relay.test".to_owned()],
            failed_relays: Vec::new(),
            events: vec![fixture.request_event.clone(), second_request_event],
        };

        let resolution = seller_order_request_resolution_from_receipt(
            fixture.seller_pubkey.as_str(),
            fixture.order_id.as_str(),
            receipt,
        )
        .expect("seller order request resolution");

        assert_eq!(resolution.fetched_count, 2);
        assert_eq!(resolution.decoded_count, 2);
        assert_eq!(resolution.skipped_count, 0);
        assert_eq!(resolution.requests.len(), 2);

        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path());
        config.output.dry_run = true;
        config.relay.urls = vec!["ws://relay.test".to_owned()];
        let args = OrderDecisionArgs {
            key: fixture.order_id.clone(),
            decision: OrderDecisionArg::Accept,
            reason: None,
            idempotency_key: None,
        };
        let view = order_decision_view_from_resolution(
            &config,
            &args,
            fixture.seller_pubkey.clone(),
            resolution,
        );

        assert_eq!(view.state, "invalid");
        assert_eq!(view.issues.len(), 1);
        assert_eq!(view.issues[0].code, "multiple_request_candidates");
        assert_eq!(view.issues[0].field, "request_event_id");
        assert_eq!(view.issues[0].event_ids, expected_event_ids);
        assert!(view.event_id.is_none());
    }

    struct OrderStatusFixture {
        buyer: RadrootsIdentity,
        seller: RadrootsIdentity,
        order_id: String,
        listing_addr: String,
        listing_event_id: String,
        buyer_pubkey: String,
        seller_pubkey: String,
        request_event: radroots_nostr::prelude::RadrootsNostrEvent,
    }

    fn order_status_fixture() -> OrderStatusFixture {
        let seller = RadrootsIdentity::generate();
        let buyer = RadrootsIdentity::generate();
        let seller_pubkey = seller.public_key_hex();
        let buyer_pubkey = buyer.public_key_hex();
        let order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg".to_owned();
        let listing_event_id = "1".repeat(64);
        let listing_addr = format!("30402:{seller_pubkey}:AAAAAAAAAAAAAAAAAAAAAg");
        let request_event = signed_order_request_event(
            &buyer,
            order_id.as_str(),
            listing_addr.as_str(),
            buyer_pubkey.as_str(),
            seller_pubkey.as_str(),
            listing_event_id.as_str(),
        );

        OrderStatusFixture {
            buyer,
            seller,
            order_id,
            listing_addr,
            listing_event_id,
            buyer_pubkey,
            seller_pubkey,
            request_event,
        }
    }

    fn sample_order_economics(
        order_id: &str,
        bin_id: &str,
        bin_count: u32,
    ) -> RadrootsTradeOrderEconomics {
        sample_order_economics_with_unit_price(order_id, bin_id, bin_count, 6)
    }

    fn sample_order_economics_with_unit_price(
        order_id: &str,
        bin_id: &str,
        bin_count: u32,
        unit_price: u32,
    ) -> RadrootsTradeOrderEconomics {
        let currency = RadrootsCoreCurrency::USD;
        let unit_price_amount = RadrootsCoreDecimal::from(unit_price);
        let line_amount = unit_price_amount * RadrootsCoreDecimal::from(bin_count);
        RadrootsTradeOrderEconomics {
            quote_id: format!("quote_{order_id}"),
            quote_version: 1,
            pricing_basis: RadrootsTradePricingBasis::ListingEvent,
            currency,
            items: vec![RadrootsTradeOrderEconomicItem {
                bin_id: bin_id.to_owned(),
                bin_count,
                quantity_amount: RadrootsCoreDecimal::ONE,
                quantity_unit: RadrootsCoreUnit::Each,
                unit_price_amount,
                unit_price_currency: currency,
                line_subtotal: RadrootsCoreMoney::new(line_amount, currency),
            }],
            discounts: Vec::new(),
            adjustments: Vec::new(),
            subtotal: RadrootsCoreMoney::new(line_amount, currency),
            discount_total: RadrootsCoreMoney::zero(currency),
            adjustment_total: RadrootsCoreMoney::zero(currency),
            total: RadrootsCoreMoney::new(line_amount, currency),
        }
    }

    fn loaded_order_draft_for_fixture(fixture: &OrderStatusFixture) -> LoadedOrderDraft {
        LoadedOrderDraft {
            file: PathBuf::from(format!("{}.toml", fixture.order_id)),
            updated_at_unix: 0,
            document: OrderDraftDocument {
                version: 1,
                kind: ORDER_DRAFT_KIND.to_owned(),
                order: OrderDraft {
                    order_id: fixture.order_id.clone(),
                    listing_addr: fixture.listing_addr.clone(),
                    listing_event_id: fixture.listing_event_id.clone(),
                    buyer_pubkey: fixture.buyer_pubkey.clone(),
                    seller_pubkey: fixture.seller_pubkey.clone(),
                    items: vec![OrderDraftItem {
                        bin_id: "bin-1".to_owned(),
                        bin_count: 2,
                    }],
                    economics: Some(sample_order_economics(
                        fixture.order_id.as_str(),
                        "bin-1",
                        2,
                    )),
                },
                listing_lookup: Some("test-listing".to_owned()),
                buyer_account_id: Some("acct_test".to_owned()),
            },
        }
    }

    fn request_resolution_for_fixture(
        fixture: &OrderStatusFixture,
    ) -> SellerOrderRequestResolution {
        seller_order_request_resolution_from_receipt(
            fixture.seller_pubkey.as_str(),
            fixture.order_id.as_str(),
            DirectRelayFetchReceipt {
                target_relays: vec!["ws://relay.test".to_owned()],
                connected_relays: vec!["ws://relay.test".to_owned()],
                failed_relays: Vec::new(),
                events: vec![fixture.request_event.clone()],
            },
        )
        .expect("seller order request resolution")
    }

    fn fulfillment_args_for_fixture(
        fixture: &OrderStatusFixture,
        state: &str,
    ) -> OrderFulfillmentArgs {
        OrderFulfillmentArgs {
            key: fixture.order_id.clone(),
            state: state.to_owned(),
            idempotency_key: None,
        }
    }

    fn cancel_args_for_fixture(fixture: &OrderStatusFixture, reason: &str) -> OrderCancelArgs {
        OrderCancelArgs {
            key: fixture.order_id.clone(),
            reason: reason.to_owned(),
            idempotency_key: None,
        }
    }

    fn receipt_args_for_fixture(
        fixture: &OrderStatusFixture,
        received: bool,
        issue: Option<&str>,
    ) -> OrderReceiptArgs {
        OrderReceiptArgs {
            key: fixture.order_id.clone(),
            received,
            issue: issue.map(str::to_owned),
            idempotency_key: None,
        }
    }

    fn payment_args_for_fixture(fixture: &OrderStatusFixture) -> OrderPaymentArgs {
        OrderPaymentArgs {
            key: fixture.order_id.clone(),
            amount: "12".to_owned(),
            currency: "USD".to_owned(),
            method: "manual_transfer".to_owned(),
            reference: Some("memo-1".to_owned()),
            paid_at: Some(1_777_666_000),
            idempotency_key: None,
        }
    }

    fn settlement_args_for_fixture(
        fixture: &OrderStatusFixture,
        payment_event_id: &str,
        decision: OrderSettlementDecisionArg,
    ) -> OrderSettlementArgs {
        OrderSettlementArgs {
            key: fixture.order_id.clone(),
            payment_event_id: payment_event_id.to_owned(),
            decision,
            reason: if decision == OrderSettlementDecisionArg::Reject {
                Some("reference mismatch".to_owned())
            } else {
                None
            },
            idempotency_key: None,
        }
    }

    fn revision_args_for_fixture(
        fixture: &OrderStatusFixture,
        bin_count: u32,
    ) -> OrderRevisionProposeArgs {
        OrderRevisionProposeArgs {
            key: fixture.order_id.clone(),
            reason: "update count".to_owned(),
            bin_id: Some("bin-1".to_owned()),
            bin_count: Some(bin_count),
            adjustment_id: None,
            adjustment_effect: None,
            adjustment_amount: None,
            adjustment_currency: None,
            adjustment_reason: None,
            idempotency_key: None,
        }
    }

    fn revision_decision_args_for_fixture(
        fixture: &OrderStatusFixture,
        revision_id: &str,
        decision: OrderRevisionDecisionArg,
    ) -> OrderRevisionDecisionArgs {
        OrderRevisionDecisionArgs {
            key: fixture.order_id.clone(),
            revision_id: revision_id.to_owned(),
            decision,
            reason: if decision == OrderRevisionDecisionArg::Decline {
                Some("keep original order".to_owned())
            } else {
                None
            },
            idempotency_key: None,
        }
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
            publish: PublishConfig {
                mode: PublishMode::NostrRelay,
                source: PublishModeSource::Defaults,
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

    fn signed_order_decision_event(
        seller: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        decision: RadrootsTradeOrderDecision,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        signed_order_decision_event_with_counterparty(
            seller,
            request_event,
            order_id,
            listing_addr,
            buyer_pubkey,
            seller_pubkey,
            buyer_pubkey,
            decision,
        )
    }

    fn signed_order_decision_event_with_counterparty(
        seller: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        counterparty_pubkey: &str,
        decision: RadrootsTradeOrderDecision,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payload = RadrootsTradeOrderDecisionEvent {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            decision,
        };
        let payload = canonicalize_active_order_decision_for_signer(payload, seller_pubkey)
            .expect("canonical order decision");
        let request_event_id = request_event.id.to_string();
        let parts = active_trade_order_decision_event_build(
            request_event_id.as_str(),
            request_event_id.as_str(),
            &payload,
        )
        .expect("order decision parts");
        let mut tags = parts.tags;
        for tag in tags.iter_mut() {
            if tag.first().map(String::as_str) == Some("p") && tag.len() > 1 {
                tag[1] = counterparty_pubkey.to_owned();
            }
        }
        radroots_nostr_build_event(parts.kind, parts.content, tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed order decision")
    }

    fn signed_order_revision_proposal_event(
        seller: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        decision_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        bin_count: u32,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let mut economics = sample_order_economics(order_id, "bin-1", bin_count);
        economics.quote_id = "revision_rev_test".to_owned();
        economics.quote_version = 2;
        economics.canonicalize();
        let payload = RadrootsTradeOrderRevisionProposed {
            revision_id: "rev_test".to_owned(),
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            root_event_id: request_event.id.to_string(),
            prev_event_id: decision_event.id.to_string(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count,
            }],
            economics,
            reason: "update count".to_owned(),
        };
        let parts = active_trade_order_revision_proposal_event_build(
            payload.root_event_id.as_str(),
            payload.prev_event_id.as_str(),
            &payload,
        )
        .expect("revision proposal parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed order revision proposal")
    }

    fn signed_order_revision_decision_event(
        buyer: &RadrootsIdentity,
        proposal_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        decision: RadrootsTradeOrderRevisionDecision,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let proposal = radroots_event_from_nostr(proposal_event);
        let envelope =
            radroots_events_codec::trade::active_trade_order_revision_proposal_from_event(
                &proposal,
            )
            .expect("decoded revision proposal");
        let payload = RadrootsTradeOrderRevisionDecisionEvent {
            revision_id: envelope.payload.revision_id.clone(),
            order_id: envelope.payload.order_id.clone(),
            listing_addr: envelope.payload.listing_addr.clone(),
            buyer_pubkey: envelope.payload.buyer_pubkey.clone(),
            seller_pubkey: envelope.payload.seller_pubkey.clone(),
            root_event_id: envelope.payload.root_event_id.clone(),
            prev_event_id: proposal_event.id.to_string(),
            decision,
        };
        let parts = active_trade_order_revision_decision_event_build(
            payload.root_event_id.as_str(),
            payload.prev_event_id.as_str(),
            &payload,
        )
        .expect("revision decision parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed order revision decision")
    }

    fn signed_fulfillment_update_event(
        seller: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        prev_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        status: RadrootsActiveTradeFulfillmentState,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payload = RadrootsTradeFulfillmentUpdated {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            status,
        };
        let request_event_id = request_event.id.to_string();
        let prev_event_id = prev_event.id.to_string();
        let parts = active_trade_fulfillment_update_event_build(
            request_event_id.as_str(),
            prev_event_id.as_str(),
            &payload,
        )
        .expect("fulfillment update parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed fulfillment update")
    }

    fn signed_order_cancellation_event(
        buyer: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        prev_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        reason: &str,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payload = RadrootsTradeOrderCancelled {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            reason: reason.to_owned(),
        };
        let request_event_id = request_event.id.to_string();
        let prev_event_id = prev_event.id.to_string();
        let parts = active_trade_order_cancel_event_build(
            request_event_id.as_str(),
            prev_event_id.as_str(),
            &payload,
        )
        .expect("order cancellation parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed order cancellation")
    }

    fn signed_buyer_receipt_event(
        buyer: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        prev_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        received: bool,
        issue: Option<&str>,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payload = RadrootsTradeBuyerReceipt {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            received,
            issue: issue.map(str::to_owned),
            received_at: 1_777_665_600,
        };
        let request_event_id = request_event.id.to_string();
        let prev_event_id = prev_event.id.to_string();
        let parts = active_trade_buyer_receipt_event_build(
            request_event_id.as_str(),
            prev_event_id.as_str(),
            &payload,
        )
        .expect("buyer receipt parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed buyer receipt")
    }

    fn signed_payment_recorded_event(
        buyer: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        prev_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        agreement_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let economics = sample_order_economics(order_id, "bin-1", 2);
        let payload = RadrootsTradePaymentRecorded {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            root_event_id: request_event.id.to_string(),
            previous_event_id: prev_event.id.to_string(),
            agreement_event_id: agreement_event.id.to_string(),
            quote_id: economics.quote_id.clone(),
            quote_version: economics.quote_version,
            economics_digest: radroots_trade::order::radroots_trade_order_economics_digest(
                &economics,
            )
            .expect("economics digest"),
            amount: economics.total.amount,
            currency: economics.total.currency,
            method: RadrootsTradePaymentMethod::ManualTransfer,
            reference: Some("memo-1".to_owned()),
            paid_at: Some(1_777_666_000),
        };
        let parts = active_trade_payment_recorded_event_build(
            payload.root_event_id.as_str(),
            payload.previous_event_id.as_str(),
            &payload,
        )
        .expect("payment recorded parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed payment recorded")
    }

    fn signed_settlement_decision_event(
        seller: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        payment_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        decision: RadrootsTradeSettlementDecision,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payment = radroots_event_from_nostr(payment_event);
        let envelope =
            radroots_events_codec::trade::active_trade_payment_recorded_from_event(&payment)
                .expect("decoded payment");
        let payload = RadrootsTradeSettlementDecisionEvent {
            order_id: envelope.payload.order_id.clone(),
            listing_addr: envelope.payload.listing_addr.clone(),
            seller_pubkey: envelope.payload.seller_pubkey.clone(),
            buyer_pubkey: envelope.payload.buyer_pubkey.clone(),
            root_event_id: request_event.id.to_string(),
            previous_event_id: payment_event.id.to_string(),
            agreement_event_id: envelope.payload.agreement_event_id.clone(),
            payment_event_id: payment_event.id.to_string(),
            quote_id: envelope.payload.quote_id.clone(),
            quote_version: envelope.payload.quote_version,
            economics_digest: envelope.payload.economics_digest.clone(),
            amount: envelope.payload.amount,
            currency: envelope.payload.currency,
            decision,
            reason: (decision == RadrootsTradeSettlementDecision::Rejected)
                .then(|| "reference mismatch".to_owned()),
        };
        let parts = active_trade_settlement_decision_event_build(
            payload.root_event_id.as_str(),
            payload.previous_event_id.as_str(),
            &payload,
        )
        .expect("settlement decision parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed settlement decision")
    }

    fn signed_malformed_order_request_event(
        buyer: &RadrootsIdentity,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        listing_event_id: &str,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payload = RadrootsTradeOrderRequested {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count: 2,
            }],
            economics: sample_order_economics(order_id, "bin-1", 2),
        };
        let parts = active_trade_order_request_event_build(
            &RadrootsNostrEventPtr {
                id: listing_event_id.to_owned(),
                relays: None,
            },
            &payload,
        )
        .expect("order request parts");
        radroots_nostr_build_event(parts.kind, "not-json".to_owned(), parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed malformed order request")
    }

    fn signed_order_request_event(
        buyer: &RadrootsIdentity,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        listing_event_id: &str,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        signed_order_request_event_with_economics(
            buyer,
            order_id,
            listing_addr,
            buyer_pubkey,
            seller_pubkey,
            listing_event_id,
            sample_order_economics(order_id, "bin-1", 2),
        )
    }

    fn signed_order_request_event_with_economics(
        buyer: &RadrootsIdentity,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
        listing_event_id: &str,
        economics: RadrootsTradeOrderEconomics,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let payload = RadrootsTradeOrderRequested {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count: 2,
            }],
            economics,
        };
        let parts = active_trade_order_request_event_build(
            &RadrootsNostrEventPtr {
                id: listing_event_id.to_owned(),
                relays: None,
            },
            &payload,
        )
        .expect("order request parts");
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(buyer.keys())
            .expect("signed order request")
    }
}
