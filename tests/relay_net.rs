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

#[test]
fn relay_ls_json_reports_workspace_configured_relays() {
    let dir = tempdir().expect("tempdir");
    let config_dir = dir.path().join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(
        config_dir.join("config.toml"),
        "[relay]\nurls = [\"wss://relay.one\", \"wss://relay.two\"]\npublish_policy = \"any\"\n",
    )
    .expect("write workspace config");

    let output = cli_command_in(dir.path())
        .args(["--json", "relay", "ls"])
        .output()
        .expect("run relay ls");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("relay json");
    assert_eq!(json["state"], "configured");
    assert_eq!(json["count"], 2);
    assert_eq!(json["publish_policy"], "any");
    assert_eq!(json["source"], "workspace config · local first");
    assert_eq!(json["relays"][0]["url"], "wss://relay.one");
    assert_eq!(json["relays"][0]["read"], true);
    assert_eq!(json["relays"][0]["write"], true);
    assert_eq!(json["relays"][1]["url"], "wss://relay.two");
}

#[test]
fn relay_ls_ndjson_emits_one_object_per_relay() {
    let dir = tempdir().expect("tempdir");
    let output = cli_command_in(dir.path())
        .args([
            "--ndjson",
            "--relay",
            "wss://relay.one",
            "--relay",
            "wss://relay.two",
            "relay",
            "ls",
        ])
        .output()
        .expect("run relay ls");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"url\":\"wss://relay.one\""));
    assert!(lines[1].contains("\"url\":\"wss://relay.two\""));
}

#[test]
fn relay_ls_without_relays_exits_unconfigured() {
    let dir = tempdir().expect("tempdir");
    let output = cli_command_in(dir.path())
        .args(["relay", "ls"])
        .output()
        .expect("run relay ls");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Not ready yet"));
    assert!(stdout.contains("Missing"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("no relays are configured"));
}

#[test]
fn net_status_json_reports_effective_network_configuration() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(init.status.success());
    let init_json: Value = serde_json::from_slice(init.stdout.as_slice()).expect("account json");
    let account_id = init_json["account"]["id"]
        .as_str()
        .expect("account id")
        .to_owned();

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "local",
            "--relay",
            "wss://relay.one",
            "--relay",
            "wss://relay.two",
            "net",
            "status",
        ])
        .output()
        .expect("run net status");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("net json");
    assert_eq!(json["state"], "configured");
    assert_eq!(json["session"], "not_started");
    assert_eq!(json["relay_count"], 2);
    assert_eq!(json["publish_policy"], "any");
    assert_eq!(json["signer_mode"], "local");
    assert_eq!(json["account_resolution"]["source"], "default_account");
    assert_eq!(
        json["account_resolution"]["resolved_account"]["id"],
        account_id
    );
    assert_eq!(json["source"], "cli flags · local first");
}
