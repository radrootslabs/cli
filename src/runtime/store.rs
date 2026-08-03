use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_replica_store::export::{ReplicaStoreExportManifestRs, export_manifest};
use radroots_replica_store::migrations;
use radroots_replica_sync::radroots_replica_sync_status;
use radroots_sdk::storage::{IntegrityStatus, Status as StorageStatus};
use radroots_sql_core::SqlxSqliteExecutor;
use radroots_storage::{
    backup::{BackupFormatVersion, BackupId, BackupPlan, BackupSecretPolicy, RestorePlan},
    status::{IntegrityHealth, ShutdownState, StorageBackend, StorageOpenMode, WriterPolicy},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::global::LocalExportFormatArg;
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession, sdk_storage_root};
use crate::runtime::sync::ensure_sync_run_table;
use crate::view::runtime::{
    LocalBackupView, LocalDerivedProjectionStatusView, LocalExportView, LocalInitView,
    LocalReplicaCountsView, LocalReplicaSyncView, LocalRestoreView, LocalStatusView,
    SdkIntegrityView, SdkStorageStatusView,
};

const DERIVED_PROJECTION_SOURCE: &str = "local derived projection cache";
const SDK_CANONICAL_SOURCE: &str = "SDK canonical event store and outbox";
const SDK_CANONICAL_STORE: &str = "sdk";
const SDK_BACKUP_KIND: &str = "sdk_canonical";
const SDK_BACKUP_MANIFEST_FILE: &str = "manifest.json";
const SDK_RUNTIME_FILE: &str = "runtime.sqlite";
const SDK_PRIVATE_FILE: &str = "private.sqlite";

pub fn init(config: &RuntimeConfig) -> Result<LocalInitView, RuntimeError> {
    let existed = config.local.replica_store_path.exists();
    ensure_local_roots(config)?;
    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
    migrations::run_all_up(&executor)?;
    ensure_sync_run_table(&executor)?;
    let manifest = export_manifest(&executor)?;

    Ok(LocalInitView {
        state: if existed {
            "ready".to_owned()
        } else {
            "initialized".to_owned()
        },
        source: DERIVED_PROJECTION_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_store: "ready".to_owned(),
        path: config.local.replica_store_path.display().to_string(),
        replica_store_version: manifest.replica_store_version,
        backup_format_version: manifest.backup_format_version,
    })
}

pub fn init_preflight(config: &RuntimeConfig) -> Result<LocalInitView, RuntimeError> {
    validate_local_roots(config)?;
    if config.local.replica_store_path.exists() {
        let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
        ensure_sync_run_table(&executor)?;
        let manifest = export_manifest(&executor)?;
        return Ok(LocalInitView {
            state: "ready".to_owned(),
            source: DERIVED_PROJECTION_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_store: "ready".to_owned(),
            path: config.local.replica_store_path.display().to_string(),
            replica_store_version: manifest.replica_store_version,
            backup_format_version: manifest.backup_format_version,
        });
    }

    Ok(LocalInitView {
        state: "dry_run".to_owned(),
        source: DERIVED_PROJECTION_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_store: "missing".to_owned(),
        path: config.local.replica_store_path.display().to_string(),
        replica_store_version: String::new(),
        backup_format_version: String::new(),
    })
}

pub fn status(config: &RuntimeConfig) -> Result<LocalStatusView, CliSdkAdapterError> {
    let sdk_root = sdk_storage_root(config);
    let sdk_existed_before_open = sdk_storage_files_exist(sdk_root.as_path());
    let derived_projection = derived_projection_status(config)?;
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().storage_status())?;
    let integrity = session.block_on(session.sdk().storage_integrity())?;
    Ok(sdk_status_view(
        config,
        sdk_root,
        sdk_existed_before_open,
        receipt,
        integrity,
        derived_projection,
    ))
}

