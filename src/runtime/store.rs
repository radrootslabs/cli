use std::fs;
use std::path::{Path, PathBuf};

use radroots_replica_db::export::{ReplicaDbExportManifestRs, export_manifest};
use radroots_replica_db::migrations;
use radroots_replica_sync::radroots_replica_sync_status;
use radroots_sdk::{
    BackupReceipt, BackupRequest, IntegrityReceipt, IntegrityRequest, RadrootsSdk, RestoreReceipt,
    RestoreRequest, SdkBackupState, SdkEventStoreStorageStatus, SdkOutboxStorageStatus,
    SdkRestoreState, SdkSqliteStoreStatus, SdkStorageKind, StorageStatusReceipt,
    StorageStatusRequest,
};
use radroots_sql_core::SqliteExecutor;
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::global::LocalExportFormatArg;
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession, sdk_runtime, sdk_storage_root};
use crate::runtime::sync::ensure_sync_run_table;
use crate::view::runtime::{
    LocalBackupView, LocalExportView, LocalInitView, LocalLegacyReplicaStatusView,
    LocalReplicaCountsView, LocalReplicaSyncView, LocalRestoreView, LocalStatusView,
    SdkEventStoreStatusView, SdkIntegrityView, SdkOutboxStatusView, SdkSqliteStatusView,
};

const LEGACY_REPLICA_SOURCE: &str = "legacy local replica · derived/migration source";
const SDK_CANONICAL_SOURCE: &str = "SDK canonical event store and outbox";
const SDK_CANONICAL_STORE: &str = "sdk";
const SDK_BACKUP_KIND: &str = "sdk_canonical";
const SDK_BACKUP_MANIFEST_FILE: &str = "manifest.json";
const SDK_EVENT_STORE_FILE: &str = "event_store.sqlite";
const SDK_OUTBOX_FILE: &str = "outbox.sqlite";

pub fn init(config: &RuntimeConfig) -> Result<LocalInitView, RuntimeError> {
    let existed = config.local.replica_db_path.exists();
    ensure_local_roots(config)?;
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    ensure_sync_run_table(&executor)?;
    let manifest = export_manifest(&executor)?;

    Ok(LocalInitView {
        state: if existed {
            "ready".to_owned()
        } else {
            "initialized".to_owned()
        },
        source: LEGACY_REPLICA_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "ready".to_owned(),
        path: config.local.replica_db_path.display().to_string(),
        replica_db_version: manifest.replica_db_version,
        backup_format_version: manifest.backup_format_version,
    })
}

pub fn init_preflight(config: &RuntimeConfig) -> Result<LocalInitView, RuntimeError> {
    validate_local_roots(config)?;
    if config.local.replica_db_path.exists() {
        let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
        ensure_sync_run_table(&executor)?;
        let manifest = export_manifest(&executor)?;
        return Ok(LocalInitView {
            state: "ready".to_owned(),
            source: LEGACY_REPLICA_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_db: "ready".to_owned(),
            path: config.local.replica_db_path.display().to_string(),
            replica_db_version: manifest.replica_db_version,
            backup_format_version: manifest.backup_format_version,
        });
    }

    Ok(LocalInitView {
        state: "dry_run".to_owned(),
        source: LEGACY_REPLICA_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "missing".to_owned(),
        path: config.local.replica_db_path.display().to_string(),
        replica_db_version: String::new(),
        backup_format_version: String::new(),
    })
}

pub fn status(config: &RuntimeConfig) -> Result<LocalStatusView, CliSdkAdapterError> {
    let sdk_root = sdk_storage_root(config);
    let sdk_existed_before_open = sdk_storage_files_exist(sdk_root.as_path());
    let legacy_replica = legacy_replica_status(config)?;
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().storage_status(StorageStatusRequest::new()))?;
    let integrity = session.block_on(session.sdk().integrity(IntegrityRequest::new()))?;
    Ok(sdk_status_view(
        config,
        sdk_root,
        sdk_existed_before_open,
        receipt,
        integrity,
        legacy_replica,
    ))
}

