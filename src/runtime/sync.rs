use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radroots_events::kinds::{KIND_FARM, KIND_LISTING, KIND_PROFILE};
use radroots_nostr::prelude::{
    RadrootsNostrFilter, radroots_event_from_nostr, radroots_nostr_kind,
};
use radroots_replica_db::ReplicaSql;
use radroots_replica_sync::{
    RadrootsReplicaIngestOutcome, radroots_replica_ingest_event, radroots_replica_sync_status,
};
use radroots_sql_core::SqliteExecutor;

use crate::domain::runtime::{
    RelayFailureView, SyncActionView, SyncFreshnessView, SyncQueueView, SyncStatusView,
    SyncWatchFrameView, SyncWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayFetchReceipt, fetch_events_from_relays,
};
use crate::runtime_args::SyncWatchArgs;

const SYNC_SOURCE: &str = "local replica · local first";
const RELAY_SETUP_ACTION: &str = "radroots --relay wss://relay.example.com relay list";

#[derive(Debug, Clone)]
struct SyncSnapshot {
    state: String,
    source: String,
    local_root: String,
    replica_db: String,
    relay_count: usize,
    publish_policy: String,
    freshness: SyncFreshnessView,
    queue: SyncQueueView,
    reason: Option<String>,
    actions: Vec<String>,
}

pub fn status(config: &RuntimeConfig) -> Result<SyncStatusView, RuntimeError> {
    let snapshot = inspect_sync(config)?;
    Ok(SyncStatusView {
        state: snapshot.state,
        source: snapshot.source,
        local_root: snapshot.local_root,
        replica_db: snapshot.replica_db,
        relay_count: snapshot.relay_count,
        publish_policy: snapshot.publish_policy,
        freshness: snapshot.freshness,
        queue: snapshot.queue,
        reason: snapshot.reason,
        actions: snapshot.actions,
    })
}

pub fn pull(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    narrowed_action(
        config,
        "pull",
        "relay ingest is not wired into `radroots sync pull` yet",
    )
}

pub fn market_refresh(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    let snapshot = inspect_sync(config)?;
    if snapshot.state == "unconfigured" {
        return Ok(empty_action_from_snapshot(snapshot, "pull"));
    }

    if config.output.dry_run {
        let mut view = empty_action_from_snapshot(snapshot, "pull");
        view.state = "ready".to_owned();
        view.reason = Some("dry run requested; relay fetch skipped".to_owned());
        view.target_relays = config.relay.urls.clone();
        view.fetched_count = Some(0);
        view.ingested_count = Some(0);
        view.skipped_count = Some(0);
        view.unsupported_count = Some(0);
        return Ok(view);
    }

    let receipt = match fetch_events_from_relays(&config.relay.urls, market_refresh_filter()) {
        Ok(receipt) => receipt,
        Err(error) => {
            let mut view = empty_action_from_snapshot(snapshot, "pull");
            view.state = "unavailable".to_owned();
            view.reason = Some(error.to_string());
            view.target_relays = config.relay.urls.clone();
            return Ok(view);
        }
    };

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let ingest = ingest_market_events(&executor, &receipt)?;
    let freshness = freshness_from_executor(&executor)?;
    let queue = radroots_replica_sync_status(&executor)?;

    Ok(SyncActionView {
        direction: "pull".to_owned(),
        state: "ready".to_owned(),
        source: "direct Nostr relay fetch · local replica ingest".to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "ready".to_owned(),
        relay_count: config.relay.urls.len(),
        publish_policy: config.relay.publish_policy.as_str().to_owned(),
        freshness,
        queue: SyncQueueView {
            expected_count: queue.expected_count,
            pending_count: queue.pending_count,
        },
        target_relays: receipt.target_relays,
        connected_relays: receipt.connected_relays,
        failed_relays: relay_failures(receipt.failed_relays),
        fetched_count: Some(ingest.fetched_count),
        ingested_count: Some(ingest.ingested_count),
        skipped_count: Some(ingest.skipped_count),
        unsupported_count: Some(ingest.unsupported_count),
        reason: None,
        actions: vec!["radroots market product search eggs".to_owned()],
    })
}

