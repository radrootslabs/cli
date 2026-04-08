use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radroots_events::kinds::KIND_LISTING;
use radroots_events::trade::{
    RadrootsTradeMessageType, RadrootsTradeOrder, RadrootsTradeOrderItem,
};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::trade::RadrootsTradeListingAddress;
use serde::{Deserialize, Serialize};

use crate::cli::{OrderNewArgs, OrderSubmitArgs, OrderWatchArgs, RecordKeyArgs};
use crate::domain::runtime::{
    OrderCancelView, OrderDraftItemView, OrderGetView, OrderHistoryEntryView, OrderHistoryView,
    OrderIssueView, OrderJobView, OrderListView, OrderNewView, OrderSubmitView, OrderSummaryView,
    OrderWatchFrameView, OrderWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::daemon::{self, DaemonRpcError};

const ORDER_DRAFT_KIND: &str = "order_draft_v1";
const ORDER_SOURCE: &str = "local order drafts · local first";
const ORDER_LIFECYCLE_SOURCE: &str = "local order drafts · durable job lifecycle";
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    submission: Option<OrderDraftSubmission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraft {
    order_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    listing_addr: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderDraftSubmission {
    job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signer_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signer_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event_addr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    submitted_at_unix: Option<u64>,
}

#[derive(Debug, Clone)]
struct LoadedOrderDraft {
    file: PathBuf,
    updated_at_unix: u64,
    document: OrderDraftDocument,
}

pub fn scaffold(config: &RuntimeConfig, args: &OrderNewArgs) -> Result<OrderNewView, RuntimeError> {
    validate_scaffold_args(args)?;

    let selected_account = accounts::resolve_account(config)?;
    let buyer_account_id = selected_account
        .as_ref()
        .map(|account| account.record.account_id.to_string());
    let buyer_pubkey = selected_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.clone())
        .unwrap_or_default();

    let listing_lookup = normalize_optional(args.listing.as_deref());
    let listing_addr = normalize_optional(args.listing_addr.as_deref()).unwrap_or_default();
    let parsed_listing_addr = parse_listing_addr(listing_addr.as_str());
    let seller_pubkey = parsed_listing_addr
        .as_ref()
        .map(|listing| listing.seller_pubkey.clone())
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
            buyer_pubkey,
            seller_pubkey,
            items,
        },
        listing_lookup,
        buyer_account_id,
        submission: None,
    };
    save_draft(file.as_path(), &document)?;

    let mut view: OrderNewView = view_from_loaded(
        config,
        LoadedOrderDraft {
            file,
            updated_at_unix: now_unix(),
            document,
        },
        false,
    )
    .into();
    view.actions
        .insert(0, format!("radroots order get {}", view.order_id));

    Ok(view)
}

