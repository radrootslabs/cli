//! CLI scheduling and presentation over the canonical shared sync engine.

use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    cli::global::SyncWatchArgs,
    runtime::{
        RuntimeError,
        config::{RuntimeConfig, TransportProfileKind},
        sdk::{CliSdkAdapterError, CliSdkSession, sync_targets},
    },
    view::runtime::{
        SyncActionView, SyncFreshnessView, SyncQueueView, SyncStatusView,
        SyncTransportStatusInspectView, SyncTransportTargetView, SyncWatchFrameView, SyncWatchView,
        TransportOperationCapabilitiesView, TransportTargetFailureView,
    },
};
use radroots_replica_store::ReplicaSql;
#[cfg(test)]
use radroots_replica_store::migrations;
use radroots_sql_core::{SqlExecutor, SqlxSqliteExecutor};
use radroots_storage::outbox::LeaseOwner;
use radroots_sync::{
    PullRequest, SyncStatus,
    ingest::RegistryPolicy,
    policy::SyncId,
    pull::PullTermination,
    push::{DeliveryRunReceipt, DeliveryRunRequest},
};
use radroots_transport::{
    SinkStatus, SourceStatus, Target,
    capability::{Availability, Maturity},
    outcome::FetchTargetState,
};

const SDK_SYNC_SOURCE: &str = "canonical SDK sync engine";
const MARKET_FRESHNESS_STALE_AFTER_SECONDS: u64 = 15 * 60;
const SYNC_PULL_FRESHNESS_STALE_AFTER_SECONDS: u64 = 30 * 60;
const PULL_PAGE_LIMIT: u16 = 1_000;
const PULL_MAX_PAGES: u16 = 5;
const DELIVERY_LIMIT: u16 = 1_000;
const DELIVERY_LEASE_MS: u64 = 30_000;

pub fn status(config: &RuntimeConfig) -> Result<SyncStatusView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let receipt = canonical_status(&session)?;
    Ok(status_view(config, &receipt))
}

pub fn pull(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    pull_for_scope(config, RelayIngestScope::SyncPull)
}

pub fn market_refresh(config: &RuntimeConfig) -> Result<SyncActionView, RuntimeError> {
    pull_for_scope(config, RelayIngestScope::MarketPull)
}