pub fn push(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    narrowed_action(
        config,
        "push",
        "relay publish is not wired into `radroots sync push` yet",
    )
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
            relay_count: snapshot.relay_count,
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

fn narrowed_action(
    config: &RuntimeConfig,
    direction: &str,
    unavailable_reason: &str,
) -> Result<SyncActionView, RuntimeError> {
    let snapshot = inspect_sync(config)?;
    if snapshot.state == "unconfigured" {
        return Ok(empty_action_from_snapshot(snapshot, direction));
    }

    let mut actions = vec!["radroots sync status get".to_owned()];
    actions.extend(snapshot.actions);

    Ok(SyncActionView {
        direction: direction.to_owned(),
        state: "unavailable".to_owned(),
        source: snapshot.source,
        local_root: snapshot.local_root,
        replica_db: snapshot.replica_db,
        relay_count: snapshot.relay_count,
        publish_policy: snapshot.publish_policy,
        freshness: snapshot.freshness,
        queue: snapshot.queue,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: None,
        ingested_count: None,
        skipped_count: None,
        unsupported_count: None,
        reason: Some(unavailable_reason.to_owned()),
        actions,
    })
}

fn empty_action_from_snapshot(snapshot: SyncSnapshot, direction: &str) -> SyncActionView {
    SyncActionView {
        direction: direction.to_owned(),
        state: snapshot.state,
        source: snapshot.source,
        local_root: snapshot.local_root,
        replica_db: snapshot.replica_db,
        relay_count: snapshot.relay_count,
        publish_policy: snapshot.publish_policy,
        freshness: snapshot.freshness,
        queue: snapshot.queue,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: None,
        ingested_count: None,
        skipped_count: None,
        unsupported_count: None,
        reason: snapshot.reason,
        actions: snapshot.actions,
    }
}

fn inspect_sync(config: &RuntimeConfig) -> Result<SyncSnapshot, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(SyncSnapshot {
            state: "unconfigured".to_owned(),
            source: SYNC_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_db: "missing".to_owned(),
            relay_count: config.relay.urls.len(),
            publish_policy: config.relay.publish_policy.as_str().to_owned(),
            freshness: SyncFreshnessView {
                state: "never".to_owned(),
                display: "never synced".to_owned(),
                age_seconds: None,
                last_event_at: None,
            },
            queue: SyncQueueView {
                expected_count: 0,
                pending_count: 0,
            },
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        });
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let queue = radroots_replica_sync_status(&executor)?;
    let freshness = freshness_from_executor(&executor)?;
    let relay_count = config.relay.urls.len();
    let publish_policy = config.relay.publish_policy.as_str().to_owned();
    let mut actions = Vec::new();

    if relay_count == 0 {
        actions.push(RELAY_SETUP_ACTION.to_owned());
        return Ok(SyncSnapshot {
            state: "unconfigured".to_owned(),
            source: SYNC_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_db: "ready".to_owned(),
            relay_count,
            publish_policy,
            freshness,
            queue: SyncQueueView {
                expected_count: queue.expected_count,
                pending_count: queue.pending_count,
            },
            reason: Some("no relays are configured for this operator session".to_owned()),
            actions,
        });
    }

    actions.push("radroots sync pull".to_owned());
    if queue.pending_count > 0 {
        actions.push("radroots sync push".to_owned());
    }

    Ok(SyncSnapshot {
        state: "ready".to_owned(),
        source: SYNC_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "ready".to_owned(),
        relay_count,
        publish_policy,
        freshness,
        queue: SyncQueueView {
            expected_count: queue.expected_count,
            pending_count: queue.pending_count,
        },
        reason: None,
        actions,
    })
}

pub(crate) fn freshness_from_executor(
    executor: &SqliteExecutor,
) -> Result<SyncFreshnessView, RuntimeError> {
    let db = ReplicaSql::new(executor);
    let last_event_at = db.nostr_event_last_created_at()?;

    Ok(match last_event_at {
        Some(last_event_at) => {
            let age_seconds = unix_now().saturating_sub(last_event_at);
            SyncFreshnessView {
                state: "synced".to_owned(),
                display: format!("synced {}", relative_age(age_seconds)),
                age_seconds: Some(age_seconds),
                last_event_at: Some(last_event_at),
            }
        }
        None => SyncFreshnessView {
            state: "never".to_owned(),
            display: "never synced".to_owned(),
            age_seconds: None,
            last_event_at: None,
        },
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct MarketIngestCounts {
    fetched_count: usize,
    ingested_count: usize,
    skipped_count: usize,
    unsupported_count: usize,
}

fn market_refresh_filter() -> RadrootsNostrFilter {
    RadrootsNostrFilter::new()
        .kinds([
            radroots_nostr_kind(KIND_PROFILE as u16),
            radroots_nostr_kind(KIND_FARM as u16),
            radroots_nostr_kind(KIND_LISTING as u16),
        ])
        .limit(1_000)
}

fn ingest_market_events(
    executor: &SqliteExecutor,
    receipt: &DirectRelayFetchReceipt,
) -> Result<MarketIngestCounts, RuntimeError> {
    let mut counts = MarketIngestCounts {
        fetched_count: receipt.events.len(),
        ..MarketIngestCounts::default()
    };

    for event in &receipt.events {
        let event = radroots_event_from_nostr(event);
        match radroots_replica_ingest_event(executor, &event) {
            Ok(RadrootsReplicaIngestOutcome::Applied) => counts.ingested_count += 1,
            Ok(RadrootsReplicaIngestOutcome::Skipped) => counts.skipped_count += 1,
            Err(_) => counts.unsupported_count += 1,
        }
    }

    Ok(counts)
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
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
