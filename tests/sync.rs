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
fn sync_status_reports_unconfigured_when_local_replica_is_missing() {
    let dir = tempdir().expect("tempdir");
    let output = cli_command_in(dir.path())
        .args(["--json", "sync", "status"])
        .output()
        .expect("run sync status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("sync json");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["replica_db"], "missing");
    assert_eq!(json["freshness"]["display"], "never synced");
    assert_eq!(json["actions"][0], "radroots local init");
}

#[test]
fn sync_status_reports_queue_and_relay_setup_need_after_local_init() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let output = cli_command_in(dir.path())
        .args(["--json", "sync", "status"])
        .output()
        .expect("run sync status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("sync json");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["replica_db"], "ready");
    assert_eq!(json["queue"]["pending_count"], 0);
    assert_eq!(json["freshness"]["display"], "never synced");
    assert_eq!(
        json["actions"][0],
        "radroots relay ls --relay wss://relay.example.com"
    );
}

#[test]
fn sync_pull_and_push_are_honestly_narrowed_until_relay_plane_lands() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());
    let config_dir = dir.path().join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(
        config_dir.join("config.toml"),
        "[relay]\nurls = [\"wss://relay.one\"]\npublish_policy = \"any\"\n",
    )
    .expect("write workspace config");

    let pull = cli_command_in(dir.path())
        .args(["--json", "sync", "pull"])
        .output()
        .expect("run sync pull");
    assert_eq!(pull.status.code(), Some(4));
    let pull_json: Value = serde_json::from_slice(pull.stdout.as_slice()).expect("pull json");
    assert_eq!(pull_json["direction"], "pull");
    assert_eq!(pull_json["state"], "unavailable");
    assert_eq!(pull_json["relay_count"], 1);
    assert!(
        pull_json["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("relay ingest"))
    );

    let push = cli_command_in(dir.path())
        .args(["--json", "sync", "push"])
        .output()
        .expect("run sync push");
    assert_eq!(push.status.code(), Some(4));
    let push_json: Value = serde_json::from_slice(push.stdout.as_slice()).expect("push json");
    assert_eq!(push_json["direction"], "push");
    assert_eq!(push_json["state"], "unavailable");
    assert_eq!(push_json["queue"]["pending_count"], 0);
    assert!(
        push_json["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("relay publish"))
    );
}

#[test]
fn sync_watch_ndjson_emits_one_frame_per_poll() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());
    let config_dir = dir.path().join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(
        config_dir.join("config.toml"),
        "[relay]\nurls = [\"wss://relay.one\", \"wss://relay.two\"]\npublish_policy = \"any\"\n",
    )
    .expect("write workspace config");

    let output = cli_command_in(dir.path())
        .args([
            "--ndjson",
            "sync",
            "watch",
            "--frames",
            "2",
            "--interval-ms",
            "1",
        ])
        .output()
        .expect("run sync watch");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"sequence\":1"));
    assert!(lines[0].contains("\"state\":\"ready\""));
    assert!(lines[1].contains("\"sequence\":2"));
    assert!(lines[1].contains("\"relay_count\":2"));
}