fn legacy_replica_status(
    config: &RuntimeConfig,
) -> Result<LocalLegacyReplicaStatusView, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(LocalLegacyReplicaStatusView {
            state: "unconfigured".to_owned(),
            source: LEGACY_REPLICA_SOURCE.to_owned(),
            replica_db: "missing".to_owned(),
            path: config.local.replica_db_path.display().to_string(),
            replica_db_version: String::new(),
            backup_format_version: String::new(),
            schema_hash: String::new(),
            counts: LocalReplicaCountsView {
                farms: 0,
                listings: 0,
                profiles: 0,
                relays: 0,
                event_states: 0,
            },
            sync: LocalReplicaSyncView {
                expected_count: 0,
                pending_count: 0,
            },
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        });
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    ensure_sync_run_table(&executor)?;
    let manifest = export_manifest(&executor)?;
    let sync = radroots_replica_sync_status(&executor)?;

    Ok(LocalLegacyReplicaStatusView {
        state: "ready".to_owned(),
        source: LEGACY_REPLICA_SOURCE.to_owned(),
        replica_db: "ready".to_owned(),
        path: config.local.replica_db_path.display().to_string(),
        replica_db_version: manifest.replica_db_version.clone(),
        backup_format_version: manifest.backup_format_version.clone(),
        schema_hash: manifest.schema_hash.clone(),
        counts: manifest_counts(&manifest),
        sync: LocalReplicaSyncView {
            expected_count: sync.expected_count,
            pending_count: sync.pending_count,
        },
        reason: None,
        actions: Vec::new(),
    })
}

pub fn backup(
    config: &RuntimeConfig,
    output: &Path,
) -> Result<LocalBackupView, CliSdkAdapterError> {
    ensure_safe_sdk_backup_destination(config, output)?;
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().backup(BackupRequest::new(output)))?;
    sdk_backup_view(receipt)
}