fn pull_for_scope(
    config: &RuntimeConfig,
    scope: RelayIngestScope,
) -> Result<SyncActionView, RuntimeError> {
    let session = CliSdkSession::connect(config).map_err(adapter_runtime_error)?;
    let before = canonical_status(&session).map_err(adapter_runtime_error)?;
    if !nostr_targets_configured(config) {
        return Ok(empty_action(
            config,
            &before,
            "pull",
            "unconfigured",
            Some("sync pull requires at least one configured Nostr relay".to_owned()),
        ));
    }
    if config.output.dry_run {
        let mut view = empty_action(
            config,
            &before,
            "pull",
            "dry_run",
            Some("dry run requested; canonical sync pull skipped".to_owned()),
        );
        view.target_transport_endpoints = config.transport.nostr_relay_urls.clone();
        view.fetched_count = Some(0);
        view.ingested_count = Some(0);
        view.skipped_count = Some(0);
        view.unsupported_count = Some(0);
        view.failed_count = Some(0);
        view.reason_code = Some("dry_run".to_owned());
        return Ok(view);
    }

    let targets = sync_targets(config).map_err(|error| RuntimeError::Config(error.to_string()))?;
    let request = PullRequest::new(targets.clone(), PULL_PAGE_LIMIT, PULL_MAX_PAGES)
        .map_err(|error| RuntimeError::Config(error.to_string()))?;
    let operations = session
        .sdk()
        .sync()
        .map_err(CliSdkAdapterError::from)
        .map_err(adapter_runtime_error)?
        .ok_or_else(|| {
            RuntimeError::Config("canonical sync engine is not configured".to_owned())
        })?;
    let receipt = session
        .block_on(operations.pull(request, &RegistryPolicy::verified()))
        .map_err(|error| adapter_runtime_error(error.into()))?;
    let after = canonical_status(&session).map_err(adapter_runtime_error)?;
    let ingested = receipt
        .ingest_outcomes()
        .iter()
        .filter(|item| item.is_ok())
        .count();
    let ingest_failed = receipt.ingest_outcomes().len().saturating_sub(ingested);
    let mut failed_targets = Vec::new();
    for outcome in receipt.target_outcomes() {
        if matches!(outcome.state(), FetchTargetState::Complete) {
            continue;
        }
        let target = targets
            .targets()
            .iter()
            .find(|target| target.fingerprint() == outcome.target());
        failed_targets.push(TransportTargetFailureView {
            transport_kind: target
                .map_or("nostr", |target| target.kind().as_str())
                .to_owned(),
            endpoint_uri: target.map_or_else(
                || outcome.target().as_str().to_owned(),
                |target| target.uri().as_str().to_owned(),
            ),
            target_scope: target
                .and_then(|target| target.scope())
                .map(|value| value.as_str().to_owned()),
            target_label: target
                .and_then(|target| target.label())
                .map(|value| value.as_str().to_owned()),
            transport_outcome_kind: Some(fetch_state_label(outcome.state()).to_owned()),
            reason: outcome
                .message()
                .unwrap_or(fetch_state_label(outcome.state()))
                .to_owned(),
        });
    }
    let failed_count = ingest_failed + failed_targets.len();
    let state = if receipt.termination() == PullTermination::SourceFailed {
        "unavailable"
    } else if failed_count > 0 || receipt.termination() != PullTermination::Complete {
        "partial"
    } else {
        "ready"
    };
    let reason = (state != "ready").then(|| {
        format!(
            "canonical pull terminated as {} with {failed_count} failed outcome(s)",
            pull_termination_label(receipt.termination())
        )
    });
    let mut view = empty_action(config, &after, "pull", state, reason);
    view.target_transport_endpoints = config.transport.nostr_relay_urls.clone();
    view.attempted_transport_endpoints = config.transport.nostr_relay_urls.clone();
    view.accepted_transport_endpoints = config
        .transport
        .nostr_relay_urls
        .iter()
        .filter(|endpoint| {
            !failed_targets
                .iter()
                .any(|failure| &failure.endpoint_uri == *endpoint)
        })
        .cloned()
        .collect();
    view.failed_transport_targets = failed_targets;
    view.fetched_count = Some(receipt.events_observed());
    view.ingested_count = Some(ingested);
    view.skipped_count = Some(0);
    view.unsupported_count = Some(0);
    view.failed_count = Some(failed_count);
    view.reason_code =
        (state != "ready").then(|| pull_termination_label(receipt.termination()).to_owned());
    view.actions = vec![scope.ready_action().to_owned()];
    Ok(view)
}

pub fn push(config: &RuntimeConfig) -> Result<SyncActionView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let before = canonical_status(&session)?;
    if config.output.dry_run {
        return Ok(empty_action(
            config,
            &before,
            "push",
            "dry_run",
            Some("dry run requested; canonical outbox delivery skipped".to_owned()),
        ));
    }
    let receipt = deliver_pending(&session)?;
    let after = canonical_status(&session)?;
    Ok(delivery_action(config, &after, &receipt))
}

pub fn watch(config: &RuntimeConfig, args: &SyncWatchArgs) -> Result<SyncWatchView, RuntimeError> {
    if args.frames == 0 {
        return Err(RuntimeError::Config(
            "`sync watch --frames` must be greater than 0".to_owned(),
        ));
    }
    let mut frames = Vec::with_capacity(args.frames);
    let mut final_view = None;
    for index in 0..args.frames {
        let view = status(config).map_err(adapter_runtime_error)?;
        frames.push(SyncWatchFrameView {
            sequence: index + 1,
            observed_at: unix_now(),
            state: view.state.clone(),
            configured_transport_target_count: view.configured_transport_target_count,
            freshness: view.freshness.clone(),
            queue: view.queue.clone(),
        });
        final_view = Some(view);
        if index + 1 < args.frames {
            thread::sleep(Duration::from_millis(args.interval_ms));
        }
    }
    let view = final_view
        .ok_or_else(|| RuntimeError::Config("sync watch produced no frames".to_owned()))?;
    Ok(SyncWatchView {
        state: view.state,
        source: view.source,
        interval_ms: args.interval_ms,
        frames,
        reason: view.reason,
        actions: view.actions,
    })
}

