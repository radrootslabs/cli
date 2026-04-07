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
fn account_new_json_creates_identity_file() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("identity.json");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "account",
            "new",
        ])
        .output()
        .expect("run account new");

    assert!(output.status.success());
    assert!(identity_path.exists());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["path"], identity_path.display().to_string());
    assert_eq!(json["created"], true);
    assert!(json["public_identity"]["id"].is_string());
    assert!(json["public_identity"]["public_key_hex"].is_string());
    assert!(json["public_identity"]["public_key_npub"].is_string());
    assert!(json.get("secret_key").is_none());
}

#[test]
fn account_whoami_json_reads_existing_public_identity() {
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
            "account",
            "whoami",
        ])
        .output()
        .expect("run account whoami");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["path"], identity_path.display().to_string());
    assert_eq!(json["state"], "ready");
    assert!(json["public_identity"]["id"].is_string());
    assert!(json["public_identity"]["public_key_hex"].is_string());
    assert!(json["public_identity"]["public_key_npub"].is_string());
    assert!(json.get("secret_key").is_none());
}

#[test]
fn account_whoami_json_reports_unconfigured_without_creating_identity() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("missing-identity.json");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "account",
            "whoami",
        ])
        .output()
        .expect("run account whoami");

    assert_eq!(output.status.code(), Some(3));
    assert!(!identity_path.exists());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["path"], identity_path.display().to_string());
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("local identity file was not found"))
    );
    assert_eq!(json.get("public_identity"), None);
}
