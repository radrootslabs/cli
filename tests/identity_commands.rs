use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use radroots_identity::RadrootsIdentity;
use serde_json::Value;
use tempfile::tempdir;

fn data_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        workdir.join("local").join("Radroots").join("data")
    } else {
        workdir.join("home").join(".radroots").join("data")
    }
}

fn secrets_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        workdir.join("roaming").join("Radroots").join("secrets")
    } else {
        workdir.join("home").join(".radroots").join("secrets")
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

#[test]
fn account_new_json_creates_local_account_store_entry() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

    let output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");

    assert!(output.status.success());
    assert!(store_path.exists());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "created");
    assert_eq!(json["source"], "shared account store · local first");
    assert!(json["account"]["id"].is_string());
    assert_eq!(json["account"]["signer"], "local");
    assert_eq!(json["account"]["is_default"], true);
    assert!(json["public_identity"]["id"].is_string());
    assert!(json["public_identity"]["public_key_hex"].is_string());
    assert!(json["public_identity"]["public_key_npub"].is_string());
}

#[test]
fn account_create_quiet_reports_created_account_id() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["--quiet", "account", "create"])
        .output()
        .expect("run quiet account create");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let line = stdout.trim();
    assert!(line.starts_with("Account created: "));
    assert!(line.len() > "Account created: ".len());
}

#[test]
fn account_new_encrypts_file_backed_secret_fallback_by_default() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    let account_id = json["account"]["id"].as_str().expect("account id");
    let secrets_dir = secrets_root(dir.path()).join("shared/accounts");
    let envelope_path = secrets_dir.join(format!("{account_id}.secret.json"));

    assert!(secrets_dir.join(".vault.key").exists());
    assert!(envelope_path.exists());
    assert!(!secrets_dir.join(format!("{account_id}.secret")).exists());

    let envelope: Value = serde_json::from_slice(
        fs::read(envelope_path)
            .expect("read encrypted envelope")
            .as_slice(),
    )
    .expect("envelope json");
    assert_eq!(envelope["header"]["cipher"], "x_cha_cha20_poly1305");
    assert_eq!(envelope["header"]["key_source"], "secret_vault_wrapped");
    assert!(envelope["ciphertext"].is_array());
}

#[test]
fn account_import_json_creates_watch_only_account_without_secret_material() {
    let dir = tempdir().expect("tempdir");
    let identity = RadrootsIdentity::generate();
    let import_path = dir.path().join("watch-only-identity.json");
    fs::write(
        &import_path,
        serde_json::to_vec_pretty(&identity.to_public()).expect("serialize public identity"),
    )
    .expect("write import file");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "account",
            "import",
            import_path.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run account import");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    let account_id = json["account"]["id"].as_str().expect("account id");
    assert_eq!(json["state"], "imported");
    assert_eq!(json["source"], "shared account store · watch-only import");
    assert_eq!(json["account"]["signer"], "watch_only");
    assert_eq!(json["account"]["is_default"], true);

    let secrets_dir = secrets_root(dir.path()).join("shared/accounts");
    assert!(!secrets_dir.join(format!("{account_id}.secret")).exists());
    assert!(!secrets_dir.join(format!("{account_id}.secret.json")).exists());

    let whoami = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");
    assert!(whoami.status.success());
    let whoami_json: Value =
        serde_json::from_slice(whoami.stdout.as_slice()).expect("whoami json");
    assert_eq!(
        whoami_json["account_resolution"]["resolved_account"]["id"],
        account_id
    );
    assert_eq!(
        whoami_json["account_resolution"]["resolved_account"]["signer"],
        "watch_only"
    );
}

#[test]
fn account_new_rejects_dry_run_without_creating_store_state() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

    let output = cli_command_in(dir.path())
        .args(["--dry-run", "account", "new"])
        .output()
        .expect("run account new");

    assert_eq!(output.status.code(), Some(2));
    assert!(!store_path.exists());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("`account create` does not support --dry-run yet"));
}

#[test]
fn account_new_rejects_plaintext_fallback_backend() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .env("RADROOTS_ACCOUNT_SECRET_BACKEND", "host_vault")
        .env("RADROOTS_ACCOUNT_SECRET_FALLBACK", "plaintext_file")
        .args(["account", "new"])
        .output()
        .expect("run account new");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("must be `host_vault`, `encrypted_file`, `memory`, or `none` for fallback")
    );
}

#[test]
fn account_whoami_json_reads_default_account() {
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
    assert_eq!(json["source"], "shared account store · local first");
    assert_eq!(json["account_resolution"]["source"], "default_account");
    assert!(json["account_resolution"]["resolved_account"]["id"].is_string());
    assert_eq!(
        json["account_resolution"]["resolved_account"]["signer"],
        "local"
    );
    assert_eq!(
        json["account_resolution"]["resolved_account"]["is_default"],
        true
    );
    assert_eq!(
        json["account_resolution"]["default_account"]["id"],
        json["account_resolution"]["resolved_account"]["id"]
    );
    assert!(json["public_identity"]["id"].is_string());
}

