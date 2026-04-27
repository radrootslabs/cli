use std::fs;
use std::path::Path;

use radroots_replica_db::backup::export_database_backup_json;
use radroots_replica_db::export::{ReplicaDbExportManifestRs, export_manifest};
use radroots_replica_db::migrations;
use radroots_replica_sync::radroots_replica_sync_status;
use radroots_sql_core::SqliteExecutor;
use serde_json::json;

use crate::domain::runtime::{
    LocalBackupView, LocalExportView, LocalInitView, LocalReplicaCountsView, LocalReplicaSyncView,
    LocalStatusView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime_args::LocalExportFormatArg;

const LOCAL_SOURCE: &str = "local replica · local first";

pub fn init(config: &RuntimeConfig) -> Result<LocalInitView, RuntimeError> {
    let existed = config.local.replica_db_path.exists();
    ensure_local_roots(config)?;
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    migrations::run_all_up(&executor)?;
    let manifest = export_manifest(&executor)?;

    Ok(LocalInitView {
        state: if existed {
            "ready".to_owned()
        } else {
            "initialized".to_owned()
        },
        source: LOCAL_SOURCE.to_owned(),
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
        let manifest = export_manifest(&executor)?;
        return Ok(LocalInitView {
            state: "ready".to_owned(),
            source: LOCAL_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
            replica_db: "ready".to_owned(),
            path: config.local.replica_db_path.display().to_string(),
            replica_db_version: manifest.replica_db_version,
            backup_format_version: manifest.backup_format_version,
        });
    }

    Ok(LocalInitView {
        state: "dry_run".to_owned(),
        source: LOCAL_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
        replica_db: "missing".to_owned(),
        path: config.local.replica_db_path.display().to_string(),
        replica_db_version: String::new(),
        backup_format_version: String::new(),
    })
}

pub fn status(config: &RuntimeConfig) -> Result<LocalStatusView, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(LocalStatusView {
            state: "unconfigured".to_owned(),
            source: LOCAL_SOURCE.to_owned(),
            local_root: config.local.root.display().to_string(),
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
    let manifest = export_manifest(&executor)?;
    let sync = radroots_replica_sync_status(&executor)?;

    Ok(LocalStatusView {
        state: "ready".to_owned(),
        source: LOCAL_SOURCE.to_owned(),
        local_root: config.local.root.display().to_string(),
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

pub fn backup(config: &RuntimeConfig, output: &Path) -> Result<LocalBackupView, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(missing_backup_view(output));
    }

    ensure_safe_output_path(config, output)?;
    create_parent_dir(output)?;

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let backup_json = export_database_backup_json(&executor)?;
    fs::write(output, backup_json)?;
    let file_size = fs::metadata(output)?.len();
    let manifest = export_manifest(&executor)?;

    Ok(LocalBackupView {
        state: "backup created".to_owned(),
        source: LOCAL_SOURCE.to_owned(),
        file: output.display().to_string(),
        size_bytes: file_size,
        backup_format_version: manifest.backup_format_version,
        replica_db_version: manifest.replica_db_version,
        reason: None,
        actions: Vec::new(),
    })
}

pub fn backup_preflight(
    config: &RuntimeConfig,
    output: &Path,
) -> Result<LocalBackupView, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(missing_backup_view(output));
    }

    ensure_safe_output_path(config, output)?;
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let manifest = export_manifest(&executor)?;

    Ok(LocalBackupView {
        state: "dry_run".to_owned(),
        source: LOCAL_SOURCE.to_owned(),
        file: output.display().to_string(),
        size_bytes: 0,
        backup_format_version: manifest.backup_format_version,
        replica_db_version: manifest.replica_db_version,
        reason: Some("dry run requested; backup file was not written".to_owned()),
        actions: vec!["radroots store backup create".to_owned()],
    })
}

pub fn export(
    config: &RuntimeConfig,
    format: LocalExportFormatArg,
    output: &Path,
) -> Result<LocalExportView, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(LocalExportView {
            state: "unconfigured".to_owned(),
            source: LOCAL_SOURCE.to_owned(),
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
                "source": LOCAL_SOURCE,
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
                    "source": LOCAL_SOURCE,
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
        source: LOCAL_SOURCE.to_owned(),
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

fn missing_backup_view(output: &Path) -> LocalBackupView {
    LocalBackupView {
        state: "unconfigured".to_owned(),
        source: LOCAL_SOURCE.to_owned(),
        file: output.display().to_string(),
        size_bytes: 0,
        backup_format_version: String::new(),
        replica_db_version: String::new(),
        reason: Some("local replica database is not initialized".to_owned()),
        actions: vec!["radroots store init".to_owned()],
    }
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
