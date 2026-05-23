use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_local_events::{
    LocalEventRecord, LocalEventRecordInput, LocalEventsStore, LocalRecordFamily,
    LocalRecordStatus, PublishOutboxStatus, SourceRuntime,
};
use radroots_sql_core::SqliteExecutor;
use serde_json::{Value, json};

use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::direct_relay::{DirectRelayFailure, DirectRelayPublishError};

const SHARED_LOCAL_EVENTS_DIR: &str = "local_events";
const SHARED_LOCAL_EVENTS_DB_FILE: &str = "local_events.sqlite";

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
    let relay_set = relay_set_fingerprint(&config.relay.urls);
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
        relay_delivery_json: Some(pending_delivery_json(&config.relay.urls)),
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
    update_signed_event_outbox(
        config,
        record_id,
        LocalRecordStatus::Published,
        PublishOutboxStatus::Acknowledged,
        json!({
            "state": "acknowledged",
            "target_relays": target_relays,
            "connected_relays": connected_relays,
            "acknowledged_relays": acknowledged_relays,
            "failed_relays": relay_failures_json(failed_relays),
        }),
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
    update_signed_event_outbox(
        config,
        record_id,
        LocalRecordStatus::Failed,
        PublishOutboxStatus::Failed,
        json!({
            "state": "failed",
            "reason": reason,
            "target_relays": target_relays,
            "connected_relays": connected_relays,
            "acknowledged_relays": [],
            "failed_relays": relay_failures_json(failed_relays),
        }),
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
    Ok(shared_local_events_root(config)?.join(SHARED_LOCAL_EVENTS_DB_FILE))
}

pub fn list_shared_records(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<LocalEventRecord>, RuntimeError> {
    let database_path = shared_local_events_db_path(config)?;
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let executor = SqliteExecutor::open(database_path)?;
    let store = LocalEventsStore::new(executor);
    Ok(store.list_records_after(0, limit)?)
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
    relay_delivery_json: Value,
) -> Result<LocalEventRecord, RuntimeError> {
    let store = open_store(config)?;
    Ok(
        store.update_outbox(&radroots_local_events::LocalEventRecordUpdate {
            record_id: record_id.to_owned(),
            status,
            outbox_status,
            relay_set_fingerprint: relay_set_fingerprint(&config.relay.urls),
            relay_delivery_json: Some(relay_delivery_json),
            updated_at_ms: current_time_ms()?,
        })?,
    )
}

fn open_store(config: &RuntimeConfig) -> Result<LocalEventsStore<SqliteExecutor>, RuntimeError> {
    let root = shared_local_events_root(config)?;
    fs::create_dir_all(&root)?;
    let executor = SqliteExecutor::open(root.join(SHARED_LOCAL_EVENTS_DB_FILE))?;
    let store = LocalEventsStore::new(executor);
    store.migrate_up()?;
    Ok(store)
}

fn shared_local_events_root(config: &RuntimeConfig) -> Result<std::path::PathBuf, RuntimeError> {
    let Some(shared_data_root) = config.paths.shared_accounts_data_root.parent() else {
        return Err(RuntimeError::Config(format!(
            "shared accounts data root {} has no parent directory",
            config.paths.shared_accounts_data_root.display()
        )));
    };
    Ok(shared_data_root.join(SHARED_LOCAL_EVENTS_DIR))
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

fn relay_set_fingerprint(relay_urls: &[String]) -> Option<String> {
    if relay_urls.is_empty() {
        return None;
    }
    let mut relays = relay_urls
        .iter()
        .map(|relay| relay.trim())
        .filter(|relay| !relay.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    relays.sort();
    relays.dedup();
    (!relays.is_empty()).then(|| format!("nostr-relay-set-v1:{}", relays.join(",")))
}

fn pending_delivery_json(relay_urls: &[String]) -> Value {
    json!({
        "state": "pending",
        "target_relays": relay_urls,
        "connected_relays": [],
        "acknowledged_relays": [],
        "failed_relays": [],
    })
}

fn relay_failures_json(failures: Vec<DirectRelayFailure>) -> Value {
    Value::Array(
        failures
            .into_iter()
            .map(|failure| {
                json!({
                    "relay": failure.relay,
                    "reason": failure.reason,
                })
            })
            .collect(),
    )
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