#[test]
fn account_new_does_not_replace_existing_default_account() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

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
    assert_eq!(first_json["account"]["is_default"], true);

    let second = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run second account new");
    assert!(second.status.success());
    let second_json: Value = serde_json::from_slice(second.stdout.as_slice()).expect("second json");
    assert_eq!(second_json["account"]["is_default"], false);

    let store_json: Value =
        serde_json::from_slice(fs::read(&store_path).expect("read store").as_slice())
            .expect("parse store");
    assert_eq!(store_json["default_account_id"], first_id);

    let whoami = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");
    assert!(whoami.status.success());
    let whoami_json: Value = serde_json::from_slice(whoami.stdout.as_slice()).expect("whoami json");
    assert_eq!(
        whoami_json["account_resolution"]["resolved_account"]["id"],
        first_id
    );
}

#[test]
fn account_clear_default_json_clears_stored_default_without_removing_accounts() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

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
        .args(["--json", "account", "clear-default"])
        .output()
        .expect("run clear-default");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("clear-default json");
    assert_eq!(json["state"], "cleared");
    assert_eq!(json["cleared_account"]["id"], first_id);
    assert_eq!(json["remaining_account_count"], 2);

    let store_json: Value =
        serde_json::from_slice(fs::read(&store_path).expect("read store").as_slice())
            .expect("parse store");
    assert_eq!(store_json["default_account_id"], Value::Null);
    assert_eq!(
        store_json["accounts"]
            .as_array()
            .expect("accounts array")
            .len(),
        2
    );

    let whoami = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");
    assert_eq!(whoami.status.code(), Some(3));
    let whoami_json: Value =
        serde_json::from_slice(whoami.stdout.as_slice()).expect("whoami json");
    assert_eq!(whoami_json["account_resolution"]["source"], "none");
    assert_eq!(whoami_json["account_resolution"]["default_account"], Value::Null);
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
    assert_eq!(json["account_resolution"]["source"], "none");
    assert_eq!(json["account_resolution"]["resolved_account"], Value::Null);
    assert_eq!(json["account_resolution"]["default_account"], Value::Null);
    assert_eq!(json["public_identity"], Value::Null);
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("no local accounts found"))
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
    assert_eq!(use_json["state"], "default");
    assert_eq!(use_json["default_account_id"], first_id);
    assert_eq!(use_json["account"]["is_default"], true);

    let whoami = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");
    assert!(whoami.status.success());
    let whoami_json: Value =
        serde_json::from_slice(whoami.stdout.as_slice()).expect("account whoami json");
    assert_eq!(
        whoami_json["account_resolution"]["source"],
        "default_account"
    );
    assert_eq!(
        whoami_json["account_resolution"]["resolved_account"]["id"],
        first_id
    );
    assert_eq!(
        whoami_json["account_resolution"]["resolved_account"]["is_default"],
        true
    );
}

#[test]
fn account_remove_json_clears_default_when_removing_default_account() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

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
        .args(["--json", "account", "remove", first_id.as_str()])
        .output()
        .expect("run account remove");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("remove json");
    assert_eq!(json["state"], "removed");
    assert_eq!(json["removed_account"]["id"], first_id);
    assert_eq!(json["default_cleared"], true);
    assert_eq!(json["remaining_account_count"], 1);

    let store_json: Value =
        serde_json::from_slice(fs::read(&store_path).expect("read store").as_slice())
            .expect("parse store");
    assert_eq!(store_json["default_account_id"], Value::Null);
    assert_eq!(
        store_json["accounts"]
            .as_array()
            .expect("accounts array")
            .len(),
        1
    );

    let whoami = cli_command_in(dir.path())
        .args(["--json", "account", "whoami"])
        .output()
        .expect("run account whoami");
    assert_eq!(whoami.status.code(), Some(3));
    let whoami_json: Value =
        serde_json::from_slice(whoami.stdout.as_slice()).expect("whoami json");
    assert_eq!(whoami_json["account_resolution"]["source"], "none");
    assert_eq!(whoami_json["account_resolution"]["default_account"], Value::Null);
}

#[test]
fn account_use_rejects_ambiguous_label_selector() {
    let dir = tempdir().expect("tempdir");
    let store_path = data_root(dir.path()).join("shared/accounts/store.json");

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

    let mut store_json: Value =
        serde_json::from_slice(fs::read(&store_path).expect("read store").as_slice())
            .expect("parse store");
    let accounts = store_json["accounts"]
        .as_array_mut()
        .expect("accounts array");
    accounts[0]["label"] = Value::from("shared");
    accounts[1]["label"] = Value::from("shared");
    fs::write(
        &store_path,
        serde_json::to_vec_pretty(&store_json).expect("serialize store"),
    )
    .expect("write store");

    let output = cli_command_in(dir.path())
        .args(["account", "use", "shared"])
        .output()
        .expect("run account use");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("matched multiple local accounts"));
}