pub fn get(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<OrderGetView, RuntimeError> {
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
            buyer_account_id: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            ready_for_submit: false,
            items: Vec::new(),
            updated_at_unix: None,
            job: None,
            reason: Some(format!("order draft `{lookup}` was not found")),
            issues: Vec::new(),
            actions: vec![
                "radroots order ls".to_owned(),
                "radroots order new".to_owned(),
            ],
        });
    }

    match load_draft(file.as_path()) {
        Ok(loaded) => Ok(view_from_loaded(config, loaded, true)),
        Err(reason) => Ok(OrderGetView {
            state: "error".to_owned(),
            source: ORDER_SOURCE.to_owned(),
            lookup,
            order_id: None,
            file: Some(file.display().to_string()),
            listing_lookup: None,
            listing_addr: None,
            buyer_account_id: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            ready_for_submit: false,
            items: Vec::new(),
            updated_at_unix: None,
            job: None,
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
            actions: vec!["radroots order new".to_owned()],
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
            Ok(loaded) => orders.push(summary_from_loaded(config, &loaded)),
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
        vec!["radroots order new".to_owned()]
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
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            order_id: args.key.clone(),
            file: file.display().to_string(),
            listing_lookup: None,
            listing_addr: None,
            buyer_account_id: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            dry_run: false,
            deduplicated: false,
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some(format!("order draft `{}` was not found", args.key)),
            job: None,
            issues: Vec::new(),
            actions: vec![
                "radroots order ls".to_owned(),
                "radroots order new".to_owned(),
            ],
        });
    }

    let loaded = match load_draft(file.as_path()) {
        Ok(loaded) => loaded,
        Err(reason) => {
            return Ok(OrderSubmitView {
                state: "error".to_owned(),
                source: ORDER_LIFECYCLE_SOURCE.to_owned(),
                order_id: args.key.clone(),
                file: file.display().to_string(),
                listing_lookup: None,
                listing_addr: None,
                buyer_account_id: None,
                buyer_pubkey: None,
                seller_pubkey: None,
                dry_run: false,
                deduplicated: false,
                idempotency_key: args.idempotency_key.clone(),
                signer_mode: None,
                signer_session_id: None,
                requested_signer_session_id: args.signer_session_id.clone(),
                reason: Some(reason),
                job: None,
                issues: Vec::new(),
                actions: Vec::new(),
            });
        }
    };

    if let Some(job) = submission_job_view(config, &loaded.document, true) {
        let mut actions = vec![
            format!("radroots order watch {}", loaded.document.order.order_id),
            format!("radroots order history"),
        ];
        actions.push(format!("radroots job get {}", job.job_id));
        return Ok(OrderSubmitView {
            state: "already_submitted".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            buyer_account_id: loaded.document.buyer_account_id.clone(),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            dry_run: false,
            deduplicated: false,
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: job.signer_mode.clone(),
            signer_session_id: job.signer_session_id.clone(),
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some("order draft already has a recorded submission job".to_owned()),
            job: Some(job),
            issues: Vec::new(),
            actions,
        });
    }

    let issues = collect_issues(&loaded.document);
    if !issues.is_empty() {
        let mut actions = actions_for_document(&loaded.document, loaded.file.as_path(), &issues);
        actions.push(format!(
            "radroots order get {}",
            loaded.document.order.order_id
        ));
        return Ok(OrderSubmitView {
            state: "unconfigured".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            buyer_account_id: loaded.document.buyer_account_id.clone(),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            dry_run: false,
            deduplicated: false,
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some("order draft is not ready for durable submit".to_owned()),
            job: None,
            issues,
            actions,
        });
    }

    if config.output.dry_run {
        return Ok(OrderSubmitView {
            state: "dry_run".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            file: loaded.file.display().to_string(),
            listing_lookup: loaded.document.listing_lookup.clone(),
            listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
            buyer_account_id: loaded.document.buyer_account_id.clone(),
            buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
            seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
            dry_run: true,
            deduplicated: false,
            idempotency_key: args.idempotency_key.clone(),
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some("dry run requested; daemon order submission skipped".to_owned()),
            job: Some(OrderJobView {
                job_id: "not_submitted".to_owned(),
                state: "not_submitted".to_owned(),
                command: Some("order.submit".to_owned()),
                signer_mode: Some(config.signer.backend.as_str().to_owned()),
                signer_session_id: None,
                requested_signer_session_id: args.signer_session_id.clone(),
                event_id: None,
                event_addr: None,
                reason: None,
            }),
            issues: Vec::new(),
            actions: vec![format!(
                "radroots order submit {}",
                loaded.document.order.order_id
            )],
        });
    }

    let signer_session_id = match daemon::resolve_signer_session_id(
        config,
        "buyer",
        loaded.document.order.buyer_pubkey.as_str(),
        u32::from(RadrootsTradeMessageType::OrderRequest.kind()),
        args.signer_session_id.as_deref(),
    ) {
        Ok(session_id) => session_id,
        Err(error) => return Ok(order_submit_error_view(&loaded, args, error)),
    };

    let order = trade_order_from_document(&loaded.document);
    match daemon::bridge_order_request(
        config,
        &order,
        args.idempotency_key.as_deref(),
        Some(signer_session_id.as_str()),
    ) {
        Ok(result) => {
            let mut updated = loaded.document.clone();
            updated.submission = Some(OrderDraftSubmission {
                job_id: result.job_id.clone(),
                state: Some(result.status.clone()),
                signer_mode: Some(result.signer_mode.clone()),
                signer_session_id: result.signer_session_id.clone(),
                command: Some("order.submit".to_owned()),
                event_id: result.event_id.clone(),
                event_addr: result.event_addr.clone(),
                submitted_at_unix: Some(now_unix()),
            });
            save_draft(loaded.file.as_path(), &updated)?;

            let failed = result.status == "failed";
            let mut actions = Vec::new();
            if failed {
                actions.push(format!("radroots job get {}", result.job_id));
                actions.push("radroots rpc status".to_owned());
                actions.push("radroots order history".to_owned());
            } else {
                actions.push(format!("radroots order watch {}", updated.order.order_id));
                actions.push(format!("radroots job get {}", result.job_id));
                actions.push("radroots order history".to_owned());
            }

            Ok(OrderSubmitView {
                state: if failed {
                    "unavailable".to_owned()
                } else if result.deduplicated {
                    "deduplicated".to_owned()
                } else {
                    result.status.clone()
                },
                source: daemon::bridge_source().to_owned(),
                order_id: updated.order.order_id.clone(),
                file: loaded.file.display().to_string(),
                listing_lookup: updated.listing_lookup.clone(),
                listing_addr: non_empty_string(updated.order.listing_addr.clone()),
                buyer_account_id: updated.buyer_account_id.clone(),
                buyer_pubkey: non_empty_string(updated.order.buyer_pubkey.clone()),
                seller_pubkey: non_empty_string(updated.order.seller_pubkey.clone()),
                dry_run: false,
                deduplicated: result.deduplicated,
                idempotency_key: result.idempotency_key.clone(),
                signer_mode: Some(result.signer_mode.clone()),
                signer_session_id: result.signer_session_id.clone(),
                requested_signer_session_id: args.signer_session_id.clone(),
                reason: failed.then(|| {
                    "daemon order request failed before relay delivery completed".to_owned()
                }),
                job: Some(OrderJobView {
                    job_id: result.job_id,
                    state: result.status,
                    command: Some("order.submit".to_owned()),
                    signer_mode: Some(result.signer_mode),
                    signer_session_id: result.signer_session_id,
                    requested_signer_session_id: args.signer_session_id.clone(),
                    event_id: result.event_id,
                    event_addr: result.event_addr,
                    reason: None,
                }),
                issues: Vec::new(),
                actions,
            })
        }
        Err(error) => Ok(order_submit_error_view(&loaded, args, error)),
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
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            order_id: args.key.clone(),
            job_id: None,
            interval_ms: args.interval_ms,
            reason: Some(format!("order draft `{}` was not found", args.key)),
            frames: Vec::new(),
            actions: vec!["radroots order ls".to_owned()],
        });
    }

    let loaded = match load_draft(file.as_path()) {
        Ok(loaded) => loaded,
        Err(reason) => {
            return Ok(OrderWatchView {
                state: "error".to_owned(),
                source: ORDER_LIFECYCLE_SOURCE.to_owned(),
                order_id: args.key.clone(),
                job_id: None,
                interval_ms: args.interval_ms,
                reason: Some(reason),
                frames: Vec::new(),
                actions: Vec::new(),
            });
        }
    };

    let Some(submission) = loaded.document.submission.as_ref() else {
        return Ok(OrderWatchView {
            state: "not_submitted".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            order_id: loaded.document.order.order_id.clone(),
            job_id: None,
            interval_ms: args.interval_ms,
            reason: Some("order draft does not have a recorded submission job yet".to_owned()),
            frames: Vec::new(),
            actions: vec![format!(
                "radroots order submit {}",
                loaded.document.order.order_id
            )],
        });
    };

    let job_id = submission.job_id.clone();
    let max_frames = args.frames.unwrap_or(usize::MAX);
    let mut frames = Vec::new();
    loop {
        match daemon::bridge_job(config, job_id.as_str()) {
            Ok(Some(job)) => {
                frames.push(OrderWatchFrameView {
                    sequence: frames.len() + 1,
                    observed_at_unix: job.completed_at_unix.unwrap_or(job.requested_at_unix),
                    state: job.state.clone(),
                    terminal: job.terminal,
                    signer_mode: job.signer.clone(),
                    signer_session_id: job.signer_session_id.clone(),
                    summary: job.relay_outcome_summary.clone(),
                });
                if job.terminal || frames.len() >= max_frames {
                    return Ok(OrderWatchView {
                        state: if job.terminal {
                            job.state
                        } else {
                            "watching".to_owned()
                        },
                        source: ORDER_LIFECYCLE_SOURCE.to_owned(),
                        order_id: loaded.document.order.order_id.clone(),
                        job_id: Some(job_id.clone()),
                        interval_ms: args.interval_ms,
                        reason: None,
                        frames,
                        actions: vec!["radroots order history".to_owned()],
                    });
                }
            }
            Ok(None) => {
                return Ok(OrderWatchView {
                    state: "missing".to_owned(),
                    source: ORDER_LIFECYCLE_SOURCE.to_owned(),
                    order_id: loaded.document.order.order_id.clone(),
                    job_id: Some(job_id.clone()),
                    interval_ms: args.interval_ms,
                    reason: Some("recorded job id was not found in radrootsd".to_owned()),
                    frames,
                    actions: vec!["radroots order history".to_owned()],
                });
            }
            Err(error) => return Ok(order_watch_error_view(&loaded, args, job_id, frames, error)),
        }

        thread::sleep(Duration::from_millis(args.interval_ms));
    }
}

