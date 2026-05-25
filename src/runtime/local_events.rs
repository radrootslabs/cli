use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_local_events::{
    LocalEventRecord, LocalEventRecordInput, LocalEventsStore, LocalRecordFamily,
    LocalRecordStatus, PublishOutboxStatus, RelayDeliveryEvidence, RelayDeliveryFailure,
    SourceRuntime,
};
use radroots_runtime_paths::{
    default_shared_local_events_database_path_from_shared_accounts_data_root,
    default_shared_local_events_root_from_shared_accounts_data_root,
};
use radroots_sql_core::SqliteExecutor;
use serde_json::{Value, json};

use crate::runtime::RuntimeError;
use crate::runtime::config::{PathsConfig, RuntimeConfig};
use crate::runtime::direct_relay::{DirectRelayFailure, DirectRelayPublishError};

static RECORD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn append_local_work(
    config: &RuntimeConfig,
    subject: &str,
    owner_account_id: Option<String>,
    owner_pubkey: Option<String>,
    farm_id: Option<String>,
    listing_addr: Option<String>,
    payload: Value,
) -> Result<LocalEventRecord, RuntimeError> {
    let timestamp = current_time_ms()?;
    let sequence = RECORD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let input = LocalEventRecordInput {
        record_id: format!("cli:local_work:{subject}:{timestamp}:{sequence}"),
        family: LocalRecordFamily::LocalWork,
        status: LocalRecordStatus::LocalSaved,
        source_runtime: SourceRuntime::Cli,
        created_at_ms: timestamp,
        inserted_at_ms: timestamp,
        owner_account_id,
        owner_pubkey,
        farm_id,
        listing_addr,
        local_work_json: Some(payload),
        event_id: None,
        event_kind: None,
        event_pubkey: None,
        event_created_at: None,
        event_tags_json: None,
        event_content: None,
        event_sig: None,
        raw_event_json: None,
        outbox_status: PublishOutboxStatus::None,
        relay_set_fingerprint: None,
        relay_delivery_json: None,
    };
    let store = open_store(config)?;
    Ok(store.append_record(&input)?)
}

pub fn append_signed_event(
    config: &RuntimeConfig,
    subject: &str,
    owner_account_id: Option<String>,
    owner_pubkey: Option<String>,
    farm_id: Option<String>,
    listing_addr: Option<String>,
    event: &radroots_nostr::prelude::RadrootsNostrEvent,
) -> Result<LocalEventRecord, RuntimeError> {
    let timestamp = current_time_ms()?;
    let delivery_evidence = RelayDeliveryEvidence::pending(&config.relay.urls)?;
    let relay_set = delivery_evidence.relay_set_fingerprint();
    let input = LocalEventRecordInput {
        record_id: format!("cli:signed_event:{subject}:{}", event.id.to_hex()),
        family: LocalRecordFamily::SignedEvent,
        status: LocalRecordStatus::PendingPublish,
        source_runtime: SourceRuntime::Cli,
        created_at_ms: timestamp,
        inserted_at_ms: timestamp,
        owner_account_id,
        owner_pubkey,
        farm_id,
        listing_addr,
        local_work_json: None,
        event_id: Some(event.id.to_hex()),
        event_kind: Some(i64::from(u32::from(event.kind.as_u16()))),
        event_pubkey: Some(event.pubkey.to_string()),
        event_created_at: Some(event_created_at_i64(event)?),
        event_tags_json: Some(json!(event_tags(event))),
        event_content: Some(event.content.clone()),
        event_sig: Some(event.sig.to_string()),
        raw_event_json: Some(raw_event_json(event)?),
        outbox_status: PublishOutboxStatus::Pending,
        relay_set_fingerprint: relay_set,
        relay_delivery_json: Some(delivery_evidence.to_json_value()?),
    };
    let store = open_store(config)?;
    Ok(store.append_record(&input)?)
}