pub(crate) fn canonical_status(session: &CliSdkSession) -> Result<SyncStatus, CliSdkAdapterError> {
    let operations = session.sdk().sync()?.ok_or_else(|| {
        RuntimeError::Config("canonical sync engine is not configured".to_owned())
    })?;
    Ok(session.block_on(operations.status(&[]))?)
}

pub(crate) fn deliver_pending(
    session: &CliSdkSession,
) -> Result<DeliveryRunReceipt, CliSdkAdapterError> {
    let mut seed = [0_u8; 16];
    getrandom::getrandom(&mut seed).map_err(|error| {
        RuntimeError::Config(format!(
            "failed to generate delivery lease identity: {error}"
        ))
    })?;
    let owner = LeaseOwner::parse(format!("radroots-cli-{}", std::process::id()))
        .map_err(|error| RuntimeError::Config(error.to_string()))?;
    let request =
        DeliveryRunRequest::new(owner, SyncId::new(seed)?, DELIVERY_LEASE_MS, DELIVERY_LIMIT)?;
    let operations = session.sdk().sync()?.ok_or_else(|| {
        RuntimeError::Config("canonical sync engine is not configured".to_owned())
    })?;
    Ok(session.block_on(operations.deliver_pending(request))?)
}

fn status_view(config: &RuntimeConfig, status: &SyncStatus) -> SyncStatusView {
    let configured_targets = configured_targets(config);
    let state = sync_health_label(status);
    SyncStatusView {
        state: state.to_owned(),
        source: SDK_SYNC_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_store: "canonical".to_owned(),
        configured_transport_target_count: configured_targets.len(),
        configured_transport_targets: configured_targets,
        transport_statuses: transport_status_views(status),
        publish_policy: "canonical satisfaction policy".to_owned(),
        freshness: missing_freshness(),
        queue: queue_view(status),
        reason: (!nostr_targets_configured(config))
            .then(|| "no Nostr relay targets are configured".to_owned()),
        actions: if !nostr_targets_configured(config) {
            vec!["radroots transport config update --kind nostr --nostr-relay wss://relay.example.com".to_owned()]
        } else {
            vec![
                "radroots sync pull".to_owned(),
                "radroots sync push".to_owned(),
            ]
        },
    }
}

fn empty_action(
    config: &RuntimeConfig,
    status: &SyncStatus,
    direction: &str,
    state: &str,
    reason: Option<String>,
) -> SyncActionView {
    let snapshot = status_view(config, status);
    SyncActionView {
        direction: direction.to_owned(),
        state: state.to_owned(),
        source: SDK_SYNC_SOURCE.to_owned(),
        local_root: snapshot.local_root,
        replica_store: snapshot.replica_store,
        configured_transport_target_count: snapshot.configured_transport_target_count,
        configured_transport_targets: snapshot.configured_transport_targets,
        transport_statuses: snapshot.transport_statuses,
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
        reason,
        actions: snapshot.actions,
    }
}

fn delivery_action(
    config: &RuntimeConfig,
    status: &SyncStatus,
    receipt: &DeliveryRunReceipt,
) -> SyncActionView {
    let attempted = receipt.outcomes().len();
    let succeeded = receipt.succeeded();
    let failed = receipt.failed();
    let state = if attempted == 0 {
        "ready"
    } else if failed == 0 {
        "published"
    } else if succeeded > 0 {
        "partial"
    } else {
        "unavailable"
    };
    let mut view = empty_action(
        config,
        status,
        "push",
        state,
        (failed > 0).then(|| format!("{failed} canonical delivery outcome(s) failed")),
    );
    view.target_transport_endpoints = config.transport.nostr_relay_urls.clone();
    view.attempted_transport_endpoints = if attempted > 0 {
        config.transport.nostr_relay_urls.clone()
    } else {
        Vec::new()
    };
    view.accepted_transport_endpoints = if succeeded > 0 {
        config.transport.nostr_relay_urls.clone()
    } else {
        Vec::new()
    };
    view.publishable_count = Some(attempted);
    view.published_count = Some(succeeded);
    view.failed_count = Some(failed);
    view.reason_code = (failed > 0).then(|| "delivery_partial_failure".to_owned());
    view.actions = vec!["radroots sync status".to_owned()];
    view
}