pub fn history(config: &RuntimeConfig) -> Result<OrderHistoryView, RuntimeError> {
    let dir = drafts_dir(config);
    if !dir.exists() {
        return Ok(OrderHistoryView {
            state: "empty".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            count: 0,
            reason: Some("no submitted order drafts recorded yet".to_owned()),
            orders: Vec::new(),
            actions: vec!["radroots order ls".to_owned()],
        });
    }

    let mut orders = Vec::new();
    let mut invalid_count = 0usize;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        match load_draft(path.as_path()) {
            Ok(loaded) => {
                if loaded.document.submission.is_some() {
                    orders.push(history_entry_from_loaded(config, &loaded));
                }
            }
            Err(_) => {
                invalid_count += 1;
            }
        }
    }

    orders.sort_by(|left, right| {
        right
            .submitted_at_unix
            .unwrap_or(right.updated_at_unix)
            .cmp(&left.submitted_at_unix.unwrap_or(left.updated_at_unix))
            .then_with(|| left.id.cmp(&right.id))
    });

    let state = if orders.is_empty() {
        "empty"
    } else if invalid_count > 0
        || orders
            .iter()
            .any(|order| matches!(order.state.as_str(), "error" | "unavailable"))
    {
        "degraded"
    } else {
        "ready"
    };

    let reason = if orders.is_empty() {
        Some("no submitted order drafts recorded yet".to_owned())
    } else if invalid_count > 0 {
        Some(format!(
            "{invalid_count} invalid order draft file{} skipped while building history",
            if invalid_count == 1 { "" } else { "s" }
        ))
    } else {
        None
    };

    Ok(OrderHistoryView {
        state: state.to_owned(),
        source: ORDER_LIFECYCLE_SOURCE.to_owned(),
        count: orders.len(),
        reason,
        orders,
        actions: if state == "empty" {
            vec!["radroots order new".to_owned()]
        } else {
            Vec::new()
        },
    })
}