pub fn backup_preflight(
    config: &RuntimeConfig,
    output: &Path,
) -> Result<LocalBackupView, CliSdkAdapterError> {
    ensure_safe_sdk_backup_destination(config, output)?;
    let session = CliSdkSession::connect(config)?;
    let status = session.block_on(session.sdk().storage_status(StorageStatusRequest::new()))?;
    let integrity = session.block_on(session.sdk().integrity(IntegrityRequest::new()))?;
    let manifest = sdk_backup_manifest_preview(output, &status, &integrity);
    Ok(LocalBackupView {
        state: "dry_run".to_owned(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        backup_kind: SDK_BACKUP_KIND.to_owned(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        destination: output.display().to_string(),
        file: output.join(SDK_BACKUP_MANIFEST_FILE).display().to_string(),
        event_store_file: Some(output.join(SDK_EVENT_STORE_FILE).display().to_string()),
        outbox_file: Some(output.join(SDK_OUTBOX_FILE).display().to_string()),
        manifest_file: Some(output.join(SDK_BACKUP_MANIFEST_FILE).display().to_string()),
        size_bytes: 0,
        manifest,
        reason: Some(
            "dry run requested; SDK canonical backup directory was not written".to_owned(),
        ),
        actions: vec!["radroots store backup create".to_owned()],
    })
}

pub fn restore(
    config: &RuntimeConfig,
    source: &Path,
    destination: Option<&Path>,
    overwrite: bool,
    dry_run: bool,
) -> Result<LocalRestoreView, CliSdkAdapterError> {
    let destination = destination
        .map(Path::to_path_buf)
        .unwrap_or_else(|| sdk_storage_root(config));
    ensure_safe_sdk_restore_destination(config, &destination)?;
    let request = RestoreRequest::new(source)
        .with_destination(destination)
        .with_overwrite(overwrite)
        .with_dry_run(dry_run);
    let runtime = sdk_runtime()?;
    let receipt = runtime.block_on(RadrootsSdk::restore(request))?;
    sdk_restore_view(receipt, overwrite, dry_run)
}

pub fn export(
    config: &RuntimeConfig,
    format: LocalExportFormatArg,
    output: &Path,
) -> Result<LocalExportView, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(LocalExportView {
            state: "unconfigured".to_owned(),
            source: LEGACY_REPLICA_SOURCE.to_owned(),
            format: format.as_str().to_owned(),
            file: output.display().to_string(),
            records: 0,
            export_version: String::new(),
            schema_hash: String::new(),
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        });
    }

    ensure_safe_output_path(config, output)?;
    create_parent_dir(output)?;

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let manifest = export_manifest(&executor)?;
    let sync = radroots_replica_sync_status(&executor)?;
    let records = match format {
        LocalExportFormatArg::Json => {
            let export = json!({
                "kind": "local_export_manifest_v1",
                "source": LEGACY_REPLICA_SOURCE,
                "replica_db_version": manifest.replica_db_version,
                "backup_format_version": manifest.backup_format_version,
                "export_version": manifest.export_version,
                "schema_hash": manifest.schema_hash,
                "sync": {
                    "expected_count": sync.expected_count,
                    "pending_count": sync.pending_count,
                },
                "table_counts": manifest.table_counts,
            });
            fs::write(output, serde_json::to_string_pretty(&export)?)?;
            1
        }
        LocalExportFormatArg::Ndjson => {
            let mut lines = Vec::new();
            lines.push(
                json!({
                    "kind": "local_export_manifest",
                    "source": LEGACY_REPLICA_SOURCE,
                    "replica_db_version": manifest.replica_db_version,
                    "backup_format_version": manifest.backup_format_version,
                    "export_version": manifest.export_version,
                    "schema_hash": manifest.schema_hash,
                })
                .to_string(),
            );
            lines.push(
                json!({
                    "kind": "local_sync_status",
                    "expected_count": sync.expected_count,
                    "pending_count": sync.pending_count,
                })
                .to_string(),
            );
            for table in &manifest.table_counts {
                lines.push(
                    json!({
                        "kind": "local_table_count",
                        "table": table.name,
                        "row_count": table.row_count,
                    })
                    .to_string(),
                );
            }
            fs::write(output, format!("{}\n", lines.join("\n")))?;
            lines.len()
        }
    };

    Ok(LocalExportView {
        state: "exported".to_owned(),
        source: LEGACY_REPLICA_SOURCE.to_owned(),
        format: format.as_str().to_owned(),
        file: output.display().to_string(),
        records,
        export_version: manifest.export_version,
        schema_hash: manifest.schema_hash,
        reason: None,
        actions: Vec::new(),
    })
}

fn ensure_local_roots(config: &RuntimeConfig) -> Result<(), RuntimeError> {
    fs::create_dir_all(&config.local.root)?;
    fs::create_dir_all(&config.local.backups_dir)?;
    fs::create_dir_all(&config.local.exports_dir)?;
    Ok(())
}

fn validate_local_roots(config: &RuntimeConfig) -> Result<(), RuntimeError> {
    validate_directory_target(&config.local.root)?;
    validate_directory_target(&config.local.backups_dir)?;
    validate_directory_target(&config.local.exports_dir)?;
    Ok(())
}

fn validate_directory_target(path: &Path) -> Result<(), RuntimeError> {
    let mut candidate = path.to_path_buf();
    loop {
        if candidate.exists() {
            if candidate.is_dir() {
                return Ok(());
            }
            return Err(RuntimeError::Config(format!(
                "path {} is not a directory",
                candidate.display()
            )));
        }
        if !candidate.pop() {
            return Err(RuntimeError::Config(format!(
                "path {} has no existing parent directory",
                path.display()
            )));
        }
    }
}

fn sdk_storage_files_exist(sdk_root: &Path) -> bool {
    sdk_root.join(SDK_EVENT_STORE_FILE).exists() && sdk_root.join(SDK_OUTBOX_FILE).exists()
}