fn queue_view(status: &SyncStatus) -> SyncQueueView {
    let outbox = status.outbox();
    let total = outbox.total().and_then(|value| usize::try_from(value).ok());
    SyncQueueView {
        expected_count: total.unwrap_or_default(),
        pending_count: usize::try_from(outbox.pending + outbox.leased + outbox.retryable)
            .unwrap_or(usize::MAX),
        total_count: total,
        retryable_count: usize::try_from(outbox.retryable).ok(),
        terminal_count: usize::try_from(outbox.satisfied + outbox.exhausted).ok(),
        failed_terminal_count: usize::try_from(outbox.exhausted).ok(),
        deferred_until_implemented_count: Some(0),
        ready_signed_count: usize::try_from(outbox.pending + outbox.retryable).ok(),
        publishing_count: usize::try_from(outbox.leased).ok(),
        last_attempt_at_ms: None,
        last_error: None,
    }
}

fn configured_targets(config: &RuntimeConfig) -> Vec<SyncTransportTargetView> {
    config
        .transport
        .nostr_relay_urls
        .iter()
        .filter_map(|endpoint| Target::nostr_relay(endpoint).ok())
        .map(|target| SyncTransportTargetView {
            transport_kind: target.kind().as_str().to_owned(),
            endpoint_uri: target.uri().as_str().to_owned(),
            endpoint_fingerprint: target.fingerprint().as_str().to_owned(),
            target_scope: target.scope().map(|scope| scope.as_str().to_owned()),
            target_label: target.label().map(|label| label.as_str().to_owned()),
        })
        .collect()
}

fn transport_status_views(status: &SyncStatus) -> Vec<SyncTransportStatusInspectView> {
    let mut views = Vec::new();
    if let Some(source) = status.source().status() {
        views.push(source_status_view(source));
    }
    if let Some(sink) = status.sink().status()
        && !views
            .iter()
            .any(|view| view.transport == sink.transport_id().as_str())
    {
        views.push(sink_status_view(sink));
    }
    views
}

fn source_status_view(status: &SourceStatus) -> SyncTransportStatusInspectView {
    SyncTransportStatusInspectView {
        transport: status.transport_id().as_str().to_owned(),
        profile_id: None,
        endpoint_uri: None,
        configured: status.is_configured(),
        implementation: "real".to_owned(),
        maturity: maturity_label(status.maturity()).to_owned(),
        availability: availability_label(status.availability()).to_owned(),
        usable_for_delivery: false,
        capabilities: TransportOperationCapabilitiesView {
            deliver: false,
            fetch: status.capabilities().can_fetch(),
        },
        message: status.message().to_owned(),
    }
}

fn sink_status_view(status: &SinkStatus) -> SyncTransportStatusInspectView {
    SyncTransportStatusInspectView {
        transport: status.transport_id().as_str().to_owned(),
        profile_id: None,
        endpoint_uri: None,
        configured: status.is_configured(),
        implementation: "real".to_owned(),
        maturity: maturity_label(status.maturity()).to_owned(),
        availability: availability_label(status.availability()).to_owned(),
        usable_for_delivery: status.capabilities().can_deliver()
            && status.availability() != Availability::Unavailable,
        capabilities: TransportOperationCapabilitiesView {
            deliver: status.capabilities().can_deliver(),
            fetch: false,
        },
        message: status.message().to_owned(),
    }
}

fn sync_health_label(status: &SyncStatus) -> &'static str {
    match status.health() {
        radroots_protocol::runtime::v1::SyncHealth::Healthy => "ready",
        radroots_protocol::runtime::v1::SyncHealth::Degraded => "degraded",
        radroots_protocol::runtime::v1::SyncHealth::Unavailable => "unavailable",
    }
}

fn maturity_label(value: Maturity) -> &'static str {
    match value {
        Maturity::Experimental => "experimental",
        Maturity::Preview => "preview",
        Maturity::Stable => "stable",
    }
}

