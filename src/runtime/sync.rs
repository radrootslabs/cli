use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radroots_events::kinds::{
    KIND_FARM, KIND_LIST_SET_APP_CURATION, KIND_LIST_SET_BOOKMARK, KIND_LIST_SET_CALENDAR,
    KIND_LIST_SET_CURATION, KIND_LIST_SET_EMOJI, KIND_LIST_SET_FOLLOW, KIND_LIST_SET_GENERIC,
    KIND_LIST_SET_INTEREST, KIND_LIST_SET_KIND_MUTE, KIND_LIST_SET_MEDIA_STARTER_PACK,
    KIND_LIST_SET_PICTURE, KIND_LIST_SET_RELAY, KIND_LIST_SET_RELEASE_ARTIFACT,
    KIND_LIST_SET_STARTER_PACK, KIND_LIST_SET_VIDEO, KIND_LISTING, KIND_PLOT, KIND_PROFILE,
};
use radroots_nostr::prelude::{
    RadrootsNostrFilter, RadrootsNostrTimestamp, radroots_event_from_nostr, radroots_nostr_kind,
};
use radroots_replica_db::{ReplicaSql, migrations};
use radroots_replica_sync::{
    RadrootsReplicaEventsError, RadrootsReplicaIngestOutcome, radroots_replica_ingest_event,
    radroots_replica_sync_status,
};
use radroots_sdk::{
    PushOutboxEventReceipt, PushOutboxReceipt, PushOutboxRequest, PushOutboxTargetOutcomeKind,
    PushOutboxTargetReceipt, SyncStatusReceipt, SyncStatusRequest,
};
use radroots_sql_core::{SqlExecutor, SqliteExecutor};
use radroots_transport_nostr::{
    RadrootsRelayFetchFailure, RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError,
};
use serde::Deserialize;
use serde_json::json;

use crate::cli::global::SyncWatchArgs;
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::sdk::{
    CliSdkAdapterError, CliSdkSession, fetch_relay_events_via_shared_transport,
    sdk_nostr_relay_url_policy,
};
use crate::view::runtime::{
    SyncActionView, SyncFreshnessView, SyncQueueView, SyncRunFreshnessView, SyncStatusView,
    SyncTransportStatusView, SyncTransportTargetView, SyncWatchFrameView, SyncWatchView,
    TransportTargetFailureView,
};

const SYNC_SOURCE: &str = "local replica · local first";
const SDK_SYNC_SOURCE: &str = "SDK canonical event store and outbox";
const SDK_PUSH_SOURCE: &str = "SDK outbox push";
const RELAY_PULL_SETUP_ACTION: &str =
    "radroots transport profile set --kind nostr --nostr-relay wss://relay.example.com";
const SYNC_PULL_ACTION: &str = "radroots sync pull";
const SYNC_PUSH_ACTION: &str = "radroots sync push";
const SYNC_READY_ACTION: &str = "radroots market product search eggs";
const MARKET_READY_ACTION: &str = "radroots market product search eggs";
const INGEST_SOURCE: &str = "shared relay transport fetch · local replica ingest";
const RELAY_FETCH_LIMIT: usize = 1_000;
const RELAY_FETCH_MAX_PAGES: usize = 5;
const MARKET_FRESHNESS_STALE_AFTER_SECONDS: u64 = 15 * 60;
const SYNC_PULL_FRESHNESS_STALE_AFTER_SECONDS: u64 = 30 * 60;
const SYNC_RUN_TABLE: &str = "radroots_cli_sync_run";
const MARKET_REFRESH_KINDS: &[u32] = &[KIND_PROFILE, KIND_FARM, KIND_LISTING];
const SYNC_PULL_KINDS: &[u32] = &[
    KIND_PROFILE,
    KIND_FARM,
    KIND_PLOT,
    KIND_LISTING,
    KIND_LIST_SET_FOLLOW,
    KIND_LIST_SET_GENERIC,
    KIND_LIST_SET_RELAY,
    KIND_LIST_SET_BOOKMARK,
    KIND_LIST_SET_CURATION,
    KIND_LIST_SET_VIDEO,
    KIND_LIST_SET_PICTURE,
    KIND_LIST_SET_KIND_MUTE,
    KIND_LIST_SET_INTEREST,
    KIND_LIST_SET_EMOJI,
    KIND_LIST_SET_RELEASE_ARTIFACT,
    KIND_LIST_SET_APP_CURATION,
    KIND_LIST_SET_CALENDAR,
    KIND_LIST_SET_STARTER_PACK,
    KIND_LIST_SET_MEDIA_STARTER_PACK,
];

#[derive(Debug, Clone)]
struct SyncSnapshot {
    state: String,
    source: String,
    local_root: String,
    replica_db: String,
    configured_transport_target_count: usize,
    publish_policy: String,
    freshness: SyncFreshnessView,
    queue: SyncQueueView,
    reason: Option<String>,
    actions: Vec<String>,
}

#[derive(Debug, Clone)]
struct SyncRunRecord {
    scope: String,
    relay_set_fingerprint: String,
    target_transport_endpoints_json: String,
    attempted_transport_endpoints_json: String,
    failed_transport_targets_json: String,
    started_at: u64,
    completed_at: Option<u64>,
    state: String,
    fetched_count: usize,
    ingested_count: usize,
    skipped_count: usize,
    unsupported_count: usize,
    failed_count: usize,
    failure_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SyncRunRow {
    scope: String,
    relay_set_fingerprint: String,
    target_transport_endpoints_json: String,
    attempted_transport_endpoints_json: String,
    failed_transport_targets_json: String,
    started_at: i64,
    completed_at: Option<i64>,
    state: String,
    fetched_count: i64,
    ingested_count: i64,
    skipped_count: i64,
    unsupported_count: i64,
    failed_count: i64,
    failure_reason: Option<String>,
}

pub fn status(config: &RuntimeConfig) -> Result<SyncStatusView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().sync().status(SyncStatusRequest::new()))?;
    Ok(sdk_sync_status_view(config, receipt))
}

pub fn pull(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    pull_with_fetcher(config, shared_relay_transport_fetch_windowed)
}

pub fn market_refresh(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    market_refresh_with_fetcher(config, shared_relay_transport_fetch_windowed)
}

fn pull_with_fetcher<F>(config: &RuntimeConfig, fetcher: F) -> Result<SyncActionView, RuntimeError>
where
    F: FnOnce(
        &[String],
        RadrootsNostrFilter,
    ) -> Result<RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError>,
{
    relay_ingest(config, RelayIngestScope::SyncPull, fetcher)
}

fn market_refresh_with_fetcher<F>(
    config: &RuntimeConfig,
    fetcher: F,
) -> Result<SyncActionView, RuntimeError>
where
    F: FnOnce(
        &[String],
        RadrootsNostrFilter,
    ) -> Result<RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError>,
{
    relay_ingest(config, RelayIngestScope::MarketRefresh, fetcher)
}

fn shared_relay_transport_fetch_windowed(
    relay_urls: &[String],
    base_filter: RadrootsNostrFilter,
) -> Result<RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError> {
    let mut next_filter = base_filter.clone();
    let mut merged: Option<RadrootsRelayFetchedEventsReceipt> = None;

    for _ in 0..RELAY_FETCH_MAX_PAGES {
        let receipt = fetch_relay_events_via_shared_transport(
            relay_urls,
            unix_now_ms(),
            RELAY_FETCH_LIMIT,
            next_filter,
        )?;
        let page_len = receipt.events.len();
        let oldest_created_at = receipt
            .events
            .iter()
            .map(|event| event.event.created_at.as_secs())
            .min();
        merge_fetch_receipt(&mut merged, receipt);
        if page_len < RELAY_FETCH_LIMIT {
            break;
        }
        let Some(oldest_created_at) = oldest_created_at else {
            break;
        };
        if oldest_created_at == 0 {
            break;
        }
        next_filter = base_filter
            .clone()
            .until(RadrootsNostrTimestamp::from(oldest_created_at - 1))
            .limit(RELAY_FETCH_LIMIT);
    }

    merged.ok_or(RadrootsRelayTransportError::EmptyTargetSet)
}

