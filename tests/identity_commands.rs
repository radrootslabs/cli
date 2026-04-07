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
fn account_new_json_creates_local_account_store_entry() {
    let dir = tempdir().expect("tempdir");
    let store_path = dir
        .path()
        .join("home/.local/share/radroots/accounts/store.json");

    let output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");

    assert!(output.status.success());
    assert!(store_path.exists());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "created");
    assert_eq!(json["source"], "local account store · local first");
    assert!(json["account"]["id"].is_string());
    assert_eq!(json["account"]["signer"], "local");
    assert_eq!(json["account"]["is_default"], true);
    assert!(json["public_identity"]["id"].is_string());
    assert!(json["public_identity"]["public_key_hex"].is_string());
    assert!(json["public_identity"]["public_key_npub"].is_string());
}

#[test]
fn account_new_rejects_dry_run_without_creating_store_state() {
    let dir = tempdir().expect("tempdir");
    let store_path = dir
        .path()
        .join("home/.local/share/radroots/accounts/store.json");

    let output = cli_command_in(dir.path())
        .args(["--dry-run", "account", "new"])
        .output()
        .expect("run account new");

    assert_eq!(output.status.code(), Some(2));
    assert!(!store_path.exists());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("`account new` does not support --dry-run yet"));
}

#[test]
fn account_whoami_json_reads_selected_account() {
    let dir = tempdir().expect("tempdir");

    let init = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(init.status.success());

    let output = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["source"], "local account store · local first");
    assert!(json["account"]["id"].is_string());
    assert_eq!(json["account"]["signer"], "local");
    assert_eq!(json["account"]["is_default"], true);
    assert!(json["public_identity"]["id"].is_string());
}

#[test]
fn account_whoami_json_reports_unconfigured_without_accounts() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");

    assert_eq!(output.status.code(), Some(3));

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["account"], Value::Null);
    assert_eq!(json["public_identity"], Value::Null);
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("no local account is selected"))
    );
}

#[test]
fn account_ls_ndjson_emits_one_line_per_account() {
    let dir = tempdir().expect("tempdir");

    let first = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run first account new");
    assert!(first.status.success());

    let second = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run second account new");
    assert!(second.status.success());

    let output = cli_command_in(dir.path())
        .args(["--ndjson", "account", "ls"])
        .output()
        .expect("run account ls");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"id\":"));
    assert!(lines[1].contains("\"id\":"));
}

#[test]
fn account_use_selects_existing_account() {
    let dir = tempdir().expect("tempdir");

    let first = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run first account new");
    assert!(first.status.success());
    let first_json: Value =
        serde_json::from_slice(first.stdout.as_slice()).expect("first account json");
    let first_id = first_json["account"]["id"]
        .as_str()
        .expect("first account id")
        .to_owned();

    let second = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run second account new");
    assert!(second.status.success());

    let use_output = cli_command_in(dir.path())
        .args(["--json", "account", "use", first_id.as_str()])
        .output()
        .expect("run account use");

    assert!(use_output.status.success());
    let use_json: Value =
        serde_json::from_slice(use_output.stdout.as_slice()).expect("account use json");
    assert_eq!(use_json["state"], "active");
    assert_eq!(use_json["active_account_id"], first_id);
    assert_eq!(use_json["account"]["is_default"], true);

    let whoami = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");
    assert!(whoami.status.success());
    let whoami_json: Value =
        serde_json::from_slice(whoami.stdout.as_slice()).expect("account whoami json");
    assert_eq!(whoami_json["account"]["id"], first_id);
    assert_eq!(whoami_json["account"]["is_default"], true);
}
