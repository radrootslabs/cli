use std::collections::BTreeMap;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use radroots_events::kinds::{
    KIND_FARM, KIND_LIST_SET_APP_CURATION, KIND_LIST_SET_BOOKMARK, KIND_LIST_SET_CALENDAR,
    KIND_LIST_SET_CURATION, KIND_LIST_SET_EMOJI, KIND_LIST_SET_FOLLOW, KIND_LIST_SET_GENERIC,
    KIND_LIST_SET_INTEREST, KIND_LIST_SET_KIND_MUTE, KIND_LIST_SET_MEDIA_STARTER_PACK,
    KIND_LIST_SET_PICTURE, KIND_LIST_SET_RELAY, KIND_LIST_SET_RELEASE_ARTIFACT,
    KIND_LIST_SET_STARTER_PACK, KIND_LIST_SET_VIDEO, KIND_LISTING, KIND_PLOT, KIND_PROFILE,
};
use radroots_events_codec::wire::WireEventParts;
use radroots_identity::RadrootsIdentity;
use radroots_nostr::prelude::{
    RadrootsNostrFilter, radroots_event_from_nostr, radroots_nostr_kind,
};
use radroots_replica_db::{ReplicaSql, migrations};
use radroots_replica_sync::{
    RadrootsReplicaEventsError, RadrootsReplicaIngestOutcome, RadrootsReplicaPendingPublishEvent,
    radroots_replica_ingest_event, radroots_replica_ingest_event_state,
    radroots_replica_pending_publish_batch, radroots_replica_sync_status,
};
use radroots_sql_core::SqliteExecutor;

use crate::domain::runtime::{
    RelayFailureView, SyncActionView, SyncFreshnessView, SyncPublishPlanAuthorView,
    SyncPublishPlanKindView, SyncPublishPlanView, SyncQueueView, SyncStatusView,
    SyncWatchFrameView, SyncWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::{PublishMode, RuntimeConfig};
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayFetchError, DirectRelayFetchReceipt, DirectRelayPublishError,
    DirectRelayPublishReceipt, fetch_events_from_relays, publish_parts_with_identity,
};
use crate::runtime_args::SyncWatchArgs;

const SYNC_SOURCE: &str = "local replica · local first";
const RELAY_PULL_SETUP_ACTION: &str = "radroots --relay wss://relay.example.com sync pull";
const RELAY_PUSH_SETUP_ACTION: &str = "radroots --relay wss://relay.example.com sync push";
const SYNC_PULL_ACTION: &str = "radroots sync pull";
const SYNC_PUSH_ACTION: &str = "radroots sync push";
const SYNC_READY_ACTION: &str = "radroots market product search eggs";
const MARKET_READY_ACTION: &str = "radroots market product search eggs";
const INGEST_SOURCE: &str = "direct Nostr relay fetch · local replica ingest";
const PUBLISH_SOURCE: &str = "direct Nostr relay publish · local replica sync";
pub(crate) const RADROOTSD_SYNC_PUSH_UNAVAILABLE_REASON: &str = "sync push is only available in publish mode `nostr_relay`; radrootsd sync push is not implemented";
const RELAY_FETCH_LIMIT: usize = 1_000;
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
    pull_with_fetcher(config, fetch_events_from_relays)
}

pub fn market_refresh(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    market_refresh_with_fetcher(config, fetch_events_from_relays)
}