fn relay_ingest<F>(
    config: &RuntimeConfig,
    scope: RelayIngestScope,
    fetcher: F,
) -> Result<SyncActionView, RuntimeError>
where
    F: FnOnce(
        &[String],
        RadrootsNostrFilter,
    ) -> Result<RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError>,
{
    let snapshot = inspect_sync(config)?;
    if snapshot.state == "unconfigured" {
        return Ok(empty_action_from_snapshot(snapshot, "pull"));
    }

    if config.output.dry_run {
        let mut view = empty_action_from_snapshot(snapshot, "pull");
        view.state = "ready".to_owned();
        view.reason = Some("dry run requested; relay fetch skipped".to_owned());
        view.target_transport_endpoints = config.transport.nostr_relay_urls.clone();
        view.fetched_count = Some(0);
        view.ingested_count = Some(0);
        view.publishable_count = None;
        view.published_count = None;
        view.skipped_count = Some(0);
        view.unsupported_count = Some(0);
        view.failed_count = Some(0);
        view.reason_code = Some("dry_run".to_owned());
        view.actions = vec![scope.ready_action().to_owned()];
        return Ok(view);
    }

    let started_at = unix_now();
    let receipt = match fetcher(&config.transport.nostr_relay_urls, scope.filter()) {
        Ok(receipt) if receipt.connected_relays.is_empty() && !receipt.failed_relays.is_empty() => {
            let target_transport_endpoints = receipt.target_relays;
            let failed_transport_targets = relay_failures(receipt.failed_relays);
            let reason = relay_failure_reason(&failed_transport_targets);
            let failure_reason = format!("relay transport fetch failed: {reason}");
            let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
            migrations::run_all_up(&executor)?;
            record_sync_run(
                &executor,
                &sync_record_from_failure(
                    scope,
                    &config.transport.nostr_relay_urls,
                    target_transport_endpoints.clone(),
                    failed_transport_targets.clone(),
                    started_at,
                    failure_reason.clone(),
                )?,
            )?;
            let mut view = empty_action_from_snapshot(snapshot, "pull");
            view.state = "unavailable".to_owned();
            view.reason = Some(failure_reason);
            view.reason_code = Some("relay_fetch_failed".to_owned());
            view.target_transport_endpoints = target_transport_endpoints;
            view.failed_transport_targets = failed_transport_targets;
            view.freshness = freshness_for_scope_from_executor(config, &executor, scope)?;
            return Ok(view);
        }
        Ok(receipt) => receipt,
        Err(error) => {
            let failure_reason = error.to_string();
            let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
            migrations::run_all_up(&executor)?;
            record_sync_run(
                &executor,
                &sync_record_from_failure(
                    scope,
                    &config.transport.nostr_relay_urls,
                    config.transport.nostr_relay_urls.clone(),
                    Vec::new(),
                    started_at,
                    failure_reason.clone(),
                )?,
            )?;
            let mut view = empty_action_from_snapshot(snapshot, "pull");
            view.state = "unavailable".to_owned();
            view.reason = Some(failure_reason);
            view.reason_code = Some("relay_fetch_failed".to_owned());
            view.target_transport_endpoints = config.transport.nostr_relay_urls.clone();
            view.freshness = freshness_for_scope_from_executor(config, &executor, scope)?;
            return Ok(view);
        }
    };

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    let ingest = ingest_events(&executor, &receipt, scope)?;
    record_sync_run(
        &executor,
        &sync_record_from_ingest(
            scope,
            &config.transport.nostr_relay_urls,
            &receipt,
            &ingest,
            started_at,
        )?,
    )?;
    let failed_transport_targets = relay_failures(receipt.failed_relays);
    let failed_count = ingest.failed_count + failed_transport_targets.len();
    let reason_code =
        relay_ingest_reason_code(&ingest, &failed_transport_targets).map(str::to_owned);
    let reason = relay_ingest_reason(&ingest, &failed_transport_targets);
    let freshness = freshness_for_scope_from_executor(config, &executor, scope)?;
    let queue = radroots_replica_sync_status(&executor)?;

    Ok(SyncActionView {
        direction: "pull".to_owned(),
        state: "ready".to_owned(),
        source: INGEST_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "ready".to_owned(),
        configured_transport_target_count: config.transport.nostr_relay_urls.len(),
        transport_statuses: Vec::new(),
        publish_policy: "any".to_owned(),
        freshness,
        queue: derived_projection_sync_queue(queue.expected_count, queue.pending_count),
        target_transport_endpoints: receipt.target_relays,
        attempted_transport_endpoints: receipt.connected_relays,
        accepted_transport_endpoints: Vec::new(),
        failed_transport_targets,
        fetched_count: Some(ingest.fetched_count),
        ingested_count: Some(ingest.ingested_count),
        publishable_count: None,
        published_count: None,
        skipped_count: Some(ingest.skipped_count),
        unsupported_count: Some(ingest.unsupported_count),
        failed_count: Some(failed_count),
        publish_plan: None,
        reason_code,
        reason,
        actions: vec![scope.ready_action().to_owned()],
    })
}

pub fn push(config: &RuntimeConfig) -> Result<SyncActionView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    if config.output.dry_run {
        let status = session.block_on(session.sdk().sync().status(SyncStatusRequest::new()))?;
        return Ok(sdk_push_dry_run_view(config, status));
    }

    let receipt = session.block_on(session.sdk().sync().push_outbox(
        PushOutboxRequest::new().with_nostr_relay_url_policy(sdk_nostr_relay_url_policy(config)),
    ))?;
    let status = session.block_on(session.sdk().sync().status(SyncStatusRequest::new()))?;
    Ok(sdk_push_view(config, receipt, status))
}

pub fn watch(config: &RuntimeConfig, args: &SyncWatchArgs) -> Result<SyncWatchView, RuntimeError> {
    if args.frames == 0 {
        return Err(RuntimeError::Config(
            "`sync watch --frames` must be greater than 0".to_owned(),
        ));
    }

    let mut frames = Vec::with_capacity(args.frames);
    let mut last_snapshot = None;

    for index in 0..args.frames {
        let snapshot = inspect_sync(config)?;
        frames.push(SyncWatchFrameView {
            sequence: index + 1,
            observed_at: unix_now(),
            state: snapshot.state.clone(),
            configured_transport_target_count: snapshot.configured_transport_target_count,
            freshness: snapshot.freshness.clone(),
            queue: snapshot.queue.clone(),
        });
        last_snapshot = Some(snapshot);

        if index + 1 < args.frames {
            thread::sleep(Duration::from_millis(args.interval_ms));
        }
    }

    let snapshot = last_snapshot.expect("watch frames are non-empty");
    Ok(SyncWatchView {
        state: snapshot.state,
        source: snapshot.source,
        interval_ms: args.interval_ms,
        frames,
        reason: snapshot.reason,
        actions: snapshot.actions,
    })
}

fn empty_action_from_snapshot(snapshot: SyncSnapshot, direction: &str) -> SyncActionView {
    SyncActionView {
        direction: direction.to_owned(),
        state: snapshot.state,
        source: snapshot.source,
        local_root: snapshot.local_root,
        replica_db: snapshot.replica_db,
        configured_transport_target_count: snapshot.configured_transport_target_count,
        transport_statuses: Vec::new(),
        publish_policy: snapshot.publish_policy,
        freshness: snapshot.freshness,
        queue: snapshot.queue,
        target_transport_endpoints: Vec::new(),
        attempted_transport_endpoints: Vec::new(),
        accepted_transport_endpoints: Vec::new(),
        failed_transport_targets: Vec::new(),
        fetched_count: None,
        ingested_count: None,
        publishable_count: None,
        published_count: None,
        skipped_count: None,
        unsupported_count: None,
        failed_count: None,
        publish_plan: None,
        reason_code: None,
        reason: snapshot.reason,
        actions: snapshot.actions,
    }
}

fn sdk_sync_status_view(config: &RuntimeConfig, receipt: SyncStatusReceipt) -> SyncStatusView {
    let actions = sdk_sync_status_actions(&receipt);
    let configured_transport_target_count =
        receipt.transport_profile.configured_transport_target_count;
    let configured_transport_targets = sdk_transport_targets(&receipt);
    let transport_statuses = sdk_transport_statuses(&receipt);
    SyncStatusView {
        state: "ready".to_owned(),
        source: SDK_SYNC_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "derived_projection_not_checked".to_owned(),
        configured_transport_target_count,
        configured_transport_targets,
        transport_statuses,
        publish_policy: "any".to_owned(),
        freshness: sdk_sync_freshness(&receipt),
        queue: sdk_sync_queue(&receipt),
        reason: None,
        actions,
    }
}

fn sdk_push_dry_run_view(config: &RuntimeConfig, status: SyncStatusReceipt) -> SyncActionView {
    let publishable_count = usize_from_i64(status.outbox.ready_signed_events);
    let state = if publishable_count > 0 {
        "dry_run"
    } else {
        "ready"
    };
    let reason = if publishable_count > 0 {
        Some("dry run requested; SDK outbox push skipped".to_owned())
    } else if status.outbox.total_events > 0 {
        Some("SDK outbox has no ready signed events to push".to_owned())
    } else {
        None
    };
    sdk_push_action_view(
        config,
        state,
        sdk_sync_queue(&status),
        sdk_sync_freshness(&status),
        status.transport_profile.configured_transport_target_count,
        sdk_transport_statuses(&status),
        status
            .transport_profile
            .configured_transport_targets
            .iter()
            .map(|target| target.endpoint_uri.clone())
            .collect(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        publishable_count,
        0,
        0,
        Some(0),
        reason,
        sdk_sync_push_actions(state, publishable_count > 0),
    )
}

fn sdk_push_view(
    config: &RuntimeConfig,
    receipt: PushOutboxReceipt,
    status: SyncStatusReceipt,
) -> SyncActionView {
    let failed_count = receipt.retryable_events + receipt.terminal_events;
    let state = if receipt.attempted_events == 0 {
        "ready"
    } else if receipt.published_events > 0 && failed_count > 0 {
        "partial"
    } else if failed_count > 0 {
        "unavailable"
    } else if receipt.published_events > 0 {
        "published"
    } else {
        "ready"
    };
    let reason = sdk_push_reason(&receipt, failed_count);
    sdk_push_action_view(
        config,
        state,
        sdk_sync_queue(&status),
        sdk_sync_freshness(&status),
        status.transport_profile.configured_transport_target_count,
        sdk_transport_statuses(&status),
        sdk_push_target_transport_endpoints(&receipt, &status),
        sdk_push_attempted_transport_endpoints(&receipt),
        sdk_push_accepted_transport_endpoints(&receipt),
        sdk_push_failed_transport_targets(&receipt),
        receipt.attempted_events,
        receipt.published_events,
        failed_count,
        Some(0),
        reason,
        sdk_sync_push_actions(state, failed_count > 0),
    )
}

fn sdk_push_action_view(
    config: &RuntimeConfig,
    state: &str,
    queue: SyncQueueView,
    freshness: SyncFreshnessView,
    configured_transport_target_count: usize,
    transport_statuses: Vec<SyncTransportStatusView>,
    target_transport_endpoints: Vec<String>,
    attempted_transport_endpoints: Vec<String>,
    accepted_transport_endpoints: Vec<String>,
    failed_transport_targets: Vec<TransportTargetFailureView>,
    publishable_count: usize,
    published_count: usize,
    failed_count: usize,
    skipped_count: Option<usize>,
    reason: Option<String>,
    actions: Vec<String>,
) -> SyncActionView {
    SyncActionView {
        direction: "push".to_owned(),
        state: state.to_owned(),
        source: SDK_PUSH_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "derived_projection_not_checked".to_owned(),
        configured_transport_target_count,
        transport_statuses,
        publish_policy: "any".to_owned(),
        freshness,
        queue,
        target_transport_endpoints,
        attempted_transport_endpoints,
        accepted_transport_endpoints,
        failed_transport_targets,
        fetched_count: None,
        ingested_count: None,
        publishable_count: Some(publishable_count),
        published_count: Some(published_count),
        skipped_count,
        unsupported_count: Some(0),
        failed_count: Some(failed_count),
        publish_plan: None,
        reason_code: sdk_sync_push_reason_code(state).map(str::to_owned),
        reason,
        actions,
    }
}

fn sdk_transport_targets(receipt: &SyncStatusReceipt) -> Vec<SyncTransportTargetView> {
    receipt
        .transport_profile
        .configured_transport_targets
        .iter()
        .map(|target| SyncTransportTargetView {
            transport_kind: target.transport_kind.clone(),
            endpoint_uri: target.endpoint_uri.clone(),
            endpoint_fingerprint: target.endpoint_fingerprint.clone(),
        })
        .collect()
}

fn sdk_transport_statuses(receipt: &SyncStatusReceipt) -> Vec<SyncTransportStatusView> {
    receipt
        .transport_profile
        .transport_statuses
        .iter()
        .map(|status| SyncTransportStatusView {
            transport_kind: status.transport_kind.clone(),
            profile_id: status.profile_id.clone(),
            endpoint_uri: status.endpoint_uri.clone(),
            implementation_state: status.implementation_state.clone(),
            readiness: status.readiness.clone(),
            publish_usable: status.publish_usable,
            fetch_usable: status.fetch_usable,
            redacted_message: status.redacted_message.clone(),
        })
        .collect()
}

fn sdk_sync_status_actions(receipt: &SyncStatusReceipt) -> Vec<String> {
    let mut actions = Vec::new();
    if receipt.outbox.ready_signed_events > 0 {
        actions.push(SYNC_PUSH_ACTION.to_owned());
    }
    if receipt.event_store.total_events == 0 {
        actions.push(SYNC_PULL_ACTION.to_owned());
    }
    actions
}

fn sdk_sync_push_actions(state: &str, retryable: bool) -> Vec<String> {
    match state {
        "published" | "ready" => vec!["radroots sync status get".to_owned()],
        "dry_run" | "partial" | "unavailable" if retryable => {
            vec![
                SYNC_PUSH_ACTION.to_owned(),
                "radroots sync status get".to_owned(),
            ]
        }
        _ => vec!["radroots sync status get".to_owned()],
    }
}

fn sdk_sync_push_reason_code(state: &str) -> Option<&'static str> {
    match state {
        "dry_run" => Some("dry_run"),
        "partial" => Some("sdk_outbox_push_partial"),
        "unavailable" => Some("sdk_outbox_push_failed"),
        _ => None,
    }
}