pub fn cancel(
    config: &RuntimeConfig,
    args: &RecordKeyArgs,
) -> Result<OrderCancelView, RuntimeError> {
    let file = draft_lookup_path(config, args.key.as_str());
    if !file.exists() {
        return Ok(OrderCancelView {
            state: "missing".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            lookup: args.key.clone(),
            order_id: None,
            reason: Some(format!("order draft `{}` was not found", args.key)),
            job: None,
            actions: vec!["radroots order ls".to_owned()],
        });
    }

    let loaded = match load_draft(file.as_path()) {
        Ok(loaded) => loaded,
        Err(reason) => {
            return Ok(OrderCancelView {
                state: "error".to_owned(),
                source: ORDER_LIFECYCLE_SOURCE.to_owned(),
                lookup: args.key.clone(),
                order_id: None,
                reason: Some(reason),
                job: None,
                actions: Vec::new(),
            });
        }
    };

    let Some(job) = submission_job_view(config, &loaded.document, false) else {
        return Ok(OrderCancelView {
            state: "not_submitted".to_owned(),
            source: ORDER_LIFECYCLE_SOURCE.to_owned(),
            lookup: args.key.clone(),
            order_id: Some(loaded.document.order.order_id.clone()),
            reason: Some("order draft has not been submitted yet".to_owned()),
            job: None,
            actions: vec![format!(
                "radroots order submit {}",
                loaded.document.order.order_id
            )],
        });
    };

    let job_id = loaded
        .document
        .submission
        .as_ref()
        .map(|submission| submission.job_id.clone());
    Ok(OrderCancelView {
        state: "unconfigured".to_owned(),
        source: ORDER_LIFECYCLE_SOURCE.to_owned(),
        lookup: args.key.clone(),
        order_id: Some(loaded.document.order.order_id.clone()),
        reason: Some(
            "durable order cancel needs trade-chain root and previous event refs that the current local order read plane does not persist yet".to_owned(),
        ),
        job: Some(job),
        actions: vec![
            "radroots order history".to_owned(),
            format!("radroots job get {}", job_id.unwrap_or_default()),
        ],
    })
}