fn pull_with_fetcher<F>(config: &RuntimeConfig, fetcher: F) -> Result<SyncActionView, RuntimeError>
where
    F: FnOnce(
        &[String],
        RadrootsNostrFilter,
    ) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError>,
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
    ) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError>,
{
    relay_ingest(config, RelayIngestScope::MarketRefresh, fetcher)
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
    ) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError>,
{
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
        view.publishable_count = None;
        view.published_count = None;
        view.skipped_count = Some(0);
        view.unsupported_count = Some(0);
        view.failed_count = Some(0);
        view.actions = vec![scope.ready_action().to_owned()];
        return Ok(view);
    }

    let receipt = match fetcher(&config.relay.urls, scope.filter()) {
        Ok(receipt) => receipt,
        Err(DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        }) => {
            let mut view = empty_action_from_snapshot(snapshot, "pull");
            view.state = "unavailable".to_owned();
            view.reason = Some(format!("direct relay connection failed: {reason}"));
            view.target_relays = target_relays;
            view.failed_relays = relay_failures(failed_relays);
            return Ok(view);
        }
        Err(error) => {
            let mut view = empty_action_from_snapshot(snapshot, "pull");
            view.state = "unavailable".to_owned();
            view.reason = Some(error.to_string());
            view.target_relays = config.relay.urls.clone();
            return Ok(view);
        }
    };

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    let ingest = ingest_events(&executor, &receipt, scope)?;
    let freshness = freshness_from_executor(&executor)?;
    let queue = radroots_replica_sync_status(&executor)?;

    Ok(SyncActionView {
        direction: "pull".to_owned(),
        state: "ready".to_owned(),
        source: INGEST_SOURCE.to_owned(),
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
        acknowledged_relays: Vec::new(),
        failed_relays: relay_failures(receipt.failed_relays),
        fetched_count: Some(ingest.fetched_count),
        ingested_count: Some(ingest.ingested_count),
        publishable_count: None,
        published_count: None,
        skipped_count: Some(ingest.skipped_count),
        unsupported_count: Some(ingest.unsupported_count),
        failed_count: Some(ingest.failed_count),
        publish_plan: None,
        reason: ingest.reason(),
        actions: vec![scope.ready_action().to_owned()],
    })
}

pub fn push(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    push_with_publisher(config, |identity, relay_urls, event| {
        publish_parts_with_identity(
            identity,
            relay_urls,
            WireEventParts {
                kind: event.draft.kind,
                content: event.draft.content.clone(),
                tags: event.draft.tags.clone(),
            },
        )
    })
}

fn push_with_publisher<F>(
    config: &RuntimeConfig,
    mut publisher: F,
) -> Result<SyncActionView, RuntimeError>
where
    F: FnMut(
        &RadrootsIdentity,
        &[String],
        &RadrootsReplicaPendingPublishEvent,
    ) -> Result<DirectRelayPublishReceipt, DirectRelayPublishError>,
{
    if matches!(config.publish.mode, PublishMode::Radrootsd) {
        return Ok(push_radrootsd_unavailable_view(config));
    }

    let snapshot = inspect_sync(config)?;
    if snapshot.state == "unconfigured" {
        return Ok(push_unconfigured_view(snapshot));
    }

    let signing = match accounts::resolve_local_signing_identity(config) {
        Ok(signing) => signing,
        Err(RuntimeError::Account(failure)) => {
            let mut view = empty_action_from_snapshot(snapshot, "push");
            view.state = "unconfigured".to_owned();
            view.reason = Some(failure.to_string());
            view.actions = vec![
                "radroots account create".to_owned(),
                "radroots account attach-secret".to_owned(),
            ];
            return Ok(view);
        }
        Err(error) => return Err(error),
    };

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    let batch = radroots_replica_pending_publish_batch(&executor)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    let (mut counts, publishable_events, publish_plan) = sync_push_plan(&batch, selected_pubkey);

    if config.output.dry_run {
        let state = if counts.pending_count > 0 {
            "dry_run"
        } else {
            "ready"
        };
        let reason = sync_push_dry_run_reason(&counts);
        let actions = sync_push_actions(state, &counts);
        return Ok(push_view(
            config,
            state,
            SyncQueueView {
                expected_count: batch.expected_count,
                pending_count: batch.pending_count,
            },
            snapshot.freshness,
            counts,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(publish_plan),
            reason,
            actions,
        ));
    }

    let mut connected_relays = Vec::new();
    let mut acknowledged_relays = Vec::new();
    let mut failed_relays = Vec::new();

    for event in publishable_events {
        match publisher(&signing.identity, &config.relay.urls, event) {
            Ok(receipt) => {
                push_unique_many(&mut connected_relays, receipt.connected_relays.iter());
                push_unique_many(&mut acknowledged_relays, receipt.acknowledged_relays.iter());
                failed_relays.extend(relay_failures(receipt.failed_relays));
                let signed_event = radroots_event_from_nostr(&receipt.event);
                radroots_replica_ingest_event_state(
                    &executor,
                    &signed_event,
                    event.d_tag.as_str(),
                    event.content_hash.as_str(),
                )?;
                counts.published_count += 1;
            }
            Err(error) => {
                counts.failed_count += 1;
                let failure = sync_push_publish_failure(error);
                push_unique_many(&mut connected_relays, failure.connected_relays.iter());
                failed_relays.extend(failure.failed_relays);
                if counts.first_failure_reason.is_none() {
                    counts.first_failure_reason = Some(failure.reason);
                }
                break;
            }
        }
    }

    let queue = radroots_replica_sync_status(&executor)?;
    let freshness = freshness_from_executor(&executor)?;
    let state = if counts.failed_count > 0 && counts.published_count > 0 {
        "partial"
    } else if counts.failed_count > 0 {
        "unavailable"
    } else if counts.published_count > 0 && counts.skipped_count > 0 && queue.pending_count > 0 {
        "partial"
    } else if counts.published_count == 0 && counts.skipped_count > 0 && queue.pending_count > 0 {
        "unconfigured"
    } else if counts.published_count > 0 {
        "published"
    } else {
        "ready"
    };
    let reason = counts.reason();
    let actions = sync_push_actions(state, &counts);

    Ok(push_view(
        config,
        state,
        SyncQueueView {
            expected_count: queue.expected_count,
            pending_count: queue.pending_count,
        },
        freshness,
        counts,
        connected_relays,
        acknowledged_relays,
        failed_relays,
        None,
        reason,
        actions,
    ))
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
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: None,
        ingested_count: None,
        publishable_count: None,
        published_count: None,
        skipped_count: None,
        unsupported_count: None,
        failed_count: None,
        publish_plan: None,
        reason: snapshot.reason,
        actions: snapshot.actions,
    }
}