fn sdk_push_reason(receipt: &PushOutboxReceipt, failed_count: usize) -> Option<String> {
    if receipt.attempted_events == 0 {
        return Some("SDK outbox had no ready signed events to push".to_owned());
    }
    if failed_count > 0 && receipt.published_events > 0 {
        return Some(format!(
            "SDK outbox push published {} event(s); {failed_count} event(s) remain retryable or terminal",
            receipt.published_events
        ));
    }
    if failed_count > 0 {
        return Some(
            "SDK outbox push did not reach accepted quorum for any ready event".to_owned(),
        );
    }
    None
}

fn sdk_sync_queue(receipt: &SyncStatusReceipt) -> SyncQueueView {
    let pending_count = usize_from_i64(
        receipt
            .outbox
            .pending_events
            .saturating_add(receipt.outbox.retryable_events),
    );
    SyncQueueView {
        expected_count: usize_from_i64(receipt.outbox.total_events),
        pending_count,
        total_count: Some(usize_from_i64(receipt.outbox.total_events)),
        retryable_count: Some(usize_from_i64(receipt.outbox.retryable_events)),
        terminal_count: Some(usize_from_i64(receipt.outbox.terminal_events)),
        failed_terminal_count: Some(usize_from_i64(receipt.outbox.failed_terminal_events)),
        preview_unavailable_count: Some(usize_from_i64(receipt.outbox.preview_unavailable_events)),
        deferred_until_implemented_count: Some(usize_from_i64(
            receipt.outbox.deferred_until_implemented_events,
        )),
        ready_signed_count: Some(usize_from_i64(receipt.outbox.ready_signed_events)),
        publishing_count: Some(usize_from_i64(receipt.outbox.publishing_events)),
        last_attempt_at_ms: receipt.outbox.last_attempt_at_ms,
        last_error: receipt.outbox.last_error.clone(),
    }
}

fn derived_projection_sync_queue(expected_count: usize, pending_count: usize) -> SyncQueueView {
    SyncQueueView {
        expected_count,
        pending_count,
        total_count: None,
        retryable_count: None,
        terminal_count: None,
        failed_terminal_count: None,
        preview_unavailable_count: None,
        deferred_until_implemented_count: None,
        ready_signed_count: None,
        publishing_count: None,
        last_attempt_at_ms: None,
        last_error: None,
    }
}

fn sdk_sync_freshness(receipt: &SyncStatusReceipt) -> SyncFreshnessView {
    let Some(last_event_updated_at_ms) = receipt.event_store.last_event_updated_at_ms else {
        return missing_freshness();
    };
    let last_event_at = u64::try_from(last_event_updated_at_ms / 1_000).unwrap_or(0);
    let observed_at = u64::try_from(receipt.observed_at_ms / 1_000).unwrap_or_else(|_| unix_now());
    let age_seconds = observed_at.saturating_sub(last_event_at);
    SyncFreshnessView {
        state: "synced".to_owned(),
        display: format!("SDK event store updated {}", relative_age(age_seconds)),
        age_seconds: Some(age_seconds),
        last_event_at: Some(last_event_at),
        run: None,
    }
}

fn sdk_push_target_transport_endpoints(
    receipt: &PushOutboxReceipt,
    status: &SyncStatusReceipt,
) -> Vec<String> {
    let mut targets = Vec::new();
    for target in receipt.events.iter().flat_map(|event| event.targets.iter()) {
        if !targets.contains(&target.endpoint_uri) {
            targets.push(target.endpoint_uri.clone());
        }
    }
    if targets.is_empty() {
        targets.extend(
            status
                .transport_profile
                .configured_transport_targets
                .iter()
                .map(|target| target.endpoint_uri.clone()),
        );
    }
    targets
}

fn sdk_push_attempted_transport_endpoints(receipt: &PushOutboxReceipt) -> Vec<String> {
    sdk_push_targets_matching(receipt, |_, target| target.attempted)
}

fn sdk_push_accepted_transport_endpoints(receipt: &PushOutboxReceipt) -> Vec<String> {
    sdk_push_targets_matching(receipt, |_, target| {
        sdk_target_accepted(target.outcome_kind)
    })
}

fn sdk_push_targets_matching(
    receipt: &PushOutboxReceipt,
    predicate: impl Fn(&PushOutboxEventReceipt, &PushOutboxTargetReceipt) -> bool,
) -> Vec<String> {
    let mut targets = Vec::new();
    for event in &receipt.events {
        for target in &event.targets {
            if predicate(event, target) && !targets.contains(&target.endpoint_uri) {
                targets.push(target.endpoint_uri.clone());
            }
        }
    }
    targets
}

fn sdk_push_failed_transport_targets(
    receipt: &PushOutboxReceipt,
) -> Vec<TransportTargetFailureView> {
    receipt
        .events
        .iter()
        .flat_map(|event| event.targets.iter())
        .filter(|target| !sdk_target_accepted(target.outcome_kind))
        .map(|target| TransportTargetFailureView {
            transport_kind: target.transport_kind.clone(),
            endpoint_uri: target.endpoint_uri.clone(),
            reason: target
                .message
                .clone()
                .unwrap_or_else(|| sdk_target_outcome_kind(target.outcome_kind).to_owned()),
        })
        .collect()
}

fn sdk_target_accepted(kind: PushOutboxTargetOutcomeKind) -> bool {
    matches!(
        kind,
        PushOutboxTargetOutcomeKind::Accepted | PushOutboxTargetOutcomeKind::DuplicateAccepted
    )
}

fn sdk_target_outcome_kind(kind: PushOutboxTargetOutcomeKind) -> &'static str {
    match kind {
        PushOutboxTargetOutcomeKind::Accepted => "accepted",
        PushOutboxTargetOutcomeKind::DuplicateAccepted => "duplicate_accepted",
        PushOutboxTargetOutcomeKind::Blocked => "blocked",
        PushOutboxTargetOutcomeKind::RateLimited => "rate_limited",
        PushOutboxTargetOutcomeKind::Invalid => "invalid",
        PushOutboxTargetOutcomeKind::PowRequired => "pow_required",
        PushOutboxTargetOutcomeKind::Restricted => "restricted",
        PushOutboxTargetOutcomeKind::AuthRequired => "auth_required",
        PushOutboxTargetOutcomeKind::Error => "error",
        PushOutboxTargetOutcomeKind::Timeout => "timeout",
        PushOutboxTargetOutcomeKind::ConnectionFailed => "connection_failed",
        PushOutboxTargetOutcomeKind::TargetUriRejected => "target_uri_rejected",
        PushOutboxTargetOutcomeKind::Unknown => "unknown",
        _ => "unknown",
    }
}

