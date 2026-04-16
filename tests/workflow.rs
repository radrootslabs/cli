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
    command.env("APPDATA", workdir.join("roaming"));
    command.env("LOCALAPPDATA", workdir.join("local"));
    for key in [
        "RADROOTS_ENV_FILE",
        "RADROOTS_OUTPUT",
        "RADROOTS_CLI_LOGGING_FILTER",
        "RADROOTS_CLI_LOGGING_OUTPUT_DIR",
        "RADROOTS_CLI_LOGGING_STDOUT",
        "RADROOTS_CLI_PATHS_PROFILE",
        "RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT",
        "RADROOTS_LOG_FILTER",
        "RADROOTS_LOG_DIR",
        "RADROOTS_LOG_STDOUT",
        "RADROOTS_ACCOUNT",
        "RADROOTS_ACCOUNT_SECRET_BACKEND",
        "RADROOTS_ACCOUNT_SECRET_FALLBACK",
        "RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE",
        "RADROOTS_IDENTITY_PATH",
        "RADROOTS_SIGNER",
        "RADROOTS_RELAYS",
        "RADROOTS_MYC_EXECUTABLE",
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command.env("RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE", "false");
    command
}

fn write_workspace_config(workdir: &Path, contents: &str) {
    let config_dir = workdir.join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write workspace config");
}

#[test]
fn setup_seller_creates_local_state_and_reports_farm_attention() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["setup", "seller"])
        .output()
        .expect("run setup seller");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Setup saved"));
    assert!(stdout.contains("Ready"));
    assert!(stdout.contains("Selected account"));
    assert!(stdout.contains("Local market data"));
    assert!(stdout.contains("Needs attention"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("Farm draft"));
    assert!(stdout.contains("radroots farm init"));
    assert!(stdout.contains("radroots status"));

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "view"])
        .output()
        .expect("run account view");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    assert_eq!(account_json["state"], "ready");

    let local_output = cli_command_in(dir.path())
        .args(["--json", "local", "status"])
        .output()
        .expect("run local status");
    assert!(local_output.status.success());
    let local_json: Value =
        serde_json::from_slice(local_output.stdout.as_slice()).expect("local json");
    assert_eq!(local_json["state"], "ready");
}

#[test]
fn status_is_unconfigured_before_setup() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["--json", "status"])
        .output()
        .expect("run status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("status json");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["ready"], Value::Array(Vec::new()));
    assert_eq!(
        json["needs_attention"],
        serde_json::json!([
            "Selected account",
            "Local market data",
            "Relay configuration"
        ])
    );
    assert_eq!(
        json["next"],
        serde_json::json!(["radroots setup buyer", "radroots setup seller"])
    );
}

#[test]
fn status_calls_out_missing_relay_after_buyer_setup() {
    let dir = tempdir().expect("tempdir");
    let setup = cli_command_in(dir.path())
        .args(["setup", "buyer"])
        .output()
        .expect("run setup buyer");
    assert!(setup.status.success());

    let output = cli_command_in(dir.path())
        .args(["--json", "status"])
        .output()
        .expect("run status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("status json");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(
        json["ready"],
        serde_json::json!(["Selected account", "Local market data"])
    );
    assert_eq!(
        json["needs_attention"],
        serde_json::json!(["Relay configuration"])
    );
    assert_eq!(
        json["next"],
        serde_json::json!([
            "radroots relay list --relay wss://relay.example.com",
            "radroots status"
        ])
    );
}

#[test]
fn status_reports_farm_publish_need_when_core_state_is_ready() {
    let dir = tempdir().expect("tempdir");

    let account = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account.status.success());

    let local = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(local.status.success());

    write_workspace_config(
        dir.path(),
        "[relay]\nurls = [\"wss://relay.one\"]\npublish_policy = \"any\"\n",
    );

    let farm = cli_command_in(dir.path())
        .args([
            "farm",
            "setup",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
        ])
        .output()
        .expect("run farm setup");
    assert!(farm.status.success());

    let output = cli_command_in(dir.path())
        .args(["status"])
        .output()
        .expect("run status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Status"));
    assert!(stdout.contains("Ready"));
    assert!(stdout.contains("Selected account"));
    assert!(stdout.contains("Local market data"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("Needs attention"));
    assert!(stdout.contains("Farm not yet published"));
    assert!(stdout.contains("radroots farm publish"));
    assert!(stdout.contains("radroots sell add tomatoes"));
}