fn push_radrootsd_unavailable_view(config: &RuntimeConfig) -> SyncActionView {
    SyncActionView {
        direction: "push".to_owned(),
        state: "unavailable".to_owned(),
        source: PUBLISH_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "not_checked".to_owned(),
        relay_count: config.relay.urls.len(),
        publish_policy: config.relay.publish_policy.as_str().to_owned(),
        freshness: SyncFreshnessView {
            state: "not_checked".to_owned(),
            display: "not checked".to_owned(),
            age_seconds: None,
            last_event_at: None,
        },
        queue: SyncQueueView {
            expected_count: 0,
            pending_count: 0,
        },
        target_relays: config.relay.urls.clone(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: None,
        ingested_count: None,
        publishable_count: None,
        published_count: None,
        skipped_count: None,
        unsupported_count: None,
        failed_count: None,
        publish_plan: None,
        reason: Some(RADROOTSD_SYNC_PUSH_UNAVAILABLE_REASON.to_owned()),
        actions: vec!["radroots --publish-mode nostr_relay sync push".to_owned()],
    }
}

fn push_unconfigured_view(snapshot: SyncSnapshot) -> SyncActionView {
    let mut view = empty_action_from_snapshot(snapshot, "push");
    if view.replica_db == "ready" && view.relay_count == 0 {
        view.actions = vec![RELAY_PUSH_SETUP_ACTION.to_owned()];
    }
    view
}

fn push_view(
    config: &RuntimeConfig,
    state: &str,
    queue: SyncQueueView,
    freshness: SyncFreshnessView,
    counts: SyncPushCounts,
    connected_relays: Vec<String>,
    acknowledged_relays: Vec<String>,
    failed_relays: Vec<RelayFailureView>,
    publish_plan: Option<SyncPublishPlanView>,
    reason: Option<String>,
    actions: Vec<String>,
) -> SyncActionView {
    SyncActionView {
        direction: "push".to_owned(),
        state: state.to_owned(),
        source: PUBLISH_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "ready".to_owned(),
        relay_count: config.relay.urls.len(),
        publish_policy: config.relay.publish_policy.as_str().to_owned(),
        freshness,
        queue,
        target_relays: config.relay.urls.clone(),
        connected_relays,
        acknowledged_relays,
        failed_relays,
        fetched_count: None,
        ingested_count: None,
        publishable_count: Some(counts.publishable_count),
        published_count: Some(counts.published_count),
        skipped_count: Some(counts.skipped_count),
        unsupported_count: Some(counts.unsupported_count),
        failed_count: Some(counts.failed_count),
        publish_plan,
        reason,
        actions,
    }
}

fn sync_push_plan<'a>(
    batch: &'a radroots_replica_sync::RadrootsReplicaPendingPublishBatch,
    selected_pubkey: &str,
) -> (
    SyncPushCounts,
    Vec<&'a RadrootsReplicaPendingPublishEvent>,
    SyncPublishPlanView,
) {
    let mut counts = SyncPushCounts::from_batch(batch);
    let mut publishable_events = Vec::new();
    let mut event_kinds = BTreeMap::<u32, SyncPublishPlanKindView>::new();
    let mut authors = BTreeMap::<String, SyncPublishPlanAuthorView>::new();
    let selected_author = canonical_pubkey_hex(selected_pubkey);

    for event in &batch.pending_events {
        let event_author = canonical_pubkey_hex(event.author.as_str());
        let is_publishable = event_author == selected_author;
        let kind = event_kinds
            .entry(event.kind)
            .or_insert_with(|| SyncPublishPlanKindView {
                kind: event.kind,
                pending_count: 0,
                publishable_count: 0,
                skipped_count: 0,
                unsupported_count: 0,
                failed_count: 0,
            });
        kind.pending_count += 1;

        let author =
            authors
                .entry(event_author.clone())
                .or_insert_with(|| SyncPublishPlanAuthorView {
                    author: event_author.clone(),
                    eligibility: if is_publishable {
                        "selected".to_owned()
                    } else {
                        "other_author".to_owned()
                    },
                    pending_count: 0,
                    publishable_count: 0,
                    skipped_count: 0,
                });
        author.pending_count += 1;

        if is_publishable {
            kind.publishable_count += 1;
            author.publishable_count += 1;
            publishable_events.push(event);
        } else {
            kind.skipped_count += 1;
            author.skipped_count += 1;
            counts.skipped_count += 1;
            if counts.first_skipped_author.is_none() {
                counts.first_skipped_author = Some(event_author);
            }
        }
    }

    counts.publishable_count = publishable_events.len();

    (
        counts,
        publishable_events,
        SyncPublishPlanView {
            selected_author,
            event_kinds: event_kinds.into_values().collect(),
            authors: authors.into_values().collect(),
        },
    )
}