fn usize_from_i64(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

fn inspect_sync(config: &RuntimeConfig) -> Result<SyncSnapshot, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(SyncSnapshot {
            state: "unconfigured".to_owned(),
            source: SYNC_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_db: "missing".to_owned(),
            configured_transport_target_count: config.transport.nostr_relay_urls.len(),
            publish_policy: "any".to_owned(),
            freshness: missing_freshness(),
            queue: derived_projection_sync_queue(0, 0),
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        });
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    let queue = radroots_replica_sync_status(&executor)?;
    let freshness =
        freshness_for_scope_from_executor(config, &executor, RelayIngestScope::SyncPull)?;
    let configured_transport_target_count = config.transport.nostr_relay_urls.len();
    let publish_policy = "any".to_owned();
    let mut actions = Vec::new();

    if configured_transport_target_count == 0 {
        actions.push(RELAY_PULL_SETUP_ACTION.to_owned());
        return Ok(SyncSnapshot {
            state: "unconfigured".to_owned(),
            source: SYNC_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_db: "ready".to_owned(),
            configured_transport_target_count,
            publish_policy,
            freshness,
            queue: derived_projection_sync_queue(queue.expected_count, queue.pending_count),
            reason: Some("no relays are configured for this operator session".to_owned()),
            actions,
        });
    }

    actions.push(SYNC_PULL_ACTION.to_owned());
    if queue.pending_count > 0 {
        actions.push(SYNC_PUSH_ACTION.to_owned());
    }

    Ok(SyncSnapshot {
        state: "ready".to_owned(),
        source: SYNC_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "ready".to_owned(),
        configured_transport_target_count,
        publish_policy,
        freshness,
        queue: derived_projection_sync_queue(queue.expected_count, queue.pending_count),
        reason: None,
        actions,
    })
}

pub(crate) fn missing_freshness() -> SyncFreshnessView {
    SyncFreshnessView {
        state: "never".to_owned(),
        display: "never synced".to_owned(),
        age_seconds: None,
        last_event_at: None,
        run: None,
    }
}

#[cfg(test)]
pub(crate) fn freshness_for_scope(
    config: &RuntimeConfig,
    scope: RelayIngestScope,
) -> Result<SyncFreshnessView, RuntimeError> {
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    freshness_for_scope_from_executor(config, &executor, scope)
}

pub(crate) fn relay_provenance_relays_for_scope(
    config: &RuntimeConfig,
    scope: RelayIngestScope,
) -> Result<Vec<String>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(Vec::new());
    }
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    ensure_sync_run_table(&executor)?;
    let current_fingerprint = relay_set_fingerprint(&config.transport.nostr_relay_urls);
    let Some(run) = latest_sync_run(&executor, scope)? else {
        return Ok(Vec::new());
    };
    if run.relay_set_fingerprint != current_fingerprint || !sync_run_successful(&run) {
        return Ok(Vec::new());
    }
    let mut relays: Vec<String> =
        serde_json::from_str(run.attempted_transport_endpoints_json.as_str())?;
    relays.sort();
    relays.dedup();
    Ok(relays)
}

pub(crate) fn freshness_for_scope_from_executor(
    config: &RuntimeConfig,
    executor: &SqliteExecutor,
    scope: RelayIngestScope,
) -> Result<SyncFreshnessView, RuntimeError> {
    let last_event_at = ReplicaSql::new(executor).nostr_event_last_created_at()?;
    let now = unix_now();
    let age_seconds = last_event_at.map(|last_event_at| now.saturating_sub(last_event_at));
    ensure_sync_run_table(executor)?;
    let current_fingerprint = relay_set_fingerprint(&config.transport.nostr_relay_urls);
    let latest = latest_sync_run(executor, scope)?;
    let current = latest
        .as_ref()
        .filter(|run| run.relay_set_fingerprint == current_fingerprint);
    let last_success = current.filter(|run| sync_run_successful(run));
    let state = freshness_state(scope, latest.as_ref(), current, last_success, age_seconds);
    let display = freshness_display(scope, state.as_str(), age_seconds, current);

    Ok(SyncFreshnessView {
        state,
        display,
        age_seconds,
        last_event_at,
        run: latest.map(|run| sync_run_freshness_view(scope, run, current_fingerprint)),
    })
}

pub(crate) fn freshness_requires_refresh(freshness: &SyncFreshnessView) -> bool {
    matches!(
        freshness.state.as_str(),
        "never" | "stale" | "relay_set_changed" | "refresh_failed"
    )
}

fn freshness_state(
    scope: RelayIngestScope,
    latest: Option<&SyncRunRecord>,
    current: Option<&SyncRunRecord>,
    last_success: Option<&SyncRunRecord>,
    age_seconds: Option<u64>,
) -> String {
    let Some(latest) = latest else {
        return "never".to_owned();
    };
    let Some(current) = current else {
        return "relay_set_changed".to_owned();
    };
    if !sync_run_successful(current) {
        return "refresh_failed".to_owned();
    }
    if last_success.is_none() {
        return "refresh_failed".to_owned();
    }
    if age_seconds.is_none() {
        return "fresh".to_owned();
    }
    if age_seconds.unwrap_or_default() > scope.stale_after_seconds() {
        return "stale".to_owned();
    }
    if latest.state == "partial" {
        return "partial".to_owned();
    }
    "fresh".to_owned()
}

fn freshness_display(
    scope: RelayIngestScope,
    state: &str,
    age_seconds: Option<u64>,
    run: Option<&SyncRunRecord>,
) -> String {
    match state {
        "fresh" => match age_seconds {
            Some(age_seconds) => format!("{} fresh {}", scope.display(), relative_age(age_seconds)),
            None => format!("{} fresh; no market events yet", scope.display()),
        },
        "partial" => match age_seconds {
            Some(age_seconds) => format!(
                "{} partially refreshed {}",
                scope.display(),
                relative_age(age_seconds)
            ),
            None => format!(
                "{} partially refreshed; no market events yet",
                scope.display()
            ),
        },
        "stale" => match age_seconds {
            Some(age_seconds) => format!("{} stale {}", scope.display(), relative_age(age_seconds)),
            None => format!("{} stale", scope.display()),
        },
        "relay_set_changed" => format!("{} relay set changed; refresh required", scope.display()),
        "refresh_failed" => run
            .and_then(|run| run.failure_reason.clone())
            .unwrap_or_else(|| format!("{} refresh failed", scope.display())),
        _ => format!("{} never synced", scope.display()),
    }
}

fn sync_run_successful(run: &SyncRunRecord) -> bool {
    matches!(run.state.as_str(), "success" | "partial")
}

fn sync_run_freshness_view(
    scope: RelayIngestScope,
    run: SyncRunRecord,
    current_fingerprint: String,
) -> SyncRunFreshnessView {
    let relay_set_current = run.relay_set_fingerprint == current_fingerprint;
    let successful = sync_run_successful(&run);
    let last_successful_at = successful.then_some(run.completed_at.unwrap_or(run.started_at));
    SyncRunFreshnessView {
        scope: run.scope,
        relay_set_fingerprint: run.relay_set_fingerprint,
        relay_set_current,
        last_state: run.state,
        last_attempted_at: Some(run.started_at),
        last_successful_at,
        last_completed_at: run.completed_at,
        stale_after_seconds: Some(scope.stale_after_seconds()),
        fetched_count: Some(run.fetched_count),
        ingested_count: Some(run.ingested_count),
        skipped_count: Some(run.skipped_count),
        unsupported_count: Some(run.unsupported_count),
        failed_count: Some(run.failed_count),
        failure_reason: run.failure_reason,
    }
}

pub(crate) fn ensure_sync_run_table(executor: &SqliteExecutor) -> Result<(), RuntimeError> {
    executor.exec(
        "CREATE TABLE IF NOT EXISTS radroots_cli_sync_run (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scope TEXT NOT NULL,
            relay_set_fingerprint TEXT NOT NULL,
            target_transport_endpoints_json TEXT NOT NULL,
            attempted_transport_endpoints_json TEXT NOT NULL,
            failed_transport_targets_json TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            completed_at INTEGER,
            state TEXT NOT NULL,
            fetched_count INTEGER NOT NULL,
            ingested_count INTEGER NOT NULL,
            skipped_count INTEGER NOT NULL,
            unsupported_count INTEGER NOT NULL,
            failed_count INTEGER NOT NULL,
            failure_reason TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_radroots_cli_sync_run_scope_started
            ON radroots_cli_sync_run(scope, started_at DESC);",
        "[]",
    )?;
    Ok(())
}

fn latest_sync_run(
    executor: &SqliteExecutor,
    scope: RelayIngestScope,
) -> Result<Option<SyncRunRecord>, RuntimeError> {
    let rows = executor.query_raw(
        &format!(
            "SELECT scope,
                    relay_set_fingerprint,
                    target_transport_endpoints_json,
                    attempted_transport_endpoints_json,
                    failed_transport_targets_json,
                    started_at,
                    completed_at,
                    state,
                    fetched_count,
                    ingested_count,
                    skipped_count,
                    unsupported_count,
                    failed_count,
                    failure_reason
             FROM {SYNC_RUN_TABLE}
             WHERE scope = ?1
             ORDER BY started_at DESC, id DESC
             LIMIT 1"
        ),
        json!([scope.id()]).to_string().as_str(),
    )?;
    let mut rows: Vec<SyncRunRow> = serde_json::from_str(rows.as_str())?;
    Ok(rows.pop().map(sync_run_record_from_row))
}

fn sync_run_record_from_row(row: SyncRunRow) -> SyncRunRecord {
    SyncRunRecord {
        scope: row.scope,
        relay_set_fingerprint: row.relay_set_fingerprint,
        target_transport_endpoints_json: row.target_transport_endpoints_json,
        attempted_transport_endpoints_json: row.attempted_transport_endpoints_json,
        failed_transport_targets_json: row.failed_transport_targets_json,
        started_at: u64_from_db(row.started_at),
        completed_at: row.completed_at.map(u64_from_db),
        state: row.state,
        fetched_count: usize_from_db(row.fetched_count),
        ingested_count: usize_from_db(row.ingested_count),
        skipped_count: usize_from_db(row.skipped_count),
        unsupported_count: usize_from_db(row.unsupported_count),
        failed_count: usize_from_db(row.failed_count),
        failure_reason: row.failure_reason,
    }
}

