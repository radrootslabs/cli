use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn doctor_command_in(workdir: &Path) -> Command {
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
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn doctor_reports_unconfigured_local_bootstrap_state() {
    let dir = tempdir().expect("tempdir");
    let output = doctor_command_in(dir.path())
        .args(["--json", "doctor"])
        .output()
        .expect("run doctor");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["ok"], false);
    assert_eq!(json["state"], "warn");
    assert_eq!(json["checks"][0]["name"], "config");
    assert_eq!(json["checks"][0]["status"], "ok");
    assert_eq!(json["checks"][1]["name"], "account");
    assert_eq!(json["checks"][1]["status"], "warn");
    assert_eq!(json["checks"][2]["name"], "relays");
    assert_eq!(json["checks"][2]["status"], "warn");
    assert_eq!(json["checks"][3]["name"], "signer");
    assert_eq!(json["checks"][3]["status"], "warn");
    assert_eq!(json["source"], "local diagnostics");
    assert_eq!(json["actions"][0], "radroots account new");
    assert_eq!(json["actions"][1], "radroots relay ls");
}

#[test]
fn doctor_reports_ready_local_bootstrap_state() {
    let dir = tempdir().expect("tempdir");
    let init = doctor_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(init.status.success());

    let output = doctor_command_in(dir.path())
        .args(["--json", "--relay", "wss://relay.one", "doctor"])
        .output()
        .expect("run doctor");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["ok"], true);
    assert_eq!(json["state"], "ok");
    assert_eq!(json["checks"][1]["name"], "account");
    assert_eq!(json["checks"][1]["status"], "ok");
    assert_eq!(json["checks"][2]["name"], "relays");
    assert_eq!(json["checks"][2]["status"], "ok");
    assert_eq!(json["checks"][3]["name"], "signer");
    assert_eq!(json["checks"][3]["status"], "ok");
    assert_eq!(json["actions"], Value::Null);
}

#[test]
fn doctor_reports_external_failure_for_missing_myc() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join(".env"),
        "RADROOTS_SIGNER=myc\nRADROOTS_RELAYS=wss://relay.one\n",
    )
    .expect("write env file");

    let output = doctor_command_in(dir.path())
        .args(["--json", "--myc-executable", "missing-myc", "doctor"])
        .output()
        .expect("run doctor");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "fail");
    assert_eq!(json["checks"][2]["name"], "relays");
    assert_eq!(json["checks"][2]["status"], "ok");
    assert_eq!(json["checks"][3]["name"], "signer");
    assert_eq!(json["checks"][3]["status"], "fail");
    assert_eq!(json["checks"][4]["name"], "myc");
    assert_eq!(json["checks"][4]["status"], "fail");
    assert_eq!(json["source"], "local diagnostics + myc status command");
}