fn canonical_pubkey_hex(pubkey: &str) -> String {
    pubkey.to_ascii_lowercase()
}

fn sync_push_dry_run_reason(counts: &SyncPushCounts) -> Option<String> {
    match counts.skipped_count {
        0 => Some("dry run requested; relay publish skipped".to_owned()),
        skipped => Some(format!(
            "dry run requested; relay publish skipped; {skipped} pending event(s) belong to another author and would not be signed"
        )),
    }
}

fn sync_push_actions(state: &str, counts: &SyncPushCounts) -> Vec<String> {
    let retry_selected_account =
        counts.failed_count > 0 || counts.publishable_count > counts.published_count;
    let selected_account_actionable =
        retry_selected_account || (state == "dry_run" && counts.publishable_count > 0);
    let mut actions = match state {
        "published" | "ready" => vec!["radroots sync status get".to_owned()],
        "dry_run" if selected_account_actionable => {
            vec![
                SYNC_PUSH_ACTION.to_owned(),
                "radroots sync status get".to_owned(),
            ]
        }
        "dry_run" => vec!["radroots sync status get".to_owned()],
        _ if selected_account_actionable => {
            vec![
                SYNC_PUSH_ACTION.to_owned(),
                "radroots sync status get".to_owned(),
            ]
        }
        _ => vec!["radroots sync status get".to_owned()],
    };

    if counts.skipped_count > 0 {
        actions.push("radroots account list".to_owned());
        if let Some(author) = counts.first_skipped_author.as_deref() {
            actions.push(format!("radroots --account-id {author} sync push"));
        }
    }

    actions.into_iter().fold(Vec::new(), |mut unique, action| {
        if !unique.contains(&action) {
            unique.push(action);
        }
        unique
    })
}

#[derive(Debug, Clone, Default)]
struct SyncPushCounts {
    pending_count: usize,
    publishable_count: usize,
    published_count: usize,
    skipped_count: usize,
    unsupported_count: usize,
    failed_count: usize,
    first_failure_reason: Option<String>,
    first_skipped_author: Option<String>,
}

impl SyncPushCounts {
    fn from_batch(batch: &radroots_replica_sync::RadrootsReplicaPendingPublishBatch) -> Self {
        Self {
            pending_count: batch.pending_count,
            ..Self::default()
        }
    }

    fn reason(&self) -> Option<String> {
        if self.failed_count > 0 {
            let failure_reason = match &self.first_failure_reason {
                Some(reason) => format!(
                    "{} pending event(s) failed publish: {reason}",
                    self.failed_count
                ),
                None => format!("{} pending event(s) failed publish", self.failed_count),
            };
            if self.skipped_count > 0 {
                return Some(format!(
                    "{failure_reason}; {} pending event(s) belong to another author and were not signed",
                    self.skipped_count
                ));
            }
            return Some(failure_reason);
        }
        if self.pending_count > 0 && self.skipped_count > 0 {
            return Some(
                "pending local replica events belong to another author and were not signed"
                    .to_owned(),
            );
        }
        None
    }
}