pub fn mark_signed_event_acknowledged(
    config: &RuntimeConfig,
    record_id: &str,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    acknowledged_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> Result<LocalEventRecord, RuntimeError> {
    let delivery_evidence = acknowledged_delivery_evidence(
        target_relays,
        connected_relays,
        acknowledged_relays,
        failed_relays,
    )?;
    update_signed_event_outbox(
        config,
        record_id,
        LocalRecordStatus::Published,
        PublishOutboxStatus::Acknowledged,
        delivery_evidence,
    )
}

pub fn mark_signed_event_failed(
    config: &RuntimeConfig,
    record_id: &str,
    reason: String,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> Result<LocalEventRecord, RuntimeError> {
    let delivery_evidence = failed_delivery_evidence(
        target_relays,
        connected_relays,
        failed_relays,
        reason.as_str(),
    )?;
    update_signed_event_outbox(
        config,
        record_id,
        LocalRecordStatus::Failed,
        PublishOutboxStatus::Failed,
        delivery_evidence,
    )
}

pub fn mark_signed_event_failed_for_publish_error(
    config: &RuntimeConfig,
    record_id: &str,
    error: &DirectRelayPublishError,
) -> Result<LocalEventRecord, RuntimeError> {
    let (target_relays, connected_relays, failed_relays) =
        publish_error_delivery_parts(error, &config.relay.urls);
    mark_signed_event_failed(
        config,
        record_id,
        error.to_string(),
        target_relays,
        connected_relays,
        failed_relays,
    )
}

pub fn shared_local_events_db_path(config: &RuntimeConfig) -> Result<PathBuf, RuntimeError> {
    shared_local_events_db_path_from_paths(&config.paths)
}

fn shared_local_events_db_path_from_paths(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    default_shared_local_events_database_path_from_shared_accounts_data_root(
        &paths.shared_accounts_data_root,
    )
    .map_err(|err| {
        RuntimeError::Config(format!("resolve shared local-events database path: {err}"))
    })
}

pub fn list_shared_records_latest(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<LocalEventRecord>, RuntimeError> {
    let database_path = shared_local_events_db_path(config)?;
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let executor = SqliteExecutor::open(database_path)?;
    let store = LocalEventsStore::new(executor);
    Ok(store.list_records_changed_latest(limit)?)
}

pub fn list_shared_records_before(
    config: &RuntimeConfig,
    before_change_seq: i64,
    before_seq: i64,
    limit: u32,
) -> Result<Vec<LocalEventRecord>, RuntimeError> {
    let database_path = shared_local_events_db_path(config)?;
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let executor = SqliteExecutor::open(database_path)?;
    let store = LocalEventsStore::new(executor);
    Ok(store.list_records_changed_before(before_change_seq, before_seq, limit)?)
}

pub fn get_shared_record(
    config: &RuntimeConfig,
    record_id: &str,
) -> Result<Option<LocalEventRecord>, RuntimeError> {
    let database_path = shared_local_events_db_path(config)?;
    if !database_path.exists() {
        return Ok(None);
    }
    let executor = SqliteExecutor::open(database_path)?;
    let store = LocalEventsStore::new(executor);
    Ok(store.get_record(record_id)?)
}

fn update_signed_event_outbox(
    config: &RuntimeConfig,
    record_id: &str,
    status: LocalRecordStatus,
    outbox_status: PublishOutboxStatus,
    delivery_evidence: RelayDeliveryEvidence,
) -> Result<LocalEventRecord, RuntimeError> {
    let relay_set_fingerprint = delivery_evidence.relay_set_fingerprint();
    let relay_delivery_json = delivery_evidence.to_json_value()?;
    let store = open_store(config)?;
    Ok(
        store.update_outbox(&radroots_local_events::LocalEventRecordUpdate {
            record_id: record_id.to_owned(),
            status,
            outbox_status,
            relay_set_fingerprint,
            relay_delivery_json: Some(relay_delivery_json),
            updated_at_ms: current_time_ms()?,
        })?,
    )
}

fn open_store(config: &RuntimeConfig) -> Result<LocalEventsStore<SqliteExecutor>, RuntimeError> {
    let root = shared_local_events_root_from_paths(&config.paths)?;
    fs::create_dir_all(&root)?;
    let executor = SqliteExecutor::open(shared_local_events_db_path_from_paths(&config.paths)?)?;
    let store = LocalEventsStore::new(executor);
    store.migrate_up()?;
    Ok(store)
}

fn shared_local_events_root_from_paths(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    default_shared_local_events_root_from_shared_accounts_data_root(
        &paths.shared_accounts_data_root,
    )
    .map_err(|err| RuntimeError::Config(format!("resolve shared local-events root: {err}")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{
        acknowledged_delivery_evidence, failed_delivery_evidence,
        shared_local_events_db_path_from_paths, shared_local_events_root_from_paths,
    };
    use crate::runtime::config::PathsConfig;
    use crate::runtime::direct_relay::DirectRelayFailure;

    #[test]
    fn shared_local_events_paths_use_shared_runtime_contract() {
        let paths = paths_config("/repo/infra/local/runtime/radroots/data/shared/accounts");

        assert_eq!(
            shared_local_events_root_from_paths(&paths).expect("shared local-events root"),
            PathBuf::from("/repo/infra/local/runtime/radroots/data/shared/local_events")
        );
        assert_eq!(
            shared_local_events_db_path_from_paths(&paths).expect("shared local-events database"),
            PathBuf::from(
                "/repo/infra/local/runtime/radroots/data/shared/local_events/local_events.sqlite"
            )
        );
    }

    #[test]
    fn acknowledged_delivery_evidence_uses_actual_target_relays() {
        let evidence = acknowledged_delivery_evidence(
            vec![
                "wss://actual-a.example".to_owned(),
                "wss://actual-b.example".to_owned(),
            ],
            vec!["wss://actual-a.example".to_owned()],
            vec!["wss://actual-a.example".to_owned()],
            vec![DirectRelayFailure {
                relay: "wss://actual-b.example".to_owned(),
                reason: "timeout".to_owned(),
            }],
        )
        .expect("acknowledged evidence");

        assert_eq!(
            evidence.relay_set_fingerprint(),
            radroots_local_events::canonical_relay_set_fingerprint([
                "wss://actual-a.example",
                "wss://actual-b.example"
            ])
        );
        assert_eq!(
            evidence.to_json_value().expect("delivery json"),
            json!({
                "state": "acknowledged",
                "target_relays": ["wss://actual-a.example", "wss://actual-b.example"],
                "connected_relays": ["wss://actual-a.example"],
                "acknowledged_relays": ["wss://actual-a.example"],
                "failed_relays": [
                    {"relay_url": "wss://actual-b.example", "error": "timeout"}
                ]
            })
        );
    }

    #[test]
    fn failed_delivery_evidence_synthesizes_target_failures_when_transport_has_none() {
        let evidence = failed_delivery_evidence(
            vec![
                "wss://actual-a.example".to_owned(),
                "wss://actual-b.example".to_owned(),
            ],
            Vec::new(),
            Vec::new(),
            "publish failed",
        )
        .expect("failed evidence");

        assert_eq!(
            evidence.to_json_value().expect("delivery json"),
            json!({
                "state": "failed",
                "target_relays": ["wss://actual-a.example", "wss://actual-b.example"],
                "connected_relays": [],
                "acknowledged_relays": [],
                "failed_relays": [
                    {"relay_url": "wss://actual-a.example", "error": "publish failed"},
                    {"relay_url": "wss://actual-b.example", "error": "publish failed"}
                ]
            })
        );
    }

    fn paths_config(shared_accounts_data_root: &str) -> PathsConfig {
        PathsConfig {
            profile: "repo_local".to_owned(),
            profile_source: "test".to_owned(),
            allowed_profiles: vec!["repo_local".to_owned()],
            root_source: "repo_local_root".to_owned(),
            repo_local_root: Some(PathBuf::from("/repo/infra/local/runtime/radroots")),
            repo_local_root_source: Some("test".to_owned()),
            subordinate_path_override_source: "runtime_config".to_owned(),
            app_namespace: "apps/cli".to_owned(),
            shared_accounts_namespace: "shared/accounts".to_owned(),
            shared_identities_namespace: "shared/identities".to_owned(),
            app_config_path: PathBuf::from(
                "/repo/infra/local/runtime/radroots/config/apps/cli/config.toml",
            ),
            workspace_config_path: None,
            app_data_root: PathBuf::from("/repo/infra/local/runtime/radroots/data/apps/cli"),
            app_logs_root: PathBuf::from("/repo/infra/local/runtime/radroots/logs/apps/cli"),
            shared_accounts_data_root: PathBuf::from(shared_accounts_data_root),
            shared_accounts_secrets_root: PathBuf::from(
                "/repo/infra/local/runtime/radroots/secrets/shared/accounts",
            ),
            default_identity_path: PathBuf::from(
                "/repo/infra/local/runtime/radroots/secrets/shared/identities/default.json",
            ),
        }
    }
}

fn current_time_ms() -> Result<i64, RuntimeError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            RuntimeError::Config(format!("system clock is before unix epoch: {error}"))
        })?;
    i64::try_from(duration.as_millis())
        .map_err(|_| RuntimeError::Config("current timestamp exceeds i64 milliseconds".to_owned()))
}

fn publish_error_delivery_parts(
    error: &DirectRelayPublishError,
    relay_urls: &[String],
) -> (Vec<String>, Vec<String>, Vec<DirectRelayFailure>) {
    match error {
        DirectRelayPublishError::MissingRelays
        | DirectRelayPublishError::Runtime(_)
        | DirectRelayPublishError::Build(_)
        | DirectRelayPublishError::Sign(_) => (relay_urls.to_vec(), Vec::new(), Vec::new()),
        DirectRelayPublishError::RelayConfig { relay, source } => (
            relay_urls.to_vec(),
            Vec::new(),
            vec![DirectRelayFailure {
                relay: relay.clone(),
                reason: source.to_string(),
            }],
        ),
        DirectRelayPublishError::Connect {
            target_relays,
            connected_relays,
            failed_relays,
            ..
        }
        | DirectRelayPublishError::Publish {
            target_relays,
            connected_relays,
            failed_relays,
            ..
        } => (
            target_relays.clone(),
            connected_relays.clone(),
            failed_relays.clone(),
        ),
    }
}

fn acknowledged_delivery_evidence(
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    acknowledged_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> Result<RelayDeliveryEvidence, RuntimeError> {
    RelayDeliveryEvidence::acknowledged(
        target_relays,
        connected_relays,
        acknowledged_relays,
        relay_delivery_failures(failed_relays)?,
    )
    .map_err(Into::into)
}

fn failed_delivery_evidence(
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
    reason: &str,
) -> Result<RelayDeliveryEvidence, RuntimeError> {
    let delivery_failures = failed_delivery_failures(failed_relays, &target_relays, reason)?;
    RelayDeliveryEvidence::failed(target_relays, connected_relays, delivery_failures)
        .map_err(Into::into)
}

fn relay_delivery_failures(
    failures: Vec<DirectRelayFailure>,
) -> Result<Vec<RelayDeliveryFailure>, RuntimeError> {
    failures
        .into_iter()
        .map(|failure| RelayDeliveryFailure::new(failure.relay, failure.reason).map_err(Into::into))
        .collect()
}

fn failed_delivery_failures(
    failed_relays: Vec<DirectRelayFailure>,
    target_relays: &[String],
    reason: &str,
) -> Result<Vec<RelayDeliveryFailure>, RuntimeError> {
    let failures = relay_delivery_failures(failed_relays)?;
    if !failures.is_empty() {
        return Ok(failures);
    }
    target_relays
        .iter()
        .map(|relay| RelayDeliveryFailure::new(relay, reason).map_err(Into::into))
        .collect()
}

fn event_tags(event: &radroots_nostr::prelude::RadrootsNostrEvent) -> Vec<Vec<String>> {
    event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect()
}

fn event_created_at_i64(
    event: &radroots_nostr::prelude::RadrootsNostrEvent,
) -> Result<i64, RuntimeError> {
    i64::try_from(event.created_at.as_secs())
        .map_err(|_| RuntimeError::Config("event timestamp exceeds i64 seconds".to_owned()))
}

fn raw_event_json(
    event: &radroots_nostr::prelude::RadrootsNostrEvent,
) -> Result<Value, RuntimeError> {
    Ok(json!({
        "id": event.id.to_hex(),
        "pubkey": event.pubkey.to_string(),
        "created_at": event_created_at_i64(event)?,
        "kind": u32::from(event.kind.as_u16()),
        "tags": event_tags(event),
        "content": event.content.clone(),
        "sig": event.sig.to_string(),
    }))
}