fn sdk_status_view(
    config: &RuntimeConfig,
    sdk_root: PathBuf,
    sdk_existed_before_open: bool,
    receipt: StorageStatusReceipt,
    integrity: IntegrityReceipt,
    legacy_replica: LocalLegacyReplicaStatusView,
) -> LocalStatusView {
    let event_store_path = receipt
        .paths
        .as_ref()
        .map(|paths| paths.event_store_path.display().to_string());
    let outbox_path = receipt
        .paths
        .as_ref()
        .map(|paths| paths.outbox_path.display().to_string());
    let state = sdk_status_state(&receipt, &integrity).to_owned();
    let reason = sdk_status_reason(&state);
    let actions = sdk_status_actions(&state);
    LocalStatusView {
        state,
        source: SDK_CANONICAL_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        sdk_storage: sdk_storage_kind_label(receipt.storage).to_owned(),
        sdk_root: sdk_root.display().to_string(),
        sdk_existed_before_open,
        event_store: sdk_event_store_status_view(receipt.event_store, event_store_path),
        outbox: sdk_outbox_status_view(receipt.outbox, outbox_path),
        integrity: sdk_integrity_view(integrity),
        legacy_replica,
        reason,
        actions,
    }
}

fn sdk_status_state(receipt: &StorageStatusReceipt, integrity: &IntegrityReceipt) -> &'static str {
    if receipt.event_store.store.integrity_ok
        && receipt.outbox.store.integrity_ok
        && integrity.event_store_ok
        && integrity.outbox_ok
    {
        "ready"
    } else {
        "needs_attention"
    }
}

fn sdk_status_reason(state: &str) -> Option<String> {
    match state {
        "ready" => None,
        _ => Some("SDK canonical store integrity check failed".to_owned()),
    }
}

fn sdk_status_actions(state: &str) -> Vec<String> {
    match state {
        "ready" => Vec::new(),
        _ => vec!["radroots store status get".to_owned()],
    }
}

fn sdk_event_store_status_view(
    status: SdkEventStoreStorageStatus,
    path: Option<String>,
) -> SdkEventStoreStatusView {
    SdkEventStoreStatusView {
        path,
        store: sdk_sqlite_status_view(status.store),
        total_events: status.total_events,
        projection_eligible_events: status.projection_eligible_events,
        relay_observations: status.relay_observations,
        last_event_seq: status.last_event_seq,
        last_event_updated_at_ms: status.last_event_updated_at_ms,
    }
}

fn sdk_outbox_status_view(
    status: SdkOutboxStorageStatus,
    path: Option<String>,
) -> SdkOutboxStatusView {
    SdkOutboxStatusView {
        path,
        store: sdk_sqlite_status_view(status.store),
        total_events: status.total_events,
        pending_events: status.pending_events,
        retryable_events: status.retryable_events,
        terminal_events: status.terminal_events,
        failed_terminal_events: status.failed_terminal_events,
        ready_signed_events: status.ready_signed_events,
        publishing_events: status.publishing_events,
        last_attempt_at_ms: status.last_attempt_at_ms,
        last_error: status.last_error,
    }
}

fn sdk_sqlite_status_view(status: SdkSqliteStoreStatus) -> SdkSqliteStatusView {
    SdkSqliteStatusView {
        schema_version: status.schema_version,
        journal_mode: status.journal_mode,
        foreign_keys_enabled: status.foreign_keys_enabled,
        busy_timeout_ms: status.busy_timeout_ms,
        integrity_ok: status.integrity_ok,
        integrity_result: status.integrity_result,
    }
}

fn sdk_integrity_view(receipt: IntegrityReceipt) -> SdkIntegrityView {
    SdkIntegrityView {
        checked_paths: receipt
            .checked_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        event_store_ok: receipt.event_store_ok,
        outbox_ok: receipt.outbox_ok,
        event_store_result: receipt.event_store_result,
        outbox_result: receipt.outbox_result,
    }
}