#[derive(Debug, Clone)]
struct SyncPushPublishFailure {
    reason: String,
    connected_relays: Vec<String>,
    failed_relays: Vec<RelayFailureView>,
}

fn sync_push_publish_failure(error: DirectRelayPublishError) -> SyncPushPublishFailure {
    match error {
        DirectRelayPublishError::Connect {
            reason,
            connected_relays,
            failed_relays,
            ..
        } => SyncPushPublishFailure {
            reason: format!("direct relay connection failed: {reason}"),
            connected_relays,
            failed_relays: relay_failures(failed_relays),
        },
        DirectRelayPublishError::Publish {
            reason,
            connected_relays,
            failed_relays,
            ..
        } => SyncPushPublishFailure {
            reason: format!("direct relay publish failed: {reason}"),
            connected_relays,
            failed_relays: relay_failures(failed_relays),
        },
        other => SyncPushPublishFailure {
            reason: other.to_string(),
            connected_relays: Vec::new(),
            failed_relays: Vec::new(),
        },
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
        actions.push(RELAY_PULL_SETUP_ACTION.to_owned());
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

    actions.push(SYNC_PULL_ACTION.to_owned());
    if queue.pending_count > 0 {
        actions.push(SYNC_PUSH_ACTION.to_owned());
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
    fn reason(&self) -> Option<String> {
        (self.failed_count > 0).then(|| match &self.first_failure_reason {
            Some(reason) => format!(
                "{} fetched event(s) failed ingest: {}",
                self.failed_count, reason
            ),
            None => format!("{} fetched event(s) failed ingest", self.failed_count),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum RelayIngestScope {
    SyncPull,
    MarketRefresh,
}

impl RelayIngestScope {
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
    receipt: &DirectRelayFetchReceipt,
    scope: RelayIngestScope,
) -> Result<RelayIngestCounts, RuntimeError> {
    let mut counts = RelayIngestCounts {
        fetched_count: receipt.events.len(),
        ..RelayIngestCounts::default()
    };

    for event in &receipt.events {
        if !scope.supports_kind(event_kind(event)) {
            counts.unsupported_count += 1;
            continue;
        }
        let event = radroots_event_from_nostr(event);
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

fn relay_failures(failures: Vec<DirectRelayFailure>) -> Vec<RelayFailureView> {
    failures
        .into_iter()
        .map(|failure| RelayFailureView {
            relay: failure.relay,
            reason: failure.reason,
        })
        .collect()
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
        RadrootsNostrEvent, RadrootsNostrFilter, radroots_nostr_build_event,
    };
    use radroots_replica_db::{farm, farm_member_claim, migrations};
    use radroots_replica_db_schema::farm::IFarmFields;
    use radroots_replica_db_schema::farm_member_claim::IFarmMemberClaimFields;
    use radroots_replica_sync::radroots_replica_sync_status;
    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use radroots_sql_core::SqliteExecutor;
    use tempfile::tempdir;

    use super::{
        DirectRelayFailure, DirectRelayFetchError, DirectRelayFetchReceipt,
        DirectRelayPublishReceipt, market_refresh_with_fetcher, pull_with_fetcher,
        push_with_publisher,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, PublishConfig, PublishMode, PublishModeSource, RelayConfig, RelayConfigSource,
        RelayPublishPolicy, RpcConfig, RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime_args::{FindQueryArgs, RecordLookupArgs};

    const FARM_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAA";
    const PLOT_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAQ";
    const LISTING_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAg";

    #[test]
    fn sync_pull_dry_run_skips_relay_fetch() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        config.output.dry_run = true;
        crate::runtime::local::init(&config).expect("store init");

        let view = pull_with_fetcher(&config, |_, _| panic!("dry run must not fetch"))
            .expect("sync pull dry run");

        assert_eq!(view.state, "ready");
        assert_eq!(view.target_relays, vec!["wss://relay.example.com"]);
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
        crate::runtime::local::init(&config).expect("store init");

        let view = pull_with_fetcher(&config, |_, _| {
            panic!("unconfigured sync pull must not fetch")
        })
        .expect("sync pull unconfigured");

        assert_eq!(view.state, "unconfigured");
        assert_eq!(
            view.actions,
            vec!["radroots --relay wss://relay.example.com sync pull"]
        );
    }

    #[test]
    fn sync_push_dry_run_reports_pending_without_publish_or_state_update() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        config.output.dry_run = true;
        crate::runtime::local::init(&config).expect("store init");
        let signing =
            crate::runtime::accounts::create_or_migrate_default_account(&config).expect("account");
        seed_replica_farm(
            &config,
            signing
                .account
                .record
                .public_identity
                .public_key_hex
                .as_str(),
        );

        let view = push_with_publisher(&config, |_, _, _| panic!("dry run must not publish"))
            .expect("sync push dry run");
        let status = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status");

        assert_eq!(view.state, "dry_run");
        assert_eq!(view.target_relays, vec!["wss://relay.example.com"]);
        assert_eq!(view.publishable_count, Some(status.pending_count));
        assert_eq!(view.published_count, Some(0));
        assert_eq!(view.failed_count, Some(0));
        let plan = view.publish_plan.as_ref().expect("publish plan");
        assert_eq!(
            plan.selected_author,
            signing.account.record.public_identity.public_key_hex
        );
        assert!(plan.event_kinds.iter().any(|kind| {
            kind.kind == KIND_FARM
                && kind.pending_count == 1
                && kind.publishable_count == 1
                && kind.skipped_count == 0
        }));
        assert!(plan.authors.iter().any(|author| {
            author.author == signing.account.record.public_identity.public_key_hex
                && author.eligibility == "selected"
                && author.publishable_count == status.pending_count
        }));
        assert!(status.pending_count > 0);
    }

    #[test]
    fn sync_push_dry_run_reports_other_author_publish_plan() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        config.output.dry_run = true;
        crate::runtime::local::init(&config).expect("store init");
        let signing =
            crate::runtime::accounts::create_or_migrate_default_account(&config).expect("account");
        let selected_pubkey = signing
            .account
            .record
            .public_identity
            .public_key_hex
            .clone();
        let other_pubkey = identity(42).public_key_hex();
        let other_pubkey_upper = other_pubkey.to_ascii_uppercase();
        seed_replica_farm(&config, selected_pubkey.as_str());
        seed_replica_farm(&config, other_pubkey_upper.as_str());

        let view = push_with_publisher(&config, |_, _, _| panic!("dry run must not publish"))
            .expect("sync push dry run");

        assert_eq!(view.state, "dry_run");
        let skipped_count = view.skipped_count.expect("skipped count");
        assert!(skipped_count > 0);
        assert!(
            view.reason
                .as_deref()
                .expect("dry-run reason")
                .contains("belong to another author")
        );
        let plan = view.publish_plan.as_ref().expect("publish plan");
        assert!(plan.event_kinds.iter().any(|kind| kind.skipped_count == 1));
        assert!(plan.authors.iter().any(|author| {
            author.author == other_pubkey
                && author.eligibility == "other_author"
                && author.pending_count == skipped_count
                && author.skipped_count == skipped_count
        }));
        assert!(view.actions.contains(&"radroots account list".to_owned()));
        assert!(
            view.actions
                .iter()
                .any(|action| action == &format!("radroots --account-id {other_pubkey} sync push"))
        );
    }

    #[test]
    fn sync_push_publishes_pending_local_author_events_and_updates_state() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::local::init(&config).expect("store init");
        let signing =
            crate::runtime::accounts::create_or_migrate_default_account(&config).expect("account");
        seed_replica_farm(
            &config,
            signing
                .account
                .record
                .public_identity
                .public_key_hex
                .as_str(),
        );
        let before = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status before");

        let view = push_with_publisher(&config, |identity, relays, event| {
            let signed = signed_event(
                identity,
                WireEventParts {
                    kind: event.draft.kind,
                    content: event.draft.content.clone(),
                    tags: event.draft.tags.clone(),
                },
            );
            Ok(DirectRelayPublishReceipt {
                event_id: signed.id.to_hex(),
                created_at: u32::try_from(signed.created_at.as_secs()).unwrap_or(u32::MAX),
                signature: signed.sig.to_string(),
                event: signed,
                target_relays: relays.to_vec(),
                connected_relays: relays.to_vec(),
                acknowledged_relays: relays.to_vec(),
                failed_relays: Vec::new(),
            })
        })
        .expect("sync push");
        let after = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status after");

        assert!(before.pending_count > 0);
        assert_eq!(view.state, "published");
        assert_eq!(view.published_count, Some(before.pending_count));
        assert_eq!(view.failed_count, Some(0));
        assert_eq!(view.connected_relays, vec!["wss://relay.example.com"]);
        assert_eq!(view.acknowledged_relays, vec!["wss://relay.example.com"]);
        assert_eq!(after.pending_count, 0);
    }

    #[test]
    fn sync_push_reports_partial_when_other_author_events_remain_pending() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::local::init(&config).expect("store init");
        let signing =
            crate::runtime::accounts::create_or_migrate_default_account(&config).expect("account");
        let selected_pubkey = signing
            .account
            .record
            .public_identity
            .public_key_hex
            .clone();
        let other_pubkey = identity(43).public_key_hex();
        seed_replica_farm(&config, selected_pubkey.as_str());
        seed_replica_member_claim(&config, other_pubkey.as_str(), selected_pubkey.as_str());

        let before = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status before");
        let view = push_with_publisher(&config, |identity, relays, event| {
            assert!(event.author.eq_ignore_ascii_case(selected_pubkey.as_str()));
            let signed = signed_event(
                identity,
                WireEventParts {
                    kind: event.draft.kind,
                    content: event.draft.content.clone(),
                    tags: event.draft.tags.clone(),
                },
            );
            Ok(DirectRelayPublishReceipt {
                event_id: signed.id.to_hex(),
                created_at: u32::try_from(signed.created_at.as_secs()).unwrap_or(u32::MAX),
                signature: signed.sig.to_string(),
                event: signed,
                target_relays: relays.to_vec(),
                connected_relays: relays.to_vec(),
                acknowledged_relays: relays.to_vec(),
                failed_relays: Vec::new(),
            })
        })
        .expect("sync push");
        let after = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status after");

        assert!(before.pending_count > after.pending_count);
        assert_eq!(view.state, "partial");
        assert_eq!(view.published_count, Some(before.pending_count - 1));
        assert_eq!(view.skipped_count, Some(1));
        assert_eq!(after.pending_count, 1);
        assert!(
            view.reason
                .as_deref()
                .expect("partial reason")
                .contains("belong to another author")
        );
        assert!(
            view.actions
                .contains(&"radroots sync status get".to_owned())
        );
        assert!(view.actions.contains(&"radroots account list".to_owned()));
        assert!(
            view.actions
                .iter()
                .any(|action| action == &format!("radroots --account-id {other_pubkey} sync push"))
        );
    }

    #[test]
    fn sync_push_reports_unconfigured_when_only_other_author_events_are_pending() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::local::init(&config).expect("store init");
        crate::runtime::accounts::create_or_migrate_default_account(&config).expect("account");
        let other_pubkey = identity(44).public_key_hex();
        seed_replica_farm(&config, other_pubkey.as_str());

        let view = push_with_publisher(&config, |_, _, _| {
            panic!("other-author-only queue must not publish")
        })
        .expect("sync push");
        let after = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status after");

        assert_eq!(view.state, "unconfigured");
        assert_eq!(view.publishable_count, Some(0));
        assert_eq!(view.published_count, Some(0));
        let skipped_count = view.skipped_count.expect("skipped count");
        assert!(skipped_count > 0);
        assert_eq!(after.pending_count, skipped_count);
        assert!(
            view.reason
                .as_deref()
                .expect("unconfigured reason")
                .contains("belong to another author")
        );
        assert!(
            view.actions
                .contains(&"radroots sync status get".to_owned())
        );
        assert!(view.actions.contains(&"radroots account list".to_owned()));
        assert!(
            view.actions
                .iter()
                .any(|action| action == &format!("radroots --account-id {other_pubkey} sync push"))
        );
    }

    #[test]
    fn sync_push_failed_publish_leaves_pending_state_retryable() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::local::init(&config).expect("store init");
        let signing =
            crate::runtime::accounts::create_or_migrate_default_account(&config).expect("account");
        seed_replica_farm(
            &config,
            signing
                .account
                .record
                .public_identity
                .public_key_hex
                .as_str(),
        );

        let view = push_with_publisher(&config, |_, relays, _| {
            Err(super::DirectRelayPublishError::Publish {
                event_id: "0".repeat(64),
                reason: "relay refused event".to_owned(),
                target_relays: relays.to_vec(),
                connected_relays: relays.to_vec(),
                failed_relays: vec![DirectRelayFailure {
                    relay: relays[0].clone(),
                    reason: "relay refused event".to_owned(),
                }],
            })
        })
        .expect("sync push failure view");
        let status = radroots_replica_sync_status(
            &SqliteExecutor::open(&config.local.replica_db_path).expect("open replica"),
        )
        .expect("status");

        assert_eq!(view.state, "unavailable");
        assert_eq!(view.published_count, Some(0));
        assert_eq!(view.failed_count, Some(1));
        assert!(status.pending_count > 0);
    }

    #[test]
    fn sync_push_rejects_radrootsd_before_store_relay_or_signer_checks() {
        let dir = tempdir().expect("tempdir");
        let mut config = sample_config(dir.path(), Vec::new());
        config.publish.mode = PublishMode::Radrootsd;

        let view = push_with_publisher(&config, |_, _, _| {
            panic!("radrootsd sync push must not publish")
        })
        .expect("radrootsd sync push view");

        assert_eq!(view.state, "unavailable");
        assert_eq!(view.replica_db, "not_checked");
        assert_eq!(view.relay_count, 0);
        assert_eq!(
            view.reason.as_deref(),
            Some(super::RADROOTSD_SYNC_PUSH_UNAVAILABLE_REASON)
        );
        assert_eq!(
            view.actions,
            vec!["radroots --publish-mode nostr_relay sync push"]
        );
        assert!(!config.local.replica_db_path.exists());
    }

    #[test]
    fn sync_pull_ingests_relay_events_and_market_reads_without_daemon() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::local::init(&config).expect("store init");
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
        crate::runtime::local::init(&config).expect("store init");
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
    fn relay_ingest_splits_unsupported_and_failed_events() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path(), vec!["wss://relay.example.com".to_owned()]);
        crate::runtime::local::init(&config).expect("store init");
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
    ) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError> {
        move |relays, _| {
            Ok(DirectRelayFetchReceipt {
                target_relays: relays.to_vec(),
                connected_relays: relays.to_vec(),
                failed_relays: Vec::new(),
                events,
            })
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
        signed_event(
            identity,
            WireEventParts {
                kind: KIND_LISTING,
                tags: vec![
                    vec!["d".to_owned(), LISTING_D_TAG.to_owned()],
                    vec![
                        "a".to_owned(),
                        format!("{}:{}:{}", KIND_FARM, identity.public_key_hex(), FARM_D_TAG),
                    ],
                    vec!["p".to_owned(), identity.public_key_hex()],
                    vec!["key".to_owned(), "pasture-eggs".to_owned()],
                    vec!["title".to_owned(), "Pasture Eggs".to_owned()],
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
                content: "# Pasture Eggs".to_owned(),
            },
        )
    }

    fn signed_event(identity: &RadrootsIdentity, parts: WireEventParts) -> RadrootsNostrEvent {
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("event builder")
            .sign_with_keys(identity.keys())
            .expect("signed event")
    }

    fn seed_replica_farm(config: &RuntimeConfig, pubkey: &str) {
        let executor = SqliteExecutor::open(&config.local.replica_db_path).expect("open replica");
        migrations::run_all_up(&executor).expect("migrations");
        let _ = farm::create(
            &executor,
            &IFarmFields {
                d_tag: FARM_D_TAG.to_owned(),
                pubkey: pubkey.to_owned(),
                name: "Local Farm".to_owned(),
                about: Some("local replica farm".to_owned()),
                website: None,
                picture: None,
                banner: None,
                location_primary: None,
                location_city: None,
                location_region: None,
                location_country: None,
            },
        )
        .expect("farm");
    }

    fn seed_replica_member_claim(config: &RuntimeConfig, member_pubkey: &str, farm_pubkey: &str) {
        let executor = SqliteExecutor::open(&config.local.replica_db_path).expect("open replica");
        migrations::run_all_up(&executor).expect("migrations");
        let _ = farm_member_claim::create(
            &executor,
            &IFarmMemberClaimFields {
                member_pubkey: member_pubkey.to_owned(),
                farm_pubkey: farm_pubkey.to_owned(),
            },
        )
        .expect("member claim");
    }

    fn identity(seed: u8) -> RadrootsIdentity {
        RadrootsIdentity::from_secret_key_bytes(&[seed; 32]).expect("identity")
    }

    fn sample_config(root: &Path, relays: Vec<String>) -> RuntimeConfig {
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
                urls: relays,
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
}