fn validate_scaffold_args(args: &OrderNewArgs) -> Result<(), RuntimeError> {
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

fn view_from_loaded(
    config: &RuntimeConfig,
    loaded: LoadedOrderDraft,
    enrich_job: bool,
) -> OrderGetView {
    let OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        seller_pubkey,
        issues,
        job,
    } = inspect_document(config, &loaded.document, enrich_job);

    let mut actions =
        actions_for_document(&loaded.document, loaded.file.as_path(), issues.as_slice());
    if let Some(job) = &job {
        actions.push(format!("radroots job get {}", job.job_id));
        actions.push("radroots order history".to_owned());
    }

    OrderGetView {
        state,
        source: ORDER_SOURCE.to_owned(),
        lookup: loaded.document.order.order_id.clone(),
        order_id: Some(loaded.document.order.order_id.clone()),
        file: Some(loaded.file.display().to_string()),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr,
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
        job,
        reason: None,
        issues,
        actions,
    }
}

fn summary_from_loaded(config: &RuntimeConfig, loaded: &LoadedOrderDraft) -> OrderSummaryView {
    let OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        seller_pubkey: _,
        issues,
        job,
    } = inspect_document(config, &loaded.document, false);

    OrderSummaryView {
        id: loaded.document.order.order_id.clone(),
        state,
        ready_for_submit,
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr,
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        item_count: loaded.document.order.items.len(),
        updated_at_unix: loaded.updated_at_unix,
        job,
        issues,
    }
}

