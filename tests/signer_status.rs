use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn cli_command_in(workdir: &Path) -> Command {
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
        "RADROOTS_MYC_EXECUTABLE",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn signer_status_reports_local_ready_when_account_exists() {
    let dir = tempdir().expect("tempdir");

    let init = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(init.status.success());

    let output = cli_command_in(dir.path())
        .args(["--json", "--signer", "local", "signer", "status"])
        .output()
        .expect("run signer status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["mode"], "local");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["source"], "local account store · local first");
    assert_eq!(json["account_id"], json["local"]["account_id"]);
    assert_eq!(json["reason"], Value::Null);
    assert_eq!(json["local"]["availability"], "secret_backed");
    assert_eq!(json["local"]["secret_backed"], true);
}

#[test]
fn signer_status_reports_local_unconfigured_when_no_account_is_selected() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["--json", "--signer", "local", "signer", "status"])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["mode"], "local");
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("no local account is selected"))
    );
    assert_eq!(json["local"], Value::Null);
}

#[test]
fn signer_status_reports_internal_error_for_invalid_account_store_file() {
    let dir = tempdir().expect("tempdir");
    let accounts_dir = dir.path().join("home/.local/share/radroots/accounts");
    fs::create_dir_all(&accounts_dir).expect("create accounts dir");
    fs::write(accounts_dir.join("store.json"), "{ not valid json").expect("write invalid store");

    let output = cli_command_in(dir.path())
        .args(["--json", "--signer", "local", "signer", "status"])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["mode"], "local");
    assert_eq!(json["state"], "error");
    assert!(json["reason"].as_str().is_some());
    assert_eq!(json["local"], Value::Null);
}

#[test]
fn signer_status_honors_explicit_account_selector_over_default_account() {
    let dir = tempdir().expect("tempdir");

    let first = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run first account new");
    assert!(first.status.success());
    let first_json: Value = serde_json::from_slice(first.stdout.as_slice()).expect("first json");
    let first_id = first_json["account"]["id"]
        .as_str()
        .expect("first account id")
        .to_owned();

    let second = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run second account new");
    assert!(second.status.success());

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "local",
            "--account",
            first_id.as_str(),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("signer json");
    assert_eq!(json["mode"], "local");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["account_id"], first_id);
    assert_eq!(json["local"]["account_id"], first_id);
}