fn ensure_safe_sdk_backup_destination(
    config: &RuntimeConfig,
    output: &Path,
) -> Result<(), RuntimeError> {
    let sdk_root = sdk_storage_root(config);
    let sdk_event_store_path = sdk_root.join(SDK_EVENT_STORE_FILE);
    let sdk_outbox_path = sdk_root.join(SDK_OUTBOX_FILE);
    let forbidden_paths = [
        sdk_root.as_path(),
        config.local.replica_db_path.as_path(),
        sdk_event_store_path.as_path(),
        sdk_outbox_path.as_path(),
    ];
    if forbidden_paths.iter().any(|forbidden| output == *forbidden) {
        return Err(RuntimeError::Config(format!(
            "backup destination {} would overwrite canonical or legacy store data",
            output.display()
        )));
    }
    if output.starts_with(sdk_root.as_path()) {
        return Err(RuntimeError::Config(format!(
            "backup destination {} must not be inside the SDK canonical store directory",
            output.display()
        )));
    }
    Ok(())
}

fn ensure_safe_sdk_restore_destination(
    config: &RuntimeConfig,
    destination: &Path,
) -> Result<(), RuntimeError> {
    let sdk_root = sdk_storage_root(config);
    let sdk_event_store_path = sdk_root.join(SDK_EVENT_STORE_FILE);
    let sdk_outbox_path = sdk_root.join(SDK_OUTBOX_FILE);
    let forbidden_paths = [
        config.local.root.as_path(),
        config.local.replica_db_path.as_path(),
        sdk_event_store_path.as_path(),
        sdk_outbox_path.as_path(),
    ];
    if forbidden_paths
        .iter()
        .any(|forbidden| destination == *forbidden)
    {
        return Err(RuntimeError::Config(format!(
            "restore destination {} would overwrite canonical runtime roots or store files",
            destination.display()
        )));
    }
    if config.local.replica_db_path.starts_with(destination)
        || config.local.backups_dir.starts_with(destination)
        || config.local.exports_dir.starts_with(destination)
    {
        return Err(RuntimeError::Config(format!(
            "restore destination {} must not contain CLI runtime state directories",
            destination.display()
        )));
    }
    Ok(())
}

