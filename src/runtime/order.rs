use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_events::RadrootsNostrEventPtr;
use radroots_events::kinds::{KIND_LISTING, KIND_TRADE_ORDER_DECISION, KIND_TRADE_ORDER_REQUEST};
use radroots_events::listing::{
    RadrootsListing, RadrootsListingAvailability, RadrootsListingStatus,
};
use radroots_events::trade::{
    RadrootsActiveTradeMessageType, RadrootsTradeInventoryCommitment, RadrootsTradeOrderDecision,
    RadrootsTradeOrderDecisionEvent, RadrootsTradeOrderItem, RadrootsTradeOrderRequested,
};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::listing::decode::listing_from_event;
use radroots_events_codec::trade::{
    RadrootsTradeListingAddress, active_trade_envelope_from_event,
    active_trade_event_context_from_tags, active_trade_order_decision_event_build,
    active_trade_order_request_event_build, active_trade_order_request_from_event,
};
use radroots_nostr::prelude::{
    RadrootsNostrEvent, RadrootsNostrFilter, radroots_event_from_nostr, radroots_nostr_filter_tag,
    radroots_nostr_kind,
};
use radroots_replica_db::{ReplicaSql, nostr_event_state, trade_product};
use radroots_replica_db_schema::nostr_event_state::{
    INostrEventStateFindOne, INostrEventStateFindOneArgs, NostrEventStateQueryBindValues,
};
use radroots_replica_db_schema::trade_product::{ITradeProductFieldsFilter, ITradeProductFindMany};
use radroots_sql_core::SqliteExecutor;
use radroots_trade::order::{
    RadrootsActiveOrderDecisionRecord, RadrootsActiveOrderReducerIssue,
    RadrootsActiveOrderRequestRecord, RadrootsActiveOrderStatus,
    RadrootsListingInventoryAccountingIssue, RadrootsListingInventoryBinAvailability,
    canonicalize_active_order_decision_for_signer, canonicalize_active_order_request_for_signer,
    reduce_active_order_events, reduce_listing_inventory_accounting,
};
use serde::{Deserialize, Serialize};

use crate::domain::runtime::{
    OrderDecisionView, OrderDraftItemView, OrderGetView, OrderHistoryEntryView, OrderHistoryView,
    OrderInventoryBinView, OrderInventoryView, OrderIssueView, OrderListView, OrderNewView,
    OrderStatusView, OrderSubmitView, OrderSummaryView, OrderWatchView, RelayFailureView,
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
    OrderDecisionArg, OrderDecisionArgs, OrderDraftCreateArgs, OrderStatusArgs, OrderSubmitArgs,
    OrderWatchArgs, RecordLookupArgs,
};

const ORDER_DRAFT_KIND: &str = "order_draft_v1";
const ORDER_SOURCE: &str = "local order drafts · local first";
const ORDER_SUBMIT_SOURCE: &str = "direct Nostr relay publish · local key";
const ORDER_DECISION_SOURCE: &str = "direct Nostr relay decision publish · local key";
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

    if config.output.dry_run {
        if let Err(error) = resolve_local_order_signing_identity(
            config,
            loaded.document.order.buyer_pubkey.as_str(),
        ) {
            return Ok(order_binding_error_view(config, &loaded, args, error));
        }
        return Ok(OrderSubmitView {
            state: "dry_run".to_owned(),
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
            dry_run: true,
            deduplicated: false,
            target_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: None,
            reason: Some("dry run requested; relay order publication skipped".to_owned()),
            job: None,
            issues: Vec::new(),
            actions: vec![format!(
                "radroots order submit {}",
                loaded.document.order.order_id
            )],
        });
    }

    if let Some(view) = order_submit_listing_freshness_view(config, &loaded, args)? {
        return Ok(view);
    }
    if let Some(view) = order_submit_quantity_preflight_view(config, &loaded, args)? {
        return Ok(view);
    }

    if config.relay.urls.is_empty() {
        return Err(RuntimeError::Network(
            "order submit requires at least one configured relay before signing".to_owned(),
        ));
    }

    let signing = match resolve_local_order_signing_identity(
        config,
        loaded.document.order.buyer_pubkey.as_str(),
    ) {
        Ok(signing) => signing,
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

    if let Some(view) =
        order_submit_existing_request_preflight_view(config, &loaded, args, &payload)?
    {
        return Ok(view);
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
        if let Some(view) = order_accept_inventory_preflight_view(
            config,
            args,
            &request,
            &resolution,
            &status_view,
        )? {
            return Ok(view);
        }
        let signing = match resolve_local_order_decision_signing_identity(
            config,
            request.seller_pubkey.as_str(),
            args.decision,
        ) {
            Ok(signing) => signing,
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
            ));
        }
        return publish_order_decision(config, args, request, resolution, signing, payload);
    }
    Ok(order_decision_view_from_resolution(
        config,
        args,
        seller_pubkey,
        resolution,
    ))
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
            listing_event_id: None,
            listing_addr: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            last_event_id: None,
            inventory: None,
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
                listing_event_id: None,
                listing_addr: None,
                buyer_pubkey: None,
                seller_pubkey: None,
                last_event_id: None,
                inventory: None,
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

    Ok(order_status_from_receipt(args.key.as_str(), receipt))
}

