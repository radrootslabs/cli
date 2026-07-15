use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_runtime_paths::{
    default_shared_runtime_store_database_path_from_shared_accounts_data_root,
    default_shared_runtime_store_root_from_shared_accounts_data_root,
};
use radroots_runtime_store::{
    PublishOutboxStatus, RuntimeStore, RuntimeStoreRecord, RuntimeStoreRecordFamily,
    RuntimeStoreRecordInput, RuntimeStoreRecordStatus, SourceRuntime,
};
use radroots_sql_core::SqlxSqliteExecutor;
use serde_json::Value;

use crate::runtime::RuntimeError;
use crate::runtime::config::{PathsConfig, RuntimeConfig};

static RECORD_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn append_local_work(
    config: &RuntimeConfig,
    subject: &str,
    owner_account_id: Option<String>,
    owner_pubkey: Option<String>,
    farm_id: Option<String>,
    listing_addr: Option<String>,
    payload: Value,
) -> Result<RuntimeStoreRecord, RuntimeError> {
    let timestamp = current_time_ms()?;
    let sequence = RECORD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let input = RuntimeStoreRecordInput {
        record_id: format!("cli:local_work:{subject}:{timestamp}:{sequence}"),
        family: RuntimeStoreRecordFamily::LocalWork,
        status: RuntimeStoreRecordStatus::LocalSaved,
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

pub fn shared_runtime_store_db_path(config: &RuntimeConfig) -> Result<PathBuf, RuntimeError> {
    shared_runtime_store_db_path_from_paths(&config.paths)
}

fn shared_runtime_store_db_path_from_paths(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    default_shared_runtime_store_database_path_from_shared_accounts_data_root(
        &paths.shared_accounts_data_root,
    )
    .map_err(|err| {
        RuntimeError::Config(format!("resolve shared runtime-store database path: {err}"))
    })
}

pub fn list_shared_records_latest(
    config: &RuntimeConfig,
    limit: u32,
) -> Result<Vec<RuntimeStoreRecord>, RuntimeError> {
    let database_path = shared_runtime_store_db_path(config)?;
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let executor = SqlxSqliteExecutor::open(database_path)?;
    let store = RuntimeStore::new(executor);
    Ok(store.list_records_changed_latest(limit)?)
}

pub fn list_shared_records_before(
    config: &RuntimeConfig,
    before_change_seq: i64,
    before_seq: i64,
    limit: u32,
) -> Result<Vec<RuntimeStoreRecord>, RuntimeError> {
    let database_path = shared_runtime_store_db_path(config)?;
    if !database_path.exists() {
        return Ok(Vec::new());
    }
    let executor = SqlxSqliteExecutor::open(database_path)?;
    let store = RuntimeStore::new(executor);
    Ok(store.list_records_changed_before(before_change_seq, before_seq, limit)?)
}

pub fn get_shared_record(
    config: &RuntimeConfig,
    record_id: &str,
) -> Result<Option<RuntimeStoreRecord>, RuntimeError> {
    let database_path = shared_runtime_store_db_path(config)?;
    if !database_path.exists() {
        return Ok(None);
    }
    let executor = SqlxSqliteExecutor::open(database_path)?;
    let store = RuntimeStore::new(executor);
    Ok(store.get_record(record_id)?)
}

fn open_store(config: &RuntimeConfig) -> Result<RuntimeStore<SqlxSqliteExecutor>, RuntimeError> {
    let root = shared_runtime_store_root_from_paths(&config.paths)?;
    fs::create_dir_all(&root)?;
    let executor =
        SqlxSqliteExecutor::open(shared_runtime_store_db_path_from_paths(&config.paths)?)?;
    let store = RuntimeStore::new(executor);
    store.migrate_up()?;
    Ok(store)
}

fn shared_runtime_store_root_from_paths(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    default_shared_runtime_store_root_from_shared_accounts_data_root(
        &paths.shared_accounts_data_root,
    )
    .map_err(|err| RuntimeError::Config(format!("resolve shared runtime-store root: {err}")))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{shared_runtime_store_db_path_from_paths, shared_runtime_store_root_from_paths};
    use crate::runtime::config::PathsConfig;

    #[test]
    fn shared_runtime_store_paths_use_shared_runtime_contract() {
        let paths = paths_config("/repo/infra/local/runtime/radroots/data/shared/accounts");

        assert_eq!(
            shared_runtime_store_root_from_paths(&paths).expect("shared runtime-store root"),
            PathBuf::from("/repo/infra/local/runtime/radroots/data/shared/runtime_store")
        );
        assert_eq!(
            shared_runtime_store_db_path_from_paths(&paths).expect("shared runtime-store database"),
            PathBuf::from(
                "/repo/infra/local/runtime/radroots/data/shared/runtime_store/runtime_store.sqlite"
            )
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
            shared_cache_root: PathBuf::from("/repo/infra/local/runtime/radroots/cache"),
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