fn sdk_backup_view(receipt: BackupReceipt) -> Result<LocalBackupView, CliSdkAdapterError> {
    let event_store_file = receipt.event_store_path.as_ref().map(display_path);
    let outbox_file = receipt.outbox_path.as_ref().map(display_path);
    let manifest_file = receipt.manifest_path.as_ref().map(display_path);
    let size_bytes = path_size(receipt.event_store_path.as_ref())?
        + path_size(receipt.outbox_path.as_ref())?
        + path_size(receipt.manifest_path.as_ref())?;
    Ok(LocalBackupView {
        state: sdk_backup_state_label(receipt.state).to_owned(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        backup_kind: SDK_BACKUP_KIND.to_owned(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        destination: display_path(&receipt.destination),
        file: manifest_file
            .clone()
            .unwrap_or_else(|| receipt.destination.display().to_string()),
        event_store_file,
        outbox_file,
        manifest_file,
        size_bytes,
        manifest: json_value(&receipt.manifest)?,
        reason: None,
        actions: Vec::new(),
    })
}

fn sdk_restore_view(
    receipt: RestoreReceipt,
    overwrite: bool,
    dry_run: bool,
) -> Result<LocalRestoreView, CliSdkAdapterError> {
    let destination_paths = receipt.destination_paths.as_ref();
    let restored_paths = receipt.restored_paths.as_ref();
    Ok(LocalRestoreView {
        state: sdk_restore_state_label(receipt.state).to_owned(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        restore_kind: SDK_BACKUP_KIND.to_owned(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        backup_source: display_path(&receipt.source),
        destination: receipt
            .destination
            .as_ref()
            .map(display_path)
            .unwrap_or_default(),
        event_store_file: display_path(&receipt.event_store_path),
        outbox_file: display_path(&receipt.outbox_path),
        manifest_file: display_path(&receipt.manifest_path),
        destination_event_store_file: destination_paths
            .map(|paths| display_path(&paths.event_store_path)),
        destination_outbox_file: destination_paths.map(|paths| display_path(&paths.outbox_path)),
        restored_event_store_file: restored_paths
            .map(|paths| display_path(&paths.event_store_path)),
        restored_outbox_file: restored_paths.map(|paths| display_path(&paths.outbox_path)),
        manifest: json_value(&receipt.manifest)?,
        verification: json_value(&receipt.verification)?,
        overwrite,
        dry_run,
        reason: if dry_run {
            Some("dry run requested; SDK canonical store was not restored".to_owned())
        } else {
            None
        },
        actions: if dry_run {
            vec!["radroots store backup restore <backup-dir>".to_owned()]
        } else {
            Vec::new()
        },
    })
}

fn sdk_restore_state_label(state: SdkRestoreState) -> &'static str {
    match state {
        SdkRestoreState::Validated => "validated",
        SdkRestoreState::DryRun => "dry_run",
        SdkRestoreState::Completed => "completed",
        _ => "unknown",
    }
}

fn sdk_backup_manifest_preview(
    output: &Path,
    status: &StorageStatusReceipt,
    integrity: &IntegrityReceipt,
) -> Value {
    json!({
        "manifest_kind": "sdk_canonical_backup_preview",
        "destination": output.display().to_string(),
        "source_storage": sdk_storage_kind_label(status.storage),
        "source_paths": &status.paths,
        "backup_paths": {
            "event_store_path": output.join(SDK_EVENT_STORE_FILE).display().to_string(),
            "outbox_path": output.join(SDK_OUTBOX_FILE).display().to_string(),
        },
        "source_status": status,
        "backup_verification": {
            "event_store_ok": integrity.event_store_ok,
            "outbox_ok": integrity.outbox_ok,
            "event_store_result": &integrity.event_store_result,
            "outbox_result": &integrity.outbox_result,
        },
    })
}

fn sdk_storage_kind_label(kind: SdkStorageKind) -> &'static str {
    match kind {
        SdkStorageKind::Memory => "memory",
        SdkStorageKind::Directory => "directory",
        _ => "unknown",
    }
}

fn sdk_backup_state_label(state: SdkBackupState) -> &'static str {
    match state {
        SdkBackupState::Planned => "planned",
        SdkBackupState::Completed => "completed",
        _ => "unknown",
    }
}

fn json_value(value: impl Serialize) -> Result<Value, RuntimeError> {
    serde_json::to_value(value).map_err(RuntimeError::from)
}

fn path_size(path: Option<&PathBuf>) -> Result<u64, RuntimeError> {
    path.map(fs::metadata)
        .transpose()?
        .map(|metadata| metadata.len())
        .ok_or_else(|| RuntimeError::Config("SDK backup did not report all file paths".to_owned()))
}

fn display_path(path: &PathBuf) -> String {
    path.display().to_string()
}

fn create_parent_dir(path: &Path) -> Result<(), RuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn ensure_safe_output_path(config: &RuntimeConfig, output: &Path) -> Result<(), RuntimeError> {
    if output == config.local.replica_db_path.as_path() {
        return Err(RuntimeError::Config(format!(
            "output path {} would overwrite the local replica database",
            output.display()
        )));
    }
    Ok(())
}

fn manifest_counts(manifest: &ReplicaDbExportManifestRs) -> LocalReplicaCountsView {
    LocalReplicaCountsView {
        farms: table_row_count(manifest, "farm"),
        listings: table_row_count(manifest, "trade_product"),
        profiles: table_row_count(manifest, "nostr_profile"),
        relays: table_row_count(manifest, "nostr_relay"),
        event_states: table_row_count(manifest, "nostr_event_state"),
    }
}

fn table_row_count(manifest: &ReplicaDbExportManifestRs, name: &str) -> u64 {
    manifest
        .table_counts
        .iter()
        .find(|table| table.name == name)
        .map(|table| table.row_count)
        .unwrap_or(0)
}