enum OrderStatusRecord {
    Request {
        listing_event_id: Option<String>,
        record: RadrootsActiveOrderRequestRecord,
    },
    Decision(RadrootsActiveOrderDecisionRecord),
}

#[derive(Debug, Clone, Copy)]
struct OrderRequestCandidateContext<'a> {
    order_id: &'a str,
    seller_pubkey: Option<&'a str>,
}

fn order_status_from_receipt(order_id: &str, receipt: DirectRelayFetchReceipt) -> OrderStatusView {
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
    let mut request_listing_events = Vec::new();
    let mut candidate_issues = Vec::new();

    for event in events {
        match order_status_record_from_event(&event) {
            Ok(OrderStatusRecord::Request {
                listing_event_id,
                record,
            }) => {
                decoded_count += 1;
                request_listing_events.push((record.event_id.clone(), listing_event_id));
                requests.push(record);
            }
            Ok(OrderStatusRecord::Decision(record)) => {
                decoded_count += 1;
                decisions.push(record);
            }
            Err(error) => {
                skipped_count += 1;
                if order_status_request_candidate(&event, order_id) {
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

    let projection = reduce_active_order_events(order_id, requests, decisions.clone());
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

    OrderStatusView {
        state,
        source: ORDER_STATUS_SOURCE.to_owned(),
        order_id: projection.order_id,
        request_event_id: projection.request_event_id,
        decision_event_id: projection.decision_event_id,
        listing_event_id,
        listing_addr: projection.listing_addr,
        buyer_pubkey: projection.buyer_pubkey,
        seller_pubkey: projection.seller_pubkey,
        last_event_id: projection.last_event_id,
        inventory,
        reducer_issues,
        target_relays,
        connected_relays,
        failed_relays: relay_failures(failed_relays),
        fetched_count,
        decoded_count,
        skipped_count,
        reason,
        actions: Vec::new(),
    }
}

fn order_status_request_candidate(event: &RadrootsNostrEvent, order_id: &str) -> bool {
    order_request_candidate_matches(
        event,
        OrderRequestCandidateContext {
            order_id,
            seller_pubkey: None,
        },
    )
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

fn active_order_status_state(status: &RadrootsActiveOrderStatus) -> &'static str {
    match status {
        RadrootsActiveOrderStatus::Missing => "missing",
        RadrootsActiveOrderStatus::Requested => "requested",
        RadrootsActiveOrderStatus::Accepted => "accepted",
        RadrootsActiveOrderStatus::Declined => "declined",
        RadrootsActiveOrderStatus::Invalid => "invalid",
    }
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
                    | "unknown_inventory_bin"
                    | "listing_inventory_over_reserved"
                    | "invalid_inventory_order"
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    match status {
        RadrootsActiveOrderStatus::Accepted => {
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

fn order_accept_inventory_preflight_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: &ResolvedSellerOrderRequest,
    resolution: &SellerOrderRequestResolution,
    status: &OrderStatusView,
) -> Result<Option<OrderDecisionView>, RuntimeError> {
    if args.decision != OrderDecisionArg::Accept {
        return Ok(None);
    }

    let listing = match fetch_current_inventory_listing(config, args, request, resolution, status)?
    {
        Ok(listing) => listing,
        Err(view) => return Ok(Some(view)),
    };
    if listing.event_id != request.listing_event_id.clone().unwrap_or_default() {
        return Ok(Some(order_decision_inventory_invalid_view(
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
        )));
    }
    if !listing_is_active(&listing.listing) {
        return Ok(Some(order_decision_inventory_invalid_view(
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
        )));
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

    let projection = reduce_listing_inventory_accounting(
        request.listing_addr.as_str(),
        listing.event_id.as_str(),
        listing.bins,
        requests,
        decisions,
    );
    if projection.issues.is_empty() {
        return Ok(None);
    }

    let issues = projection
        .issues
        .into_iter()
        .map(listing_inventory_accounting_issue_view)
        .collect::<Vec<_>>();
    Ok(Some(order_decision_inventory_invalid_view(
        config,
        args,
        request,
        resolution,
        status,
        "order accept refused because visible inventory accounting is invalid",
        issues,
    )))
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
        let record = listing_accounting_request_from_event(&event)?;
        if record.listing_event_id.as_deref() == Some(listing.event_id.as_str()) {
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
        match order_status_record_from_event(&event)? {
            OrderStatusRecord::Decision(record) => records.push(record),
            OrderStatusRecord::Request { .. } => {}
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
) -> OrderDecisionView {
    let decision_reason = args
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    let mut view = order_decision_base_view(config, args, "dry_run", true);
    apply_order_decision_request(&mut view, request);
    apply_order_decision_status(&mut view, status);
    view.reason = Some(match decision_reason {
        Some(reason) => format!(
            "dry run requested; seller order decision publication skipped with reason `{reason}`"
        ),
        None => "dry run requested; seller order decision publication skipped".to_owned(),
    });
    view.actions = vec![format!("radroots order status get {}", request.order_id)];
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
    })
}

fn publish_order_decision(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: ResolvedSellerOrderRequest,
    resolution: SellerOrderRequestResolution,
    signing: accounts::AccountSigningIdentity,
    payload: RadrootsTradeOrderDecisionEvent,
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
        config, args, request, resolution, event_kind, receipt,
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
    view
}

fn order_decision_binding_error_view(
    config: &RuntimeConfig,
    args: &OrderDecisionArgs,
    request: ResolvedSellerOrderRequest,
    resolution: SellerOrderRequestResolution,
    error: ActorWriteBindingError,
) -> OrderDecisionView {
    let (state, reason, actions) = match error {
        ActorWriteBindingError::Unconfigured(reason) => (
            "unconfigured".to_owned(),
            reason,
            vec!["run radroots signer status get".to_owned()],
        ),
    };
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

fn order_status_filter(order_id: &str) -> Result<RadrootsNostrFilter, RuntimeError> {
    let filter = RadrootsNostrFilter::new()
        .kinds([
            radroots_nostr_kind(KIND_TRADE_ORDER_REQUEST as u16),
            radroots_nostr_kind(KIND_TRADE_ORDER_DECISION as u16),
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
        return Ok(Some(ResolvedOrderListing {
            listing_addr: listing_addr.to_owned(),
            listing_event_id,
            seller_pubkey: parsed.seller_pubkey,
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
            }))
        }
        count => Err(RuntimeError::Config(format!(
            "listing lookup `{listing_lookup}` matched {count} local listings; use a unique product key or pass `--listing-addr`"
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
        qty_unit: None,
        qty_label: None,
        qty_avail: None,
        price_amt: None,
        price_currency: None,
        price_qty_amt: None,
        price_qty_unit: None,
        listing_addr: Some(listing_addr.to_owned()),
        notes: None,
    }
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
        dry_run: false,
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
    let (state, reason, actions) = match error {
        ActorWriteBindingError::Unconfigured(reason) => (
            "unconfigured".to_owned(),
            reason,
            vec!["run radroots signer status get".to_owned()],
        ),
    };

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
        .map_err(|error| ActorWriteBindingError::Unconfigured(error.to_string()))?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(buyer_pubkey) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "selected local account pubkey `{selected_pubkey}` cannot sign order buyer_pubkey `{buyer_pubkey}`"
        )));
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
        .map_err(|error| ActorWriteBindingError::Unconfigured(error.to_string()))?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(seller_pubkey) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "selected local account pubkey `{selected_pubkey}` cannot sign order seller_pubkey `{seller_pubkey}`"
        )));
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
            issues: view.issues,
            actions: view.actions,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_events::RadrootsNostrEventPtr;
    use radroots_events::kinds::KIND_TRADE_ORDER_DECISION;
    use radroots_events::trade::{
        RadrootsActiveTradeMessageType, RadrootsTradeInventoryCommitment,
        RadrootsTradeOrderDecision, RadrootsTradeOrderDecisionEvent, RadrootsTradeOrderItem,
        RadrootsTradeOrderRequested,
    };
    use radroots_events_codec::trade::{
        active_trade_event_context_from_tags, active_trade_order_decision_event_build,
        active_trade_order_decision_from_event, active_trade_order_request_event_build,
    };
    use radroots_identity::RadrootsIdentity;
    use radroots_nostr::prelude::{radroots_event_from_nostr, radroots_nostr_build_event};
    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use radroots_trade::order::canonicalize_active_order_decision_for_signer;
    use tempfile::tempdir;

    use super::{
        LoadedOrderDraft, ORDER_DRAFT_KIND, OrderDraft, OrderDraftDocument, OrderDraftItem,
        SellerOrderRequestResolution, accepted_order_decision_payload_from_request,
        canonical_order_request_payload_from_loaded, collect_issues,
        declined_order_decision_payload_from_request, inspect_document, next_order_id,
        order_decision_dry_run_view, order_decision_preflight_view_from_status,
        order_decision_view_from_resolution, order_history_entry_from_event,
        order_history_from_receipt, order_request_filter, order_status_from_receipt,
        order_submit_existing_request_view_from_receipt,
        seller_order_request_resolution_from_receipt,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime::direct_relay::DirectRelayFetchReceipt;
    use crate::runtime_args::{OrderDecisionArg, OrderDecisionArgs, OrderSubmitArgs};

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
        assert_eq!(view.decoded_count, 1);
        assert_eq!(view.skipped_count, 0);
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

        let view = order_decision_dry_run_view(&config, &args, &request, &status_view);

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

        let view = order_decision_dry_run_view(&config, &args, &request, &status_view);

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
        let decision_event_id = decision_event.id.to_string();

        assert_eq!(view.state, "accepted");
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
        assert!(view.reducer_issues.is_empty());
        assert_eq!(view.decoded_count, 2);
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

    fn signed_order_decision_event(
        seller: &RadrootsIdentity,
        request_event: &radroots_nostr::prelude::RadrootsNostrEvent,
        order_id: &str,
        listing_addr: &str,
        buyer_pubkey: &str,
        seller_pubkey: &str,
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
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("nostr event builder")
            .sign_with_keys(seller.keys())
            .expect("signed order decision")
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
        let payload = RadrootsTradeOrderRequested {
            order_id: order_id.to_owned(),
            listing_addr: listing_addr.to_owned(),
            buyer_pubkey: buyer_pubkey.to_owned(),
            seller_pubkey: seller_pubkey.to_owned(),
            items: vec![RadrootsTradeOrderItem {
                bin_id: "bin-1".to_owned(),
                bin_count: 2,
            }],
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