fn availability_label(value: Availability) -> &'static str {
    match value {
        Availability::Available => "available",
        Availability::Degraded => "degraded",
        Availability::Unavailable => "unavailable",
    }
}

fn fetch_state_label(value: FetchTargetState) -> &'static str {
    match value {
        FetchTargetState::Complete => "complete",
        FetchTargetState::Partial => "partial",
        FetchTargetState::Unavailable => "unavailable",
        FetchTargetState::FailedRetryable => "failed_retryable",
        FetchTargetState::FailedTerminal => "failed_terminal",
        FetchTargetState::Cancelled => "cancelled",
    }
}

fn pull_termination_label(value: PullTermination) -> &'static str {
    match value {
        PullTermination::Complete => "complete",
        PullTermination::PageLimit => "page_limit",
        PullTermination::Deadline => "deadline",
        PullTermination::Cancelled => "cancelled",
        PullTermination::SourceFailed => "source_failed",
    }
}

fn nostr_targets_configured(config: &RuntimeConfig) -> bool {
    matches!(
        config.transport.profile,
        TransportProfileKind::Nostr | TransportProfileKind::MultiTarget
    ) && !config.transport.nostr_relay_urls.is_empty()
}

fn adapter_runtime_error(error: CliSdkAdapterError) -> RuntimeError {
    RuntimeError::Network(error.to_string())
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
    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
    migrations::run_all_up(&executor)?;
    freshness_for_scope_from_executor(config, &executor, scope)
}

#[cfg(test)]
pub(crate) fn relay_provenance_relays_for_scope(
    _config: &RuntimeConfig,
    _scope: RelayIngestScope,
) -> Result<Vec<String>, RuntimeError> {
    Ok(Vec::new())
}

pub(crate) fn freshness_for_scope_from_executor(
    _config: &RuntimeConfig,
    executor: &SqlxSqliteExecutor,
    scope: RelayIngestScope,
) -> Result<SyncFreshnessView, RuntimeError> {
    let last_event_at = ReplicaSql::new(executor).nostr_event_last_created_at()?;
    let age_seconds = last_event_at.map(|last| unix_now().saturating_sub(last));
    let state = match age_seconds {
        None => "never",
        Some(age) if age > scope.stale_after_seconds() => "stale",
        Some(_) => "fresh",
    };
    Ok(SyncFreshnessView {
        state: state.to_owned(),
        display: match age_seconds {
            Some(age) => format!("{} {state} {}s ago", scope.display(), age),
            None => format!("{} never synced", scope.display()),
        },
        age_seconds,
        last_event_at,
        run: None,
    })
}

pub(crate) fn freshness_requires_refresh(freshness: &SyncFreshnessView) -> bool {
    matches!(
        freshness.state.as_str(),
        "never" | "stale" | "relay_set_changed" | "refresh_failed"
    )
}

pub(crate) fn ensure_sync_run_table(executor: &SqlxSqliteExecutor) -> Result<(), RuntimeError> {
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

#[derive(Debug, Clone, Copy)]
pub(crate) enum RelayIngestScope {
    SyncPull,
    MarketPull,
}

impl RelayIngestScope {
    fn display(self) -> &'static str {
        match self {
            Self::SyncPull => "sync pull",
            Self::MarketPull => "market refresh",
        }
    }

    fn stale_after_seconds(self) -> u64 {
        match self {
            Self::SyncPull => SYNC_PULL_FRESHNESS_STALE_AFTER_SECONDS,
            Self::MarketPull => MARKET_FRESHNESS_STALE_AFTER_SECONDS,
        }
    }

    fn ready_action(self) -> &'static str {
        match self {
            Self::SyncPull => "radroots market search eggs",
            Self::MarketPull => "radroots market search eggs",
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_labels_cover_partial_failure_and_cancellation() {
        assert_eq!(
            pull_termination_label(PullTermination::Cancelled),
            "cancelled"
        );
        assert_eq!(
            fetch_state_label(FetchTargetState::FailedRetryable),
            "failed_retryable"
        );
        assert_eq!(fetch_state_label(FetchTargetState::Partial), "partial");
    }

    #[test]
    fn missing_freshness_requires_a_pull() {
        assert!(freshness_requires_refresh(&missing_freshness()));
    }
}
