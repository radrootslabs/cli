use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radroots_replica_sync::radroots_replica_sync_status;
use radroots_sql_core::{SqlExecutor, SqliteExecutor};
use serde_json::Value;

use crate::cli::SyncWatchArgs;
use crate::domain::runtime::{
    SyncActionView, SyncFreshnessView, SyncQueueView, SyncStatusView, SyncWatchFrameView,
    SyncWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

const SYNC_SOURCE: &str = "local replica · local first";
const RELAY_SETUP_ACTION: &str = "radroots relay ls --relay wss://relay.example.com";

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
        return Ok(SyncActionView {
            direction: direction.to_owned(),
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
        });
    }

    let mut actions = vec!["radroots sync status".to_owned()];
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
        reason: Some(unavailable_reason.to_owned()),
        actions,
    })
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
            actions: vec!["radroots local init".to_owned()],
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
    let raw = executor.query_raw(
        "SELECT MAX(last_created_at) AS last_created_at FROM nostr_event_state WHERE last_created_at IS NOT NULL",
        "[]",
    )?;
    let json: Value = serde_json::from_str(&raw)?;
    let last_event_at = json
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("last_created_at"))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|signed| u64::try_from(signed).ok()))
        });

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
