use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn local_command_in(workdir: &Path) -> Command {
    let mut command = Command::cargo_bin("radroots").expect("binary");
    command.current_dir(workdir);
    command.env("HOME", workdir.join("home"));
    for key in [
        "RADROOTS_ENV_FILE",
        "RADROOTS_OUTPUT",
        "RADROOTS_CLI_LOGGING_FILTER",
        "RADROOTS_CLI_LOGGING_OUTPUT_DIR",
        "RADROOTS_CLI_LOGGING_STDOUT",
        "RADROOTS_LOG_FILTER",
        "RADROOTS_LOG_DIR",
        "RADROOTS_LOG_STDOUT",
        "RADROOTS_ACCOUNT",
        "RADROOTS_IDENTITY_PATH",
        "RADROOTS_SIGNER",
        "RADROOTS_RELAYS",
        "RADROOTS_MYC_EXECUTABLE",
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn local_init_json_creates_replica_db_and_roots() {
    let dir = tempdir().expect("tempdir");
    let output = local_command_in(dir.path())
        .args(["--json", "local", "init"])
        .output()
        .expect("run local init");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "initialized");
    assert_eq!(json["replica_db"], "ready");

    let replica_db = dir
        .path()
        .join("home")
        .join(".local/share/radroots/replica/replica.sqlite");
    assert!(replica_db.exists());
    assert!(
        dir.path()
            .join("home")
            .join(".local/share/radroots/replica/backups")
            .exists()
    );
    assert!(
        dir.path()
            .join("home")
            .join(".local/share/radroots/replica/exports")
            .exists()
    );
}

#[test]
fn local_status_reports_unconfigured_when_replica_is_missing() {
    let dir = tempdir().expect("tempdir");
    let output = local_command_in(dir.path())
        .args(["--json", "local", "status"])
        .output()
        .expect("run local status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["replica_db"], "missing");
    assert_eq!(json["actions"][0], "radroots local init");
}

#[test]
fn local_status_reports_real_replica_metadata_after_init() {
    let dir = tempdir().expect("tempdir");
    let init = local_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let output = local_command_in(dir.path())
        .args(["--json", "local", "status"])
        .output()
        .expect("run local status");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["counts"]["farms"], 0);
    assert_eq!(json["counts"]["listings"], 0);
    assert_eq!(json["sync"]["expected_count"], 0);
    assert_eq!(json["sync"]["pending_count"], 0);
}

#[test]
fn local_backup_and_export_write_files() {
    let dir = tempdir().expect("tempdir");
    let init = local_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let backup_path = dir.path().join("backup").join("local.radb");
    let backup = local_command_in(dir.path())
        .args([
            "--json",
            "local",
            "backup",
            "--output",
            backup_path.to_str().expect("backup path"),
        ])
        .output()
        .expect("run local backup");
    assert!(backup.status.success());
    let backup_json: Value = serde_json::from_slice(backup.stdout.as_slice()).expect("json");
    assert_eq!(backup_json["state"], "backup created");
    assert!(backup_path.exists());
    assert!(fs::metadata(&backup_path).expect("backup metadata").len() > 0);

    let export_path = dir.path().join("export").join("local.ndjson");
    let export = local_command_in(dir.path())
        .args([
            "--json",
            "local",
            "export",
            "--format",
            "ndjson",
            "--output",
            export_path.to_str().expect("export path"),
        ])
        .output()
        .expect("run local export");
    assert!(export.status.success());
    let export_json: Value = serde_json::from_slice(export.stdout.as_slice()).expect("json");
    assert_eq!(export_json["state"], "exported");
    assert_eq!(export_json["format"], "ndjson");
    let export_raw = fs::read_to_string(&export_path).expect("read export");
    let lines = export_raw.lines().collect::<Vec<_>>();
    assert!(lines.len() >= 3);
    assert!(lines[0].contains("\"kind\":\"local_export_manifest\""));
}