fn record_sync_run(executor: &SqliteExecutor, record: &SyncRunRecord) -> Result<(), RuntimeError> {
    ensure_sync_run_table(executor)?;
    executor.exec(
        &format!(
            "INSERT INTO {SYNC_RUN_TABLE} (
                scope,
                relay_set_fingerprint,
                target_transport_endpoints_json,
                attempted_transport_endpoints_json,
                failed_transport_targets_json,
                started_at,
                completed_at,
                state,
                fetched_count,
                ingested_count,
                skipped_count,
                unsupported_count,
                failed_count,
                failure_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
        ),
        json!([
            record.scope.as_str(),
            record.relay_set_fingerprint.as_str(),
            record.target_transport_endpoints_json.as_str(),
            record.attempted_transport_endpoints_json.as_str(),
            record.failed_transport_targets_json.as_str(),
            i64_from_u64(record.started_at),
            record.completed_at.map(i64_from_u64),
            record.state.as_str(),
            i64_from_usize(record.fetched_count),
            i64_from_usize(record.ingested_count),
            i64_from_usize(record.skipped_count),
            i64_from_usize(record.unsupported_count),
            i64_from_usize(record.failed_count),
            record.failure_reason.as_deref(),
        ])
        .to_string()
        .as_str(),
    )?;
    Ok(())
}

fn sync_record_from_failure(
    scope: RelayIngestScope,
    relays: &[String],
    target_transport_endpoints: Vec<String>,
    failed_transport_targets: Vec<TransportTargetFailureView>,
    started_at: u64,
    reason: String,
) -> Result<SyncRunRecord, RuntimeError> {
    Ok(SyncRunRecord {
        scope: scope.id().to_owned(),
        relay_set_fingerprint: relay_set_fingerprint(relays),
        target_transport_endpoints_json: serde_json::to_string(&target_transport_endpoints)?,
        attempted_transport_endpoints_json: serde_json::to_string(&Vec::<String>::new())?,
        failed_transport_targets_json: serde_json::to_string(&failed_transport_targets)?,
        started_at,
        completed_at: Some(unix_now()),
        state: "failed".to_owned(),
        fetched_count: 0,
        ingested_count: 0,
        skipped_count: 0,
        unsupported_count: 0,
        failed_count: 1,
        failure_reason: Some(reason),
    })
}

fn sync_record_from_ingest(
    scope: RelayIngestScope,
    relays: &[String],
    receipt: &RadrootsRelayFetchedEventsReceipt,
    ingest: &RelayIngestCounts,
    started_at: u64,
) -> Result<SyncRunRecord, RuntimeError> {
    let failed_transport_targets = relay_failures(receipt.failed_relays.clone());
    let state = if ingest.failed_count > 0 || !failed_transport_targets.is_empty() {
        "partial"
    } else {
        "success"
    };
    Ok(SyncRunRecord {
        scope: scope.id().to_owned(),
        relay_set_fingerprint: relay_set_fingerprint(relays),
        target_transport_endpoints_json: serde_json::to_string(&receipt.target_relays)?,
        attempted_transport_endpoints_json: serde_json::to_string(&receipt.connected_relays)?,
        failed_transport_targets_json: serde_json::to_string(&failed_transport_targets)?,
        started_at,
        completed_at: Some(unix_now()),
        state: state.to_owned(),
        fetched_count: ingest.fetched_count,
        ingested_count: ingest.ingested_count,
        skipped_count: ingest.skipped_count,
        unsupported_count: ingest.unsupported_count,
        failed_count: ingest.failed_count + failed_transport_targets.len(),
        failure_reason: ingest.reason(),
    })
}

