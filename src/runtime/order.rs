use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_events::RadrootsNostrEventPtr;
use radroots_events::kinds::KIND_LISTING;
use radroots_events::trade::{RadrootsTradeOrderItem, RadrootsTradeOrderRequested};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::trade::{
    RadrootsTradeListingAddress, active_trade_order_request_event_build,
};
use radroots_replica_db::{ReplicaSql, nostr_event_state, trade_product};
use radroots_replica_db_schema::nostr_event_state::{
    INostrEventStateFindOne, INostrEventStateFindOneArgs, NostrEventStateQueryBindValues,
};
use radroots_replica_db_schema::trade_product::{ITradeProductFieldsFilter, ITradeProductFindMany};
use radroots_sql_core::SqliteExecutor;
use radroots_trade::order::canonicalize_active_order_request_for_signer;
use serde::{Deserialize, Serialize};

use crate::domain::runtime::{
    OrderCancelView, OrderDraftItemView, OrderGetView, OrderHistoryView, OrderIssueView,
    OrderListView, OrderNewView, OrderSubmitView, OrderSummaryView, OrderWatchView,
    RelayFailureView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayPublishReceipt, publish_parts_with_identity,
};
use crate::runtime::signer::ActorWriteBindingError;
use crate::runtime_args::{
    OrderDraftCreateArgs, OrderSubmitArgs, OrderWatchArgs, RecordLookupArgs,
};

const ORDER_DRAFT_KIND: &str = "order_draft_v1";
const ORDER_SOURCE: &str = "local order drafts · local first";
const ORDER_SUBMIT_SOURCE: &str = "direct Nostr relay publish · local key";
const ORDER_EVENT_STATE_UNAVAILABLE_REASON: &str =
    "relay-backed order event state is not implemented";
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

    match publish_order_request(config, &loaded, args, signing) {
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
        reason: Some(ORDER_EVENT_STATE_UNAVAILABLE_REASON.to_owned()),
        workflow: None,
        frames: Vec::new(),
        actions: vec![format!(
            "radroots order get {}",
            loaded.document.order.order_id
        )],
    })
}

pub fn history(config: &RuntimeConfig) -> Result<OrderHistoryView, RuntimeError> {
    let dir = drafts_dir(config);
    if !dir.exists() {
        return Ok(OrderHistoryView {
            state: "empty".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            count: 0,
            reason: Some("no relay-backed order events recorded yet".to_owned()),
            orders: Vec::new(),
            actions: vec!["radroots order list".to_owned()],
        });
    }

    let mut invalid_count = 0usize;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        if load_draft(path.as_path()).is_err() {
            invalid_count += 1;
        }
    }

    let state = if invalid_count > 0 {
        "degraded"
    } else {
        "empty"
    };

    let reason = if invalid_count > 0 {
        Some(format!(
            "{invalid_count} invalid order draft file{} skipped while reading local order event state",
            if invalid_count == 1 { "" } else { "s" }
        ))
    } else {
        Some("no relay-backed order events recorded yet".to_owned())
    };

    Ok(OrderHistoryView {
        state: state.to_owned(),
        source: ORDER_SOURCE.to_owned(),
        count: 0,
        reason,
        orders: Vec::new(),
        actions: vec!["radroots order list".to_owned()],
    })
}

pub fn cancel(
    config: &RuntimeConfig,
    args: &RecordLookupArgs,
) -> Result<OrderCancelView, RuntimeError> {
    let file = draft_lookup_path(config, args.key.as_str());
    if !file.exists() {
        return Ok(OrderCancelView {
            state: "missing".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup: args.key.clone(),
            order_id: None,
            reason: Some(format!("order draft `{}` was not found", args.key)),
            job: None,
            actions: vec!["radroots order list".to_owned()],
        });
    }

    let loaded = match load_draft(file.as_path()) {
        Ok(loaded) => loaded,
        Err(reason) => {
            return Ok(OrderCancelView {
                state: "error".to_owned(),
                source: ORDER_SOURCE.to_owned(),
                lookup: args.key.clone(),
                order_id: None,
                reason: Some(reason),
                job: None,
                actions: Vec::new(),
            });
        }
    };

    Ok(OrderCancelView {
        state: "unavailable".to_owned(),
        source: ORDER_SOURCE.to_owned(),
        lookup: args.key.clone(),
        order_id: Some(loaded.document.order.order_id.clone()),
        reason: Some(ORDER_EVENT_STATE_UNAVAILABLE_REASON.to_owned()),
        job: None,
        actions: vec![format!(
            "radroots order get {}",
            loaded.document.order.order_id
        )],
    })
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
        issues: vec![OrderIssueView {
            field: "draft".to_owned(),
            message: reason,
        }],
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

fn publish_order_request(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    signing: accounts::AccountSigningIdentity,
) -> Result<OrderSubmitView, RuntimeError> {
    let signer_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
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
    let payload = canonicalize_active_order_request_for_signer(payload, signer_pubkey)
        .map_err(|error| RuntimeError::Config(format!("canonicalize order request: {error}")))?;
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
    OrderIssueView {
        field: field.into(),
        message: message.into(),
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
    use super::{
        ORDER_DRAFT_KIND, OrderDraft, OrderDraftDocument, OrderDraftItem, collect_issues,
        inspect_document, next_order_id,
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
}