fn derived_projection_status(
    config: &RuntimeConfig,
) -> Result<LocalDerivedProjectionStatusView, RuntimeError> {
    if !config.local.replica_store_path.exists() {
        return Ok(LocalDerivedProjectionStatusView {
            state: "unconfigured".to_owned(),
            source: DERIVED_PROJECTION_SOURCE.to_owned(),
            replica_store: "missing".to_owned(),
            path: config.local.replica_store_path.display().to_string(),
            replica_store_version: String::new(),
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
            actions: vec!["radroots store inspect".to_owned()],
        });
    }

    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
    ensure_sync_run_table(&executor)?;
    let manifest = export_manifest(&executor)?;
    let sync = radroots_replica_sync_status(&executor)?;

    Ok(LocalDerivedProjectionStatusView {
        state: "ready".to_owned(),
        source: DERIVED_PROJECTION_SOURCE.to_owned(),
        replica_store: "ready".to_owned(),
        path: config.local.replica_store_path.display().to_string(),
        replica_store_version: manifest.replica_store_version.clone(),
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
    let plan = sdk_backup_plan()?;
    let operation = session
        .block_on(session.sdk().storage_operations()?.begin_backup(plan))
        .map_err(|error| RuntimeError::Config(format!("SDK backup planning failed: {error}")))?;
    Ok(LocalBackupView {
        state: format!("{:?}", operation.stage()).to_lowercase(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        backup_kind: SDK_BACKUP_KIND.to_owned(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        destination: output.display().to_string(),
        file: output.join(SDK_BACKUP_MANIFEST_FILE).display().to_string(),
        event_store_file: None,
        outbox_file: None,
        manifest_file: None,
        size_bytes: 0,
        manifest: json!({
            "backup_id": hex_bytes(operation.plan().backup_id().as_bytes()),
            "format_version": operation.plan().format_version().get(),
            "secret_policy": format!("{:?}", operation.plan().secret_policy()).to_lowercase(),
            "requested_at_unix_ms": operation.plan().requested_at_unix_ms(),
            "revision": operation.revision().get(),
        }),
        reason: Some(
            "backup is durably planned; the host capture worker must complete staged capture, verification, and finalization"
                .to_owned(),
        ),
        actions: vec!["radroots store status".to_owned()],
    })
}

pub fn backup_preflight(
    config: &RuntimeConfig,
    output: &Path,
) -> Result<LocalBackupView, CliSdkAdapterError> {
    ensure_safe_sdk_backup_destination(config, output)?;
    let session = CliSdkSession::connect(config)?;
    let status = session.block_on(session.sdk().storage_status())?;
    let integrity = session.block_on(session.sdk().storage_integrity())?;
    let manifest = sdk_backup_manifest_preview(output, &status, &integrity);
    Ok(LocalBackupView {
        state: "dry_run".to_owned(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        backup_kind: SDK_BACKUP_KIND.to_owned(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        destination: output.display().to_string(),
        file: output.join(SDK_BACKUP_MANIFEST_FILE).display().to_string(),
        event_store_file: Some(output.join(SDK_RUNTIME_FILE).display().to_string()),
        outbox_file: Some(output.join(SDK_RUNTIME_FILE).display().to_string()),
        manifest_file: Some(output.join(SDK_BACKUP_MANIFEST_FILE).display().to_string()),
        size_bytes: 0,
        manifest,
        reason: Some(
            "dry run requested; SDK canonical backup directory was not written".to_owned(),
        ),
        actions: vec!["radroots store backup".to_owned()],
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
    let manifest_path = source.join(SDK_BACKUP_MANIFEST_FILE);
    let manifest = serde_json::from_slice::<radroots_storage::backup::BackupManifest>(&fs::read(
        &manifest_path,
    )?)
    .map_err(|error| RuntimeError::Config(format!("invalid backup manifest: {error}")))?;
    let plan = RestorePlan::new(manifest.clone(), manifest.secret_policy(), unix_ms()?)
        .map_err(|error| RuntimeError::Config(format!("invalid restore plan: {error}")))?;
    let session = CliSdkSession::connect(config)?;
    let operation = session
        .block_on(session.sdk().storage_operations()?.begin_restore(plan))
        .map_err(|error| RuntimeError::Config(format!("SDK restore planning failed: {error}")))?;
    if !dry_run {
        return Err(RuntimeError::Config(
            "restore is planned but requires the host staging and atomic replacement worker"
                .to_owned(),
        )
        .into());
    }
    Ok(LocalRestoreView {
        state: "dry_run".to_owned(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        restore_kind: SDK_BACKUP_KIND.to_owned(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        backup_source: source.display().to_string(),
        destination: destination.display().to_string(),
        event_store_file: source.join(SDK_RUNTIME_FILE).display().to_string(),
        outbox_file: source.join(SDK_RUNTIME_FILE).display().to_string(),
        manifest_file: manifest_path.display().to_string(),
        destination_event_store_file: Some(
            destination.join(SDK_RUNTIME_FILE).display().to_string(),
        ),
        destination_outbox_file: Some(destination.join(SDK_RUNTIME_FILE).display().to_string()),
        restored_event_store_file: None,
        restored_outbox_file: None,
        manifest: json_value(&manifest)?,
        verification: json!({
            "stage": format!("{:?}", operation.stage()).to_lowercase(),
            "revision": operation.revision().get(),
        }),
        overwrite,
        dry_run,
        reason: Some("dry run requested; restore was validated and not staged".to_owned()),
        actions: vec!["radroots store restore <backup-dir>".to_owned()],
    })
}

pub fn export(
    config: &RuntimeConfig,
    format: LocalExportFormatArg,
    output: &Path,
) -> Result<LocalExportView, RuntimeError> {
    if !config.local.replica_store_path.exists() {
        return Ok(LocalExportView {
            state: "unconfigured".to_owned(),
            source: DERIVED_PROJECTION_SOURCE.to_owned(),
            format: format.as_str().to_owned(),
            file: output.display().to_string(),
            records: 0,
            export_version: String::new(),
            schema_hash: String::new(),
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store inspect".to_owned()],
        });
    }

    ensure_safe_output_path(config, output)?;
    create_parent_dir(output)?;

    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
    let manifest = export_manifest(&executor)?;
    let sync = radroots_replica_sync_status(&executor)?;
    let records = match format {
        LocalExportFormatArg::Json => {
            let export = json!({
                "kind": "local_export_manifest_v1",
                "source": DERIVED_PROJECTION_SOURCE,
                "replica_store_version": manifest.replica_store_version,
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
                    "source": DERIVED_PROJECTION_SOURCE,
                    "replica_store_version": manifest.replica_store_version,
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
        source: DERIVED_PROJECTION_SOURCE.to_owned(),
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
    sdk_root.join(SDK_RUNTIME_FILE).exists() && sdk_root.join(SDK_PRIVATE_FILE).exists()
}

fn sdk_status_view(
    config: &RuntimeConfig,
    sdk_root: PathBuf,
    sdk_existed_before_open: bool,
    status: StorageStatus,
    integrity: IntegrityStatus,
    derived_projection: LocalDerivedProjectionStatusView,
) -> LocalStatusView {
    let state = if integrity.health() == IntegrityHealth::Healthy {
        "ready"
    } else {
        "needs_attention"
    };
    LocalStatusView {
        state: state.to_owned(),
        source: SDK_CANONICAL_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        canonical_store: SDK_CANONICAL_STORE.to_owned(),
        sdk_storage: storage_backend_label(status.backend()).to_owned(),
        sdk_root: sdk_root.display().to_string(),
        sdk_existed_before_open,
        storage: SdkStorageStatusView {
            backend: storage_backend_label(status.backend()).to_owned(),
            open_mode: storage_open_mode_label(status.open_mode()).to_owned(),
            writer_policy: writer_policy_label(status.writer_policy()).to_owned(),
            shutdown: shutdown_state_label(status.shutdown()).to_owned(),
            wal_enabled: status.wal_enabled(),
            busy_timeout_ms: status.busy_timeout_ms(),
        },
        integrity: SdkIntegrityView {
            health: integrity_health_label(integrity.health()).to_owned(),
            checked_at_unix_ms: integrity.checked_at_unix_ms(),
            verified_members: integrity.verified_members(),
            failed_members: integrity.failed_members(),
        },
        derived_projection,
        reason: (state != "ready")
            .then(|| "SDK canonical store integrity requires attention".to_owned()),
        actions: if state == "ready" {
            Vec::new()
        } else {
            vec!["radroots store inspect".to_owned()]
        },
    }
}

fn storage_backend_label(value: StorageBackend) -> &'static str {
    match value {
        StorageBackend::Memory => "memory",
        StorageBackend::Sqlite => "sqlite",
    }
}

fn storage_open_mode_label(value: StorageOpenMode) -> &'static str {
    match value {
        StorageOpenMode::ReadOnly => "read_only",
        StorageOpenMode::ReadWriteExisting => "read_write_existing",
        StorageOpenMode::Create => "create",
    }
}

fn writer_policy_label(value: WriterPolicy) -> &'static str {
    match value {
        WriterPolicy::NoWriter => "no_writer",
        WriterPolicy::AdvisoryProcessLock => "advisory_process_lock",
    }
}

fn shutdown_state_label(value: ShutdownState) -> &'static str {
    match value {
        ShutdownState::Open => "open",
        ShutdownState::Closing => "closing",
        ShutdownState::Closed => "closed",
    }
}

fn integrity_health_label(value: IntegrityHealth) -> &'static str {
    match value {
        IntegrityHealth::Healthy => "healthy",
        IntegrityHealth::Degraded => "degraded",
        IntegrityHealth::Corrupt => "corrupt",
        IntegrityHealth::Unknown => "unknown",
    }
}

fn sdk_backup_manifest_preview(
    output: &Path,
    status: &StorageStatus,
    integrity: &IntegrityStatus,
) -> Value {
    json!({
        "manifest_kind": "sdk_canonical_backup_preview",
        "destination": output.display().to_string(),
        "backup_paths": {
            "runtime_path": output.join(SDK_RUNTIME_FILE).display().to_string(),
            "private_path": output.join(SDK_PRIVATE_FILE).display().to_string(),
        },
        "source_status": status,
        "integrity": integrity,
    })
}

fn ensure_safe_sdk_backup_destination(
    config: &RuntimeConfig,
    output: &Path,
) -> Result<(), RuntimeError> {
    let sdk_root = sdk_storage_root(config);
    let sdk_runtime_path = sdk_root.join(SDK_RUNTIME_FILE);
    let sdk_private_path = sdk_root.join(SDK_PRIVATE_FILE);
    let forbidden_paths = [
        sdk_root.as_path(),
        config.local.replica_store_path.as_path(),
        sdk_runtime_path.as_path(),
        sdk_private_path.as_path(),
    ];
    if forbidden_paths.contains(&output) {
        return Err(RuntimeError::Config(format!(
            "backup destination {} would overwrite canonical or derived projection store data",
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
    let sdk_runtime_path = sdk_root.join(SDK_RUNTIME_FILE);
    let sdk_private_path = sdk_root.join(SDK_PRIVATE_FILE);
    let forbidden_paths = [
        config.local.root.as_path(),
        config.local.replica_store_path.as_path(),
        sdk_runtime_path.as_path(),
        sdk_private_path.as_path(),
    ];
    if forbidden_paths.contains(&destination) {
        return Err(RuntimeError::Config(format!(
            "restore destination {} would overwrite canonical runtime roots or store files",
            destination.display()
        )));
    }
    if config.local.replica_store_path.starts_with(destination)
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

fn json_value(value: impl Serialize) -> Result<Value, RuntimeError> {
    serde_json::to_value(value).map_err(RuntimeError::from)
}

fn sdk_backup_plan() -> Result<BackupPlan, RuntimeError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| RuntimeError::Config(format!("failed to generate backup ID: {error}")))?;
    let backup_id = BackupId::new(bytes)
        .map_err(|error| RuntimeError::Config(format!("invalid backup ID: {error}")))?;
    BackupPlan::new(
        backup_id,
        BackupFormatVersion::V1,
        BackupSecretPolicy::ExcludeProtectedStorage,
        unix_ms()?,
    )
    .map_err(|error| RuntimeError::Config(format!("invalid backup plan: {error}")))
}

fn unix_ms() -> Result<u64, RuntimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RuntimeError::Config(format!("system clock error: {error}")))?
        .as_millis()
        .try_into()
        .map_err(|_| RuntimeError::Config("system clock is outside SDK range".to_owned()))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn create_parent_dir(path: &Path) -> Result<(), RuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn ensure_safe_output_path(config: &RuntimeConfig, output: &Path) -> Result<(), RuntimeError> {
    if output == config.local.replica_store_path.as_path() {
        return Err(RuntimeError::Config(format!(
            "output path {} would overwrite the local replica database",
            output.display()
        )));
    }
    Ok(())
}

fn manifest_counts(manifest: &ReplicaStoreExportManifestRs) -> LocalReplicaCountsView {
    LocalReplicaCountsView {
        farms: table_row_count(manifest, "farm"),
        listings: table_row_count(manifest, "trade_product"),
        profiles: table_row_count(manifest, "nostr_profile"),
        relays: table_row_count(manifest, "nostr"),
        event_states: table_row_count(manifest, "nostr_event_state"),
    }
}

fn table_row_count(manifest: &ReplicaStoreExportManifestRs, name: &str) -> u64 {
    manifest
        .table_counts
        .iter()
        .find(|table| table.name == name)
        .map(|table| table.row_count)
        .unwrap_or(0)
}