fn relay_set_fingerprint(relays: &[String]) -> String {
    let mut normalized = relays
        .iter()
        .map(|relay| relay.trim().to_ascii_lowercase())
        .filter(|relay| !relay.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    let mut hash = 0xcbf29ce484222325_u64;
    for relay in normalized {
        for byte in relay.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("relayset_{hash:016x}")
}

fn u64_from_db(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn usize_from_db(value: i64) -> usize {
    usize::try_from(value).unwrap_or_default()
}

fn i64_from_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn i64_from_usize(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[derive(Debug, Clone, Default)]
struct RelayIngestCounts {
    fetched_count: usize,
    ingested_count: usize,
    skipped_count: usize,
    unsupported_count: usize,
    failed_count: usize,
    first_failure_reason: Option<String>,
}

impl RelayIngestCounts {
    fn reason_code(&self) -> Option<&'static str> {
        if self.failed_count > 0 {
            Some("sync_ingest_failed")
        } else if self.skipped_count > 0 {
            Some("sync_no_overwrite")
        } else {
            None
        }
    }

    fn reason(&self) -> Option<String> {
        if self.failed_count > 0 {
            return Some(match &self.first_failure_reason {
                Some(reason) => format!(
                    "{} fetched event(s) failed ingest: {}",
                    self.failed_count, reason
                ),
                None => format!("{} fetched event(s) failed ingest", self.failed_count),
            });
        }
        if self.skipped_count > 0 {
            return Some(format!(
                "{} fetched event(s) skipped because the local replica already has current or newer state",
                self.skipped_count
            ));
        }
        None
    }
}

fn relay_ingest_reason_code(
    ingest: &RelayIngestCounts,
    failed_transport_targets: &[TransportTargetFailureView],
) -> Option<&'static str> {
    ingest
        .reason_code()
        .or_else(|| (!failed_transport_targets.is_empty()).then_some("relay_fetch_partial"))
}

fn relay_ingest_reason(
    ingest: &RelayIngestCounts,
    failed_transport_targets: &[TransportTargetFailureView],
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(reason) = ingest.reason() {
        parts.push(reason);
    }
    if !failed_transport_targets.is_empty() {
        parts.push(format!(
            "{} relay(s) failed during fetch: {}",
            failed_transport_targets.len(),
            relay_failure_reason(failed_transport_targets)
        ));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn relay_failure_reason(failed_transport_targets: &[TransportTargetFailureView]) -> String {
    failed_transport_targets
        .iter()
        .map(|failure| format!("{}: {}", failure.endpoint_uri, failure.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RelayIngestScope {
    SyncPull,
    MarketRefresh,
}

impl RelayIngestScope {
    fn id(self) -> &'static str {
        match self {
            Self::SyncPull => "sync_pull",
            Self::MarketRefresh => "market_refresh",
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::SyncPull => "sync pull",
            Self::MarketRefresh => "market refresh",
        }
    }

    fn stale_after_seconds(self) -> u64 {
        match self {
            Self::SyncPull => SYNC_PULL_FRESHNESS_STALE_AFTER_SECONDS,
            Self::MarketRefresh => MARKET_FRESHNESS_STALE_AFTER_SECONDS,
        }
    }

    fn kinds(self) -> &'static [u32] {
        match self {
            Self::SyncPull => SYNC_PULL_KINDS,
            Self::MarketRefresh => MARKET_REFRESH_KINDS,
        }
    }

    fn filter(self) -> RadrootsNostrFilter {
        RadrootsNostrFilter::new()
            .kinds(
                self.kinds()
                    .iter()
                    .copied()
                    .map(|kind| radroots_nostr_kind(kind as u16)),
            )
            .limit(RELAY_FETCH_LIMIT)
    }

    fn ready_action(self) -> &'static str {
        match self {
            Self::SyncPull => SYNC_READY_ACTION,
            Self::MarketRefresh => MARKET_READY_ACTION,
        }
    }

    fn supports_kind(self, kind: u32) -> bool {
        self.kinds().contains(&kind)
    }
}

fn ingest_events(
    executor: &SqliteExecutor,
    receipt: &RadrootsRelayFetchedEventsReceipt,
    scope: RelayIngestScope,
) -> Result<RelayIngestCounts, RuntimeError> {
    let mut counts = RelayIngestCounts {
        fetched_count: receipt.events.len(),
        ..RelayIngestCounts::default()
    };

    for event in &receipt.events {
        if !scope.supports_kind(event_kind(&event.event)) {
            counts.unsupported_count += 1;
            continue;
        }
        let event = radroots_event_from_nostr(&event.event);
        match radroots_replica_ingest_event(executor, &event) {
            Ok(RadrootsReplicaIngestOutcome::Applied) => counts.ingested_count += 1,
            Ok(RadrootsReplicaIngestOutcome::Skipped) => counts.skipped_count += 1,
            Err(error @ RadrootsReplicaEventsError::Sql(_)) => return Err(error.into()),
            Err(error) => {
                counts.failed_count += 1;
                if counts.first_failure_reason.is_none() {
                    counts.first_failure_reason = Some(error.to_string());
                }
            }
        }
    }

    Ok(counts)
}

fn event_kind(event: &radroots_nostr::prelude::RadrootsNostrEvent) -> u32 {
    u32::from(event.kind.as_u16())
}

fn relay_failures(failures: Vec<RadrootsRelayFetchFailure>) -> Vec<TransportTargetFailureView> {
    failures
        .into_iter()
        .map(|failure| TransportTargetFailureView {
            transport_kind: "nostr".to_owned(),
            endpoint_uri: failure.relay_url,
            reason: failure.reason,
        })
        .collect()
}

fn merge_fetch_receipt(
    target: &mut Option<RadrootsRelayFetchedEventsReceipt>,
    receipt: RadrootsRelayFetchedEventsReceipt,
) {
    match target {
        Some(target) => {
            push_unique_many(&mut target.target_relays, receipt.target_relays.iter());
            push_unique_many(
                &mut target.connected_relays,
                receipt.connected_relays.iter(),
            );
            for failure in receipt.failed_relays {
                if !target
                    .failed_relays
                    .iter()
                    .any(|existing| existing.relay_url == failure.relay_url)
                {
                    target.failed_relays.push(failure);
                }
            }
            target.events.extend(receipt.events);
            target.event_receipts.extend(receipt.event_receipts);
            target.malformed_count += receipt.malformed_count;
            target.out_of_filter_count += receipt.out_of_filter_count;
            target.skipped_over_limit_count += receipt.skipped_over_limit_count;
            target.eose_count += receipt.eose_count;
            target.closed_count += receipt.closed_count;
            target.notice_count += receipt.notice_count;
            target.relay_outcomes.extend(receipt.relay_outcomes);
        }
        None => *target = Some(receipt),
    }
}

fn push_unique_many<'a>(target: &mut Vec<String>, values: impl Iterator<Item = &'a String>) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unix_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn relative_age(age_seconds: u64) -> String {
    match age_seconds {
        0 => "now".to_owned(),
        1..=59 => format!("{age_seconds}s ago"),
        60..=3_599 => format!("{}m ago", age_seconds / 60),
        3_600..=86_399 => format!("{}h ago", age_seconds / 3_600),
        _ => format!("{}d ago", age_seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_events::farm::{RadrootsFarm, RadrootsFarmRef};
    use radroots_events::ids::RadrootsEventId;
    use radroots_events::kinds::{KIND_FARM, KIND_LIST_SET_GENERIC, KIND_LISTING, KIND_POST};
    use radroots_events::list::RadrootsListEntry;
    use radroots_events::list_set::RadrootsListSet;
    use radroots_events::plot::RadrootsPlot;
    use radroots_events::profile::{RadrootsProfile, RadrootsProfileType};
    use radroots_events_codec::farm::encode as farm_encode;
    use radroots_events_codec::list_set::encode as list_set_encode;
    use radroots_events_codec::plot::encode as plot_encode;
    use radroots_events_codec::profile::encode as profile_encode;
    use radroots_events_codec::wire::WireEventParts;
    use radroots_identity::RadrootsIdentity;
    use radroots_nostr::prelude::{
        RadrootsNostrEvent, RadrootsNostrFilter, RadrootsNostrTimestamp, radroots_nostr_build_event,
    };
    use radroots_sdk::{
        PushOutboxEventReceipt, PushOutboxEventState, PushOutboxReceipt,
        PushOutboxTargetOutcomeKind, PushOutboxTargetReceipt, SyncEventStoreStatus,
        SyncOutboxStatus, SyncStatusReceipt, SyncStatusSource, SyncTransportProfileSummary,
        SyncTransportStatusSummary, SyncTransportTargetSummary,
    };
    use radroots_secret_vault::RadrootsSecretBackend;
    use radroots_transport_nostr::{
        RadrootsRelayFetchFailure, RadrootsRelayFetchedEvent, RadrootsRelayFetchedEventsReceipt,
        RadrootsRelayTransportError,
    };
    use tempfile::tempdir;

    use super::{
        RelayIngestScope, freshness_for_scope, market_refresh_with_fetcher, pull_with_fetcher,
        relay_provenance_relays_for_scope, sdk_push_dry_run_view, sdk_push_view,
        sdk_sync_status_view,
    };
    use crate::cli::global::{FindQueryArgs, RecordLookupArgs};
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MycConfig, OutputConfig, OutputFormat, PathsConfig, RpcConfig,
        RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };

    const FARM_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAA";
    const PLOT_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAQ";
    const LISTING_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAg";

    #[test]
    fn sync_pull_dry_run_skips_relay_fetch() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        config.output.dry_run = true;
        crate::runtime::store::init(&config).expect("store init");

        let view = pull_with_fetcher(&config, |_, _| panic!("dry run must not fetch"))
            .expect("sync pull dry run");

        assert_eq!(view.state, "ready");
        assert_eq!(
            view.target_transport_endpoints,
            vec!["wss://relay.example.com"]
        );
        assert_eq!(view.fetched_count, Some(0));
        assert_eq!(view.ingested_count, Some(0));
        assert_eq!(view.skipped_count, Some(0));
        assert_eq!(view.unsupported_count, Some(0));
        assert_eq!(view.failed_count, Some(0));
    }

    #[test]
    fn sync_pull_no_relay_action_is_actionable() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), Vec::new());
        crate::runtime::store::init(&config).expect("store init");

        let view = pull_with_fetcher(&config, |_, _| {
            panic!("unconfigured sync pull must not fetch")
        })
        .expect("sync pull unconfigured");

        assert_eq!(view.state, "unconfigured");
        assert_eq!(
            view.actions,
            vec![
                "radroots transport profile set --kind nostr --nostr-relay wss://relay.example.com"
            ]
        );
    }

    #[test]
    fn sync_status_empty_sdk_store_reports_canonical_source() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(
            dir.path(),
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned(),
            ],
        );

        let view = sdk_sync_status_view(
            &config,
            sdk_status_receipt(
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                None,
                None,
                &["wss://relay-a.example.com", "wss://relay-b.example.com"],
            ),
        );

        assert_eq!(view.state, "ready");
        assert_eq!(view.source, "SDK canonical event store and outbox");
        assert_eq!(view.replica_db, "derived_projection_not_checked");
        assert_eq!(view.configured_transport_target_count, 2);
        assert_eq!(view.queue.total_count, Some(0));
        assert_eq!(view.queue.pending_count, 0);
        assert_eq!(view.queue.retryable_count, Some(0));
        assert_eq!(view.queue.terminal_count, Some(0));
        assert_eq!(view.queue.preview_unavailable_count, Some(0));
        assert_eq!(view.queue.deferred_until_implemented_count, Some(0));
        assert_eq!(view.queue.ready_signed_count, Some(0));
        assert_eq!(view.actions, vec!["radroots sync pull"]);
    }

    #[test]
    fn sync_status_reports_sdk_pending_retryable_and_terminal_outbox_counts() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);

        let view = sdk_sync_status_view(
            &config,
            sdk_status_receipt(
                3,
                4,
                1,
                1,
                2,
                1,
                0,
                0,
                1,
                0,
                Some(1_700_000_010_000),
                Some("auth-required: login".to_owned()),
                &["wss://relay.example.com"],
            ),
        );

        assert_eq!(view.state, "ready");
        assert_eq!(view.queue.expected_count, 4);
        assert_eq!(view.queue.pending_count, 2);
        assert_eq!(view.queue.retryable_count, Some(1));
        assert_eq!(view.queue.terminal_count, Some(2));
        assert_eq!(view.queue.failed_terminal_count, Some(1));
        assert_eq!(view.queue.preview_unavailable_count, Some(0));
        assert_eq!(view.queue.deferred_until_implemented_count, Some(0));
        assert_eq!(view.queue.ready_signed_count, Some(1));
        assert_eq!(view.queue.last_attempt_at_ms, Some(1_700_000_010_000));
        assert_eq!(
            view.queue.last_error.as_deref(),
            Some("auth-required: login")
        );
        assert_eq!(view.actions, vec!["radroots sync push"]);
    }

    #[test]
    fn sync_push_dry_run_reports_sdk_ready_outbox_plan() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        config.output.dry_run = true;

        let view = sdk_push_dry_run_view(
            &config,
            sdk_status_receipt(
                1,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                1,
                0,
                None,
                None,
                &["wss://relay.example.com"],
            ),
        );

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.source, "SDK outbox push");
        assert_eq!(view.replica_db, "derived_projection_not_checked");
        assert_eq!(
            view.target_transport_endpoints,
            vec!["wss://relay.example.com"]
        );
        assert_eq!(view.publishable_count, Some(1));
        assert_eq!(view.published_count, Some(0));
        assert_eq!(view.failed_count, Some(0));
        assert_eq!(view.reason_code.as_deref(), Some("dry_run"));
        assert_eq!(
            view.reason.as_deref(),
            Some("dry run requested; SDK outbox push skipped")
        );
        assert_eq!(
            view.actions,
            vec!["radroots sync push", "radroots sync status get"]
        );
        assert!(view.publish_plan.is_none());
    }

    #[test]
    fn sync_push_empty_queue_reports_ready_sdk_state() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), Vec::new());

        let view = sdk_push_view(
            &config,
            PushOutboxReceipt::default(),
            sdk_status_receipt(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, None, None, &[]),
        );

        assert_eq!(view.state, "ready");
        assert_eq!(view.publishable_count, Some(0));
        assert_eq!(view.published_count, Some(0));
        assert_eq!(view.failed_count, Some(0));
        assert_eq!(
            view.reason.as_deref(),
            Some("SDK outbox had no ready signed events to push")
        );
        assert_eq!(view.actions, vec!["radroots sync status get"]);
    }

    #[test]
    fn sync_push_maps_published_and_auth_required_sdk_receipts() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(
            dir.path(),
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned(),
            ],
        );
        let receipt = PushOutboxReceipt {
            attempted_events: 2,
            published_events: 1,
            retryable_events: 1,
            terminal_events: 0,
            events: vec![
                sdk_push_event(
                    "a",
                    PushOutboxEventState::Published,
                    PushOutboxTargetOutcomeKind::Accepted,
                    "wss://relay-a.example.com",
                    Some("accepted".to_owned()),
                ),
                sdk_push_event(
                    "b",
                    PushOutboxEventState::PublishRetryable,
                    PushOutboxTargetOutcomeKind::AuthRequired,
                    "wss://relay-b.example.com",
                    Some("auth-required: login".to_owned()),
                ),
            ],
        };

        let view = sdk_push_view(
            &config,
            receipt,
            sdk_status_receipt(
                2,
                2,
                0,
                1,
                1,
                0,
                0,
                0,
                0,
                0,
                Some(1_700_000_020_000),
                Some("auth-required: login".to_owned()),
                &["wss://relay-a.example.com", "wss://relay-b.example.com"],
            ),
        );

        assert_eq!(view.state, "partial");
        assert_eq!(view.publishable_count, Some(2));
        assert_eq!(view.published_count, Some(1));
        assert_eq!(view.failed_count, Some(1));
        assert_eq!(view.reason_code.as_deref(), Some("sdk_outbox_push_partial"));
        assert_eq!(
            view.target_transport_endpoints,
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned()
            ]
        );
        assert_eq!(
            view.attempted_transport_endpoints,
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned()
            ]
        );
        assert_eq!(
            view.accepted_transport_endpoints,
            vec!["wss://relay-a.example.com".to_owned()]
        );
        assert_eq!(view.failed_transport_targets.len(), 1);
        assert_eq!(
            view.failed_transport_targets[0].endpoint_uri,
            "wss://relay-b.example.com"
        );
        assert_eq!(view.failed_transport_targets[0].transport_kind, "nostr");
        assert_eq!(
            view.failed_transport_targets[0].reason,
            "auth-required: login"
        );
        assert_eq!(
            view.actions,
            vec!["radroots sync push", "radroots sync status get"]
        );
    }

    fn sdk_status_receipt(
        total_events: i64,
        outbox_total_events: i64,
        pending_events: i64,
        retryable_events: i64,
        terminal_events: i64,
        failed_terminal_events: i64,
        preview_unavailable_events: i64,
        deferred_until_implemented_events: i64,
        ready_signed_events: i64,
        publishing_events: i64,
        last_attempt_at_ms: Option<i64>,
        last_error: Option<String>,
        relays: &[&str],
    ) -> SyncStatusReceipt {
        SyncStatusReceipt {
            source: SyncStatusSource::SdkCanonicalStores,
            observed_at_ms: 1_700_000_030_000,
            event_store: SyncEventStoreStatus {
                total_events,
                projection_eligible_events: total_events,
                transport_observations: 0,
                last_event_seq: (total_events > 0).then_some(total_events),
                last_event_updated_at_ms: (total_events > 0).then_some(1_700_000_000_000),
            },
            outbox: SyncOutboxStatus {
                total_events: outbox_total_events,
                pending_events,
                retryable_events,
                terminal_events,
                failed_terminal_events,
                preview_unavailable_events,
                deferred_until_implemented_events,
                ready_signed_events,
                publishing_events,
                last_attempt_at_ms,
                last_error,
            },
            transport_profile: SyncTransportProfileSummary {
                transport_profile_id: "nostr".to_owned(),
                configured_transport_target_count: relays.len(),
                configured_transport_targets: relays
                    .iter()
                    .enumerate()
                    .map(|(index, relay)| SyncTransportTargetSummary {
                        transport_kind: "nostr".to_owned(),
                        endpoint_uri: (*relay).to_owned(),
                        endpoint_fingerprint: format!("test-fingerprint-{index}"),
                    })
                    .collect(),
                transport_statuses: vec![SyncTransportStatusSummary {
                    transport_kind: "nostr".to_owned(),
                    profile_id: Some("nostr".to_owned()),
                    endpoint_uri: None,
                    implementation_state: "available".to_owned(),
                    readiness: "ready".to_owned(),
                    publish_usable: true,
                    fetch_usable: true,
                    redacted_message: None,
                }],
            },
        }
    }

    fn sdk_push_event(
        event_id_prefix: &str,
        final_state: PushOutboxEventState,
        outcome_kind: PushOutboxTargetOutcomeKind,
        relay_url: &str,
        message: Option<String>,
    ) -> PushOutboxEventReceipt {
        PushOutboxEventReceipt {
            event_id: RadrootsEventId::parse(event_id_prefix.repeat(64).as_str())
                .expect("event id"),
            outbox_event_id: 7,
            final_state,
            attempted_count: 1,
            accepted_count: usize::from(matches!(
                outcome_kind,
                PushOutboxTargetOutcomeKind::Accepted
                    | PushOutboxTargetOutcomeKind::DuplicateAccepted
            )),
            retryable_count: usize::from(matches!(
                outcome_kind,
                PushOutboxTargetOutcomeKind::AuthRequired
                    | PushOutboxTargetOutcomeKind::Timeout
                    | PushOutboxTargetOutcomeKind::ConnectionFailed
            )),
            terminal_count: usize::from(matches!(
                outcome_kind,
                PushOutboxTargetOutcomeKind::Blocked
                    | PushOutboxTargetOutcomeKind::RateLimited
                    | PushOutboxTargetOutcomeKind::Invalid
                    | PushOutboxTargetOutcomeKind::PowRequired
                    | PushOutboxTargetOutcomeKind::Restricted
                    | PushOutboxTargetOutcomeKind::Error
                    | PushOutboxTargetOutcomeKind::Unknown
            )),
            quorum: 1,
            quorum_met: matches!(
                outcome_kind,
                PushOutboxTargetOutcomeKind::Accepted
                    | PushOutboxTargetOutcomeKind::DuplicateAccepted
            ),
            targets: vec![PushOutboxTargetReceipt {
                transport_kind: "nostr".to_owned(),
                endpoint_uri: relay_url.to_owned(),
                outcome_kind,
                attempted: true,
                message,
            }],
        }
    }

    #[test]
    fn sync_pull_ingests_relay_events_and_market_reads_without_daemon() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(7);
        let seller_pubkey = seller.public_key_hex();
        let listing_addr = format!("{KIND_LISTING}:{seller_pubkey}:{LISTING_D_TAG}");
        let events = vec![
            profile_event(&seller),
            farm_event(&seller),
            plot_event(&seller),
            listing_event(&seller),
            list_set_event(&seller),
        ];

        let view = pull_with_fetcher(&config, fake_fetcher(events)).expect("sync pull ingest");

        assert_eq!(view.state, "ready");
        assert_eq!(view.fetched_count, Some(5));
        assert_eq!(view.ingested_count, Some(5));
        assert_eq!(view.skipped_count, Some(0));
        assert_eq!(view.unsupported_count, Some(0));
        assert_eq!(view.failed_count, Some(0));
        assert_eq!(view.reason, None);

        let search = crate::runtime::find::search(
            &config,
            &FindQueryArgs {
                query: vec!["eggs".to_owned()],
            },
        )
        .expect("market search");
        assert_eq!(search.state, "ready");
        assert_eq!(search.count, 1);
        assert_eq!(
            search.results[0].listing_addr.as_deref(),
            Some(listing_addr.as_str())
        );

        let listing = crate::runtime::listing::get(
            &config,
            &RecordLookupArgs {
                key: "pasture-eggs".to_owned(),
            },
        )
        .expect("listing get");
        assert_eq!(listing.state, "ready");
        assert_eq!(listing.listing_addr.as_deref(), Some(listing_addr.as_str()));
    }

    #[test]
    fn market_refresh_uses_market_scope_for_ingest() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(8);
        let events = vec![listing_event(&seller), plot_event(&seller)];

        let view =
            market_refresh_with_fetcher(&config, fake_fetcher(events)).expect("market refresh");

        assert_eq!(view.state, "ready");
        assert_eq!(view.fetched_count, Some(2));
        assert_eq!(view.ingested_count, Some(1));
        assert_eq!(view.unsupported_count, Some(1));
        assert_eq!(view.failed_count, Some(0));
    }

    #[test]
    fn market_refresh_records_relay_provenance_relays_for_order_drafts() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(
            dir.path(),
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned(),
            ],
        );
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(9);

        let _ = market_refresh_with_fetcher(&config, fake_fetcher(vec![listing_event(&seller)]))
            .expect("market refresh");
        let relays = relay_provenance_relays_for_scope(&config, RelayIngestScope::MarketRefresh)
            .expect("relay provenance");

        assert_eq!(
            relays,
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned()
            ]
        );
    }

    #[test]
    fn relay_refresh_records_current_run_freshness() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(10);

        let view = market_refresh_with_fetcher(&config, fake_fetcher(vec![listing_event(&seller)]))
            .expect("market refresh");

        assert_eq!(view.freshness.state, "fresh");
        let run = view.freshness.run.as_ref().expect("run freshness");
        assert_eq!(run.scope, "market_refresh");
        assert_eq!(run.last_state, "success");
        assert_eq!(run.relay_set_current, true);
        assert_eq!(run.fetched_count, Some(1));
        assert_eq!(run.ingested_count, Some(1));
    }

    #[test]
    fn sync_pull_reports_partial_relay_fetch_reason_code() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(
            dir.path(),
            vec![
                "wss://relay-a.example.com".to_owned(),
                "wss://relay-b.example.com".to_owned(),
            ],
        );
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(13);

        let view = pull_with_fetcher(&config, |relays, _| {
            Ok(relay_fetch_receipt_with_failed(
                relays,
                vec![listing_event(&seller)],
                vec![RadrootsRelayFetchFailure {
                    relay_url: relays[1].clone(),
                    reason: "connection refused".to_owned(),
                }],
            ))
        })
        .expect("sync pull partial relay fetch");

        assert_eq!(view.state, "ready");
        assert_eq!(
            view.attempted_transport_endpoints,
            vec!["wss://relay-a.example.com"]
        );
        assert_eq!(view.failed_transport_targets.len(), 1);
        assert_eq!(view.failed_count, Some(1));
        assert_eq!(view.reason_code.as_deref(), Some("relay_fetch_partial"));
        assert!(
            view.reason
                .as_deref()
                .expect("partial relay reason")
                .contains("relay(s) failed during fetch")
        );
        let run = view.freshness.run.as_ref().expect("run freshness");
        assert_eq!(run.last_state, "partial");
        assert_eq!(run.failed_count, Some(1));
    }

    #[test]
    fn sync_pull_reports_no_overwrite_skips_without_replacing_projection() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(12);

        let first = listing_event_with_title_at(&seller, "Pasture Eggs", 200);
        let stale = listing_event_with_title_at(&seller, "Older Eggs", 199);
        pull_with_fetcher(&config, fake_fetcher(vec![first])).expect("initial sync pull");
        let view = pull_with_fetcher(&config, fake_fetcher(vec![stale])).expect("stale sync pull");

        assert_eq!(view.state, "ready");
        assert_eq!(view.fetched_count, Some(1));
        assert_eq!(view.ingested_count, Some(0));
        assert_eq!(view.skipped_count, Some(1));
        assert_eq!(view.reason_code.as_deref(), Some("sync_no_overwrite"));
        assert!(
            view.reason
                .as_deref()
                .expect("skip reason")
                .contains("current or newer state")
        );
        let run = view.freshness.run.as_ref().expect("run freshness");
        assert_eq!(run.last_state, "success");
        assert_eq!(run.skipped_count, Some(1));

        let search = crate::runtime::find::search(
            &config,
            &FindQueryArgs {
                query: vec!["eggs".to_owned()],
            },
        )
        .expect("market search");
        assert_eq!(search.results[0].title, "Pasture Eggs");
    }

    #[test]
    fn sync_pull_freshness_reports_relay_set_changed() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay-a.example.com".to_owned()]);
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(11);
        pull_with_fetcher(&config, fake_fetcher(vec![listing_event(&seller)])).expect("sync pull");
        let changed = sample_config(dir.path(), vec!["wss://relay-b.example.com".to_owned()]);

        let freshness =
            freshness_for_scope(&changed, RelayIngestScope::SyncPull).expect("sync freshness");

        assert_eq!(freshness.state, "relay_set_changed");
        let run = freshness.run.as_ref().expect("run freshness");
        assert_eq!(run.scope, "sync_pull");
        assert_eq!(run.relay_set_current, false);
    }

    #[test]
    fn relay_ingest_splits_unsupported_and_failed_events() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::store::init(&config).expect("store init");
        let seller = identity(9);
        let events = vec![
            signed_event(
                &seller,
                WireEventParts {
                    kind: KIND_POST,
                    content: "hello".to_owned(),
                    tags: Vec::new(),
                },
            ),
            signed_event(
                &seller,
                WireEventParts {
                    kind: KIND_LISTING,
                    content: "not a listing".to_owned(),
                    tags: Vec::new(),
                },
            ),
        ];

        let view = pull_with_fetcher(&config, fake_fetcher(events)).expect("sync pull ingest");

        assert_eq!(view.state, "ready");
        assert_eq!(view.fetched_count, Some(2));
        assert_eq!(view.ingested_count, Some(0));
        assert_eq!(view.unsupported_count, Some(1));
        assert_eq!(view.failed_count, Some(1));
        assert!(
            view.reason
                .as_deref()
                .expect("failure reason")
                .contains("failed ingest")
        );
    }

    fn fake_fetcher(
        events: Vec<RadrootsNostrEvent>,
    ) -> impl FnOnce(
        &[String],
        RadrootsNostrFilter,
    ) -> Result<RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError> {
        move |relays, _| Ok(relay_fetch_receipt(relays, events))
    }

    fn relay_fetch_receipt(
        relays: &[String],
        events: Vec<RadrootsNostrEvent>,
    ) -> RadrootsRelayFetchedEventsReceipt {
        relay_fetch_receipt_with_failed(relays, events, Vec::new())
    }

    fn relay_fetch_receipt_with_failed(
        relays: &[String],
        events: Vec<RadrootsNostrEvent>,
        failed_relays: Vec<RadrootsRelayFetchFailure>,
    ) -> RadrootsRelayFetchedEventsReceipt {
        let connected_relays = relays
            .iter()
            .filter(|relay| {
                failed_relays
                    .iter()
                    .all(|failure| failure.relay_url.as_str() != relay.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        let closed_count = failed_relays.len();
        RadrootsRelayFetchedEventsReceipt {
            target_relays: relays.to_vec(),
            connected_relays: connected_relays.clone(),
            failed_relays,
            events: events
                .into_iter()
                .map(|event| RadrootsRelayFetchedEvent {
                    relay_url: connected_relays.first().cloned().unwrap_or_default(),
                    event,
                    raw_json: String::new(),
                    observed_at_ms: 0,
                })
                .collect(),
            event_receipts: Vec::new(),
            malformed_count: 0,
            out_of_filter_count: 0,
            skipped_over_limit_count: 0,
            eose_count: connected_relays.len(),
            closed_count,
            notice_count: 0,
            relay_outcomes: Vec::new(),
        }
    }

    fn profile_event(identity: &RadrootsIdentity) -> RadrootsNostrEvent {
        let profile = RadrootsProfile {
            name: "seller".to_owned(),
            display_name: Some("Seller".to_owned()),
            nip05: None,
            about: Some("market seller".to_owned()),
            website: Some("https://seller.example.com".to_owned()),
            picture: None,
            banner: None,
            lud06: None,
            lud16: None,
            bot: None,
        };
        signed_event(
            identity,
            profile_encode::to_wire_parts_with_profile_type(
                &profile,
                Some(RadrootsProfileType::Farm),
            )
            .expect("profile parts"),
        )
    }

    fn farm_event(identity: &RadrootsIdentity) -> RadrootsNostrEvent {
        let farm = RadrootsFarm {
            d_tag: FARM_D_TAG.to_owned(),
            name: "Relay Farm".to_owned(),
            about: Some("relay farm".to_owned()),
            website: Some("https://farm.example.com".to_owned()),
            picture: None,
            banner: None,
            location: None,
            tags: None,
        };
        signed_event(
            identity,
            farm_encode::to_wire_parts(&farm).expect("farm parts"),
        )
    }

    fn plot_event(identity: &RadrootsIdentity) -> RadrootsNostrEvent {
        let plot = RadrootsPlot {
            d_tag: PLOT_D_TAG.to_owned(),
            farm: RadrootsFarmRef {
                pubkey: identity.public_key_hex(),
                d_tag: FARM_D_TAG.to_owned(),
            },
            name: "Relay Plot".to_owned(),
            about: Some("relay plot".to_owned()),
            location: None,
            tags: None,
        };
        signed_event(
            identity,
            plot_encode::to_wire_parts(&plot).expect("plot parts"),
        )
    }

    fn list_set_event(identity: &RadrootsIdentity) -> RadrootsNostrEvent {
        let list_set = RadrootsListSet {
            d_tag: "member_of.farms".to_owned(),
            content: String::new(),
            entries: vec![RadrootsListEntry {
                tag: "p".to_owned(),
                values: vec![identity.public_key_hex()],
            }],
            title: None,
            description: None,
            image: None,
        };
        signed_event(
            identity,
            list_set_encode::to_wire_parts_with_kind(&list_set, KIND_LIST_SET_GENERIC)
                .expect("list set parts"),
        )
    }

    fn listing_event(identity: &RadrootsIdentity) -> RadrootsNostrEvent {
        listing_event_with_title_at(identity, "Pasture Eggs", 0)
    }

    fn listing_event_with_title_at(
        identity: &RadrootsIdentity,
        title: &str,
        created_at: u64,
    ) -> RadrootsNostrEvent {
        let mut builder = radroots_nostr_build_event(
            KIND_LISTING,
            "# Pasture Eggs",
            vec![
                vec!["d".to_owned(), LISTING_D_TAG.to_owned()],
                vec![
                    "a".to_owned(),
                    format!("{}:{}:{}", KIND_FARM, identity.public_key_hex(), FARM_D_TAG),
                ],
                vec!["p".to_owned(), identity.public_key_hex()],
                vec!["key".to_owned(), "pasture-eggs".to_owned()],
                vec!["title".to_owned(), title.to_owned()],
                vec!["category".to_owned(), "eggs".to_owned()],
                vec!["summary".to_owned(), "Pasture-raised eggs".to_owned()],
                vec!["process".to_owned(), "washed".to_owned()],
                vec!["lot".to_owned(), "lot-a".to_owned()],
                vec!["profile".to_owned(), "dozen".to_owned()],
                vec!["year".to_owned(), "2026".to_owned()],
                vec!["radroots:primary_bin".to_owned(), "bin-a".to_owned()],
                vec![
                    "radroots:bin".to_owned(),
                    "bin-a".to_owned(),
                    "12".to_owned(),
                    "each".to_owned(),
                    "12".to_owned(),
                    "each".to_owned(),
                    "dozen".to_owned(),
                ],
                vec![
                    "radroots:price".to_owned(),
                    "bin-a".to_owned(),
                    "6".to_owned(),
                    "USD".to_owned(),
                    "1".to_owned(),
                    "each".to_owned(),
                    "6".to_owned(),
                    "each".to_owned(),
                ],
                vec!["inventory".to_owned(), "5".to_owned()],
                vec!["status".to_owned(), "active".to_owned()],
            ],
        )
        .expect("listing parts");
        if created_at > 0 {
            builder = builder.custom_created_at(RadrootsNostrTimestamp::from(created_at));
        }
        builder
            .sign_with_keys(identity.keys())
            .expect("signed event")
    }

    fn signed_event(identity: &RadrootsIdentity, parts: WireEventParts) -> RadrootsNostrEvent {
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("event builder")
            .sign_with_keys(identity.keys())
            .expect("signed event")
    }

    fn identity(seed: u8) -> RadrootsIdentity {
        RadrootsIdentity::from_secret_key_bytes(&[seed; 32]).expect("identity")
    }

    fn sample_config(root: &Path, relays: Vec<String>) -> RuntimeConfig {
        let data = root.join("data");
        let cache = root.join("cache");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Terminal,
                verbosity: Verbosity::Normal,
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
                shared_cache_root: cache.clone(),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
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
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
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
            transport: crate::runtime::config::TransportConfig::from_nostr_relay_urls(
                relays.clone(),
            ),
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
            mesh: crate::runtime::config::MeshConfig::disabled(),
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
            },
            rhi: crate::runtime::config::RhiConfig {
                trusted_worker_pubkeys: Vec::new(),
            },
            capability_bindings: Vec::new(),
        }
    }
}
