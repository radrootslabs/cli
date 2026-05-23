use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_local_events::{
    LocalEventRecord, LocalEventRecordInput, LocalEventsStore, LocalRecordFamily,
    LocalRecordStatus, PublishOutboxStatus, SourceRuntime,
};
use radroots_sql_core::SqliteExecutor;
use serde_json::Value;

use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

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
