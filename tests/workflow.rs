use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn data_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        workdir.join("local").join("Radroots").join("data")
    } else {
        workdir.join("home").join(".radroots").join("data")
    }
}

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
    let config_dir = workdir.join("infra/local/runtime/radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write workspace config");
}

#[test]
fn setup_seller_without_account_is_unconfigured_and_does_not_create_account() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

    let output = cli_command_in(dir.path())
        .args(["setup", "seller"])
        .output()
        .expect("run setup seller");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Not ready yet"));
    assert!(stdout.contains("Ready"));
    assert!(stdout.contains("Resolved account"));
    assert!(stdout.contains("Local market data"));
    assert!(stdout.contains("Needs attention"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("Account resolution"));
    assert!(stdout.contains("radroots account create"));
    assert!(stdout.contains("radroots setup seller"));
    assert!(!store_path.exists());

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
fn setup_seller_with_default_account_reports_farm_attention() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

    let account_output = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    assert!(store_path.exists());

    let output = cli_command_in(dir.path())
        .args(["setup", "seller"])
        .output()
        .expect("run setup seller");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Setup saved"));
    assert!(stdout.contains("Ready"));
    assert!(stdout.contains("Resolved account"));
    assert!(stdout.contains("Local market data"));
    assert!(stdout.contains("Needs attention"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("Farm draft"));
    assert!(stdout.contains("Account resolution"));
    assert!(stdout.contains("radroots farm init"));
    assert!(stdout.contains("radroots status"));
}

#[test]
fn status_is_unconfigured_before_account_setup() {
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
            "Resolved account",
            "Local market data",
            "Relay configuration"
        ])
    );
    assert_eq!(json["next"], serde_json::json!(["radroots account create"]));
}

#[test]
fn status_points_to_account_selection_when_accounts_exist_without_default() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

    let account_output = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let mut store_json: Value =
        serde_json::from_slice(fs::read(&store_path).expect("read store").as_slice())
            .expect("parse store");
    store_json["default_account_id"] = Value::Null;
    fs::write(
        &store_path,
        serde_json::to_vec_pretty(&store_json).expect("serialize store"),
    )
    .expect("write store");

    let output = cli_command_in(dir.path())
        .args(["--json", "status"])
        .output()
        .expect("run status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("status json");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["account_resolution"]["source"], "none");
    assert_eq!(
        json["next"],
        serde_json::json!([
            "radroots account list",
            "radroots account select <selector>"
        ])
    );
}

#[test]
fn status_calls_out_missing_relay_after_buyer_setup() {
    let dir = tempdir().expect("tempdir");
    let account = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account.status.success());
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
        serde_json::json!(["Resolved account", "Local market data"])
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
    assert!(stdout.contains("Resolved account"));
    assert!(stdout.contains("Account resolution"));
    assert!(stdout.contains("Local market data"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("Needs attention"));
    assert!(stdout.contains("Farm not yet published"));
    assert!(stdout.contains("radroots farm publish"));
    assert!(stdout.contains("radroots sell add tomatoes"));
}