fn history_entry_from_loaded(
    config: &RuntimeConfig,
    loaded: &LoadedOrderDraft,
) -> OrderHistoryEntryView {
    let job = submission_job_view(config, &loaded.document, true);
    let submitted_at_unix = loaded
        .document
        .submission
        .as_ref()
        .and_then(|submission| submission.submitted_at_unix);
    OrderHistoryEntryView {
        id: loaded.document.order.order_id.clone(),
        state: job
            .as_ref()
            .map(|job| job.state.clone())
            .unwrap_or_else(|| "recorded".to_owned()),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        submitted_at_unix,
        updated_at_unix: loaded.updated_at_unix,
        job,
        issues: Vec::new(),
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

fn inspect_document(
    config: &RuntimeConfig,
    document: &OrderDraftDocument,
    enrich_job: bool,
) -> OrderInspection {
    let listing_addr = non_empty_string(document.order.listing_addr.clone());
    let parsed_listing_addr = listing_addr
        .as_deref()
        .and_then(|value| parse_listing_addr(value).ok());
    let seller_pubkey = non_empty_string(document.order.seller_pubkey.clone()).or_else(|| {
        parsed_listing_addr
            .as_ref()
            .map(|listing| listing.seller_pubkey.clone())
    });
    let issues = collect_issues(document);
    let job = submission_job_view(config, document, enrich_job);
    let ready_for_submit = issues.is_empty() && job.is_none();
    let state = if job.is_some() {
        "submitted".to_owned()
    } else if ready_for_submit {
        "ready".to_owned()
    } else {
        "draft".to_owned()
    };

    OrderInspection {
        state,
        ready_for_submit,
        listing_addr,
        seller_pubkey,
        issues,
        job,
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
        actions.push("radroots account new".to_owned());
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

fn submission_job_view(
    config: &RuntimeConfig,
    document: &OrderDraftDocument,
    enrich: bool,
) -> Option<OrderJobView> {
    let submission = document.submission.as_ref()?;
    let job_id = normalize_optional(Some(submission.job_id.as_str()))?;
    if !enrich || config.rpc.bridge_bearer_token.is_none() {
        return Some(recorded_job_view(submission, job_id));
    }

    match daemon::bridge_job(config, job_id.as_str()) {
        Ok(Some(job)) => Some(OrderJobView {
            job_id,
            state: job.state,
            command: Some(job.command),
            signer_mode: Some(job.signer.clone()),
            signer_session_id: job.signer_session_id,
            requested_signer_session_id: None,
            event_id: job.event_id,
            event_addr: job.event_addr,
            reason: None,
        }),
        Ok(None) => Some(OrderJobView {
            job_id,
            state: "missing".to_owned(),
            command: submission.command.clone(),
            signer_mode: submission.signer_mode.clone(),
            signer_session_id: submission.signer_session_id.clone(),
            requested_signer_session_id: None,
            event_id: submission.event_id.clone(),
            event_addr: submission.event_addr.clone(),
            reason: Some("recorded job id was not found in radrootsd".to_owned()),
        }),
        Err(error) => Some(job_view_from_error(job_id, error)),
    }
}

fn recorded_job_view(submission: &OrderDraftSubmission, job_id: String) -> OrderJobView {
    OrderJobView {
        job_id,
        state: submission
            .state
            .clone()
            .unwrap_or_else(|| "recorded".to_owned()),
        command: submission.command.clone(),
        signer_mode: submission.signer_mode.clone(),
        signer_session_id: submission.signer_session_id.clone(),
        requested_signer_session_id: None,
        event_id: submission.event_id.clone(),
        event_addr: submission.event_addr.clone(),
        reason: None,
    }
}

fn job_view_from_error(job_id: String, error: DaemonRpcError) -> OrderJobView {
    match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => OrderJobView {
            job_id,
            state: "unconfigured".to_owned(),
            command: None,
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: None,
            event_id: None,
            event_addr: None,
            reason: Some(reason),
        },
        DaemonRpcError::External(reason) => OrderJobView {
            job_id,
            state: "unavailable".to_owned(),
            command: None,
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: None,
            event_id: None,
            event_addr: None,
            reason: Some(reason),
        },
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => OrderJobView {
            job_id,
            state: "error".to_owned(),
            command: None,
            signer_mode: None,
            signer_session_id: None,
            requested_signer_session_id: None,
            event_id: None,
            event_addr: None,
            reason: Some(reason),
        },
    }
}

fn order_submit_error_view(
    loaded: &LoadedOrderDraft,
    args: &OrderSubmitArgs,
    error: DaemonRpcError,
) -> OrderSubmitView {
    let (state, reason, mut actions) = match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => (
            "unconfigured".to_owned(),
            reason,
            vec![
                "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                "start radrootsd with bridge ingress enabled".to_owned(),
            ],
        ),
        DaemonRpcError::External(reason) => (
            "unavailable".to_owned(),
            reason,
            vec!["start radrootsd and verify the rpc url".to_owned()],
        ),
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => ("error".to_owned(), reason, Vec::new()),
    };
    actions.push(format!(
        "radroots order get {}",
        loaded.document.order.order_id
    ));

    OrderSubmitView {
        state,
        source: daemon::bridge_source().to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        file: loaded.file.display().to_string(),
        listing_lookup: loaded.document.listing_lookup.clone(),
        listing_addr: non_empty_string(loaded.document.order.listing_addr.clone()),
        buyer_account_id: loaded.document.buyer_account_id.clone(),
        buyer_pubkey: non_empty_string(loaded.document.order.buyer_pubkey.clone()),
        seller_pubkey: non_empty_string(loaded.document.order.seller_pubkey.clone()),
        dry_run: false,
        deduplicated: false,
        idempotency_key: args.idempotency_key.clone(),
        signer_mode: None,
        signer_session_id: None,
        requested_signer_session_id: args.signer_session_id.clone(),
        reason: Some(reason),
        job: None,
        issues: Vec::new(),
        actions,
    }
}

fn order_watch_error_view(
    loaded: &LoadedOrderDraft,
    args: &OrderWatchArgs,
    job_id: String,
    frames: Vec<OrderWatchFrameView>,
    error: DaemonRpcError,
) -> OrderWatchView {
    let (state, reason, actions) = match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => (
            "unconfigured".to_owned(),
            reason,
            vec![
                "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                "start radrootsd with bridge ingress enabled".to_owned(),
            ],
        ),
        DaemonRpcError::External(reason) => (
            "unavailable".to_owned(),
            reason,
            vec!["start radrootsd and verify the rpc url".to_owned()],
        ),
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => ("error".to_owned(), reason, Vec::new()),
    };

    OrderWatchView {
        state,
        source: ORDER_LIFECYCLE_SOURCE.to_owned(),
        order_id: loaded.document.order.order_id.clone(),
        job_id: Some(job_id),
        interval_ms: args.interval_ms,
        reason: Some(reason),
        frames,
        actions: if actions.is_empty() {
            Vec::new()
        } else {
            actions
        },
    }
}

fn trade_order_from_document(document: &OrderDraftDocument) -> RadrootsTradeOrder {
    RadrootsTradeOrder {
        order_id: document.order.order_id.clone(),
        listing_addr: document.order.listing_addr.clone(),
        buyer_pubkey: document.order.buyer_pubkey.clone(),
        seller_pubkey: document.order.seller_pubkey.clone(),
        items: document
            .order
            .items
            .iter()
            .map(|item| RadrootsTradeOrderItem {
                bin_id: item.bin_id.clone(),
                bin_count: item.bin_count,
            })
            .collect(),
        discounts: None,
    }
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
    seller_pubkey: Option<String>,
    issues: Vec<OrderIssueView>,
    job: Option<OrderJobView>,
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
        ORDER_DRAFT_KIND, OrderDraft, OrderDraftDocument, OrderDraftItem, OrderDraftSubmission,
        next_order_id,
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
                buyer_pubkey: "a".repeat(64),
                seller_pubkey: "b".repeat(64),
                items: vec![OrderDraftItem {
                    bin_id: "bin-1".to_owned(),
                    bin_count: 2,
                }],
            },
            listing_lookup: Some("fresh-eggs".to_owned()),
            buyer_account_id: Some("acct_demo".to_owned()),
            submission: Some(OrderDraftSubmission {
                job_id: "job_01".to_owned(),
                state: Some("accepted".to_owned()),
                signer_mode: Some("embedded_service_identity".to_owned()),
                signer_session_id: None,
                command: Some("order.submit".to_owned()),
                event_id: None,
                event_addr: None,
                submitted_at_unix: Some(1),
            }),
        };

        let rendered = toml::to_string_pretty(&document).expect("render draft");
        assert!(rendered.contains("kind = \"order_draft_v1\""));
        assert!(rendered.contains("order_id = \"ord_AAAAAAAAAAAAAAAAAAAAAg\""));
        assert!(rendered.contains("job_id = \"job_01\""));
    }
}
