use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn cli_command_in(workdir: &Path) -> Command {
    let mut command = Command::cargo_bin("radroots").expect("binary");
    command.current_dir(workdir);
    for key in [
        "RADROOTS_ENV_FILE",
        "RADROOTS_OUTPUT",
        "RADROOTS_CLI_LOGGING_FILTER",
        "RADROOTS_CLI_LOGGING_OUTPUT_DIR",
        "RADROOTS_CLI_LOGGING_STDOUT",
        "RADROOTS_LOG_FILTER",
        "RADROOTS_LOG_DIR",
        "RADROOTS_LOG_STDOUT",
        "RADROOTS_IDENTITY_PATH",
        "RADROOTS_SIGNER_BACKEND",
        "RADROOTS_MYC_EXECUTABLE",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn signer_status_reports_local_ready_when_identity_exists() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("identity.json");

    let init = cli_command_in(dir.path())
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "account",
            "new",
        ])
        .output()
        .expect("run account new");
    assert!(init.status.success());

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "--signer-backend",
            "local",
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["backend"], "local");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["reason"], Value::Null);
    assert_eq!(json["local"]["availability"], "secret_backed");
    assert_eq!(json["local"]["secret_backed"], true);
}

#[test]
fn signer_status_reports_local_unconfigured_when_identity_is_missing() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("missing-identity.json");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "--signer-backend",
            "local",
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["backend"], "local");
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("local identity file was not found"))
    );
    assert_eq!(json["local"], Value::Null);
}

#[test]
fn signer_status_reports_internal_error_for_invalid_identity_file() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("invalid-identity.json");
    fs::write(&identity_path, "{ not valid json").expect("write invalid identity");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "--signer-backend",
            "local",
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["backend"], "local");
    assert_eq!(json["state"], "error");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("invalid identity JSON"))
    );
    assert_eq!(json["local"], Value::Null);
}
