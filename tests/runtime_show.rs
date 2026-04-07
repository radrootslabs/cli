use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn appdata_root(workdir: &Path) -> std::path::PathBuf {
    workdir.join("roaming").join("Radroots")
}

fn localappdata_root(workdir: &Path) -> std::path::PathBuf {
    workdir.join("local").join("Radroots")
}

fn interactive_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        localappdata_root(workdir)
    } else {
        workdir.join("home").join(".radroots")
    }
}

fn config_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        appdata_root(workdir).join("config")
    } else {
        interactive_root(workdir).join("config")
    }
}

fn data_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        localappdata_root(workdir).join("data")
    } else {
        interactive_root(workdir).join("data")
    }
}

fn logs_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        localappdata_root(workdir).join("logs")
    } else {
        interactive_root(workdir).join("logs")
    }
}

fn secrets_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        appdata_root(workdir).join("secrets")
    } else {
        interactive_root(workdir).join("secrets")
    }
}

fn runtime_show_command_in(workdir: &Path) -> Command {
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
fn config_show_json_reports_default_bootstrap_state() {
    let dir = tempdir().expect("tempdir");
    let canonical_root = dir.path().canonicalize().expect("canonical tempdir");
    let output = runtime_show_command_in(dir.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["source"], "local runtime state");
    assert_eq!(json["output"]["format"], "json");
    assert_eq!(json["output"]["verbosity"], "normal");
    assert_eq!(json["output"]["color"], true);
    assert_eq!(json["output"]["dry_run"], false);
    assert_eq!(json["paths"]["profile"], "interactive_user");
    assert_eq!(
        json["paths"]["app_config_path"],
        config_root(dir.path())
            .join("apps/cli/config.toml")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["workspace_config_path"],
        canonical_root
            .join(".radroots/config.toml")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["app_data_root"],
        data_root(dir.path()).join("apps/cli").display().to_string()
    );
    assert_eq!(
        json["paths"]["app_logs_root"],
        logs_root(dir.path()).join("apps/cli").display().to_string()
    );
    assert_eq!(
        json["paths"]["shared_accounts_data_root"],
        data_root(dir.path())
            .join("shared/accounts")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["shared_accounts_secrets_root"],
        secrets_root(dir.path())
            .join("shared/accounts")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["default_identity_path"],
        secrets_root(dir.path())
            .join("shared/identities/default.json")
            .display()
            .to_string()
    );
    assert_eq!(json["logging"]["initialized"], true);
    assert_eq!(json["logging"]["stdout"], false);
    assert_eq!(
        json["logging"]["directory"],
        logs_root(dir.path()).join("apps/cli").display().to_string()
    );
    assert_eq!(json["config_files"]["user_present"], false);
    assert_eq!(json["config_files"]["workspace_present"], false);
    assert_eq!(json["account"]["selector"], Value::Null);
    assert_eq!(
        json["account"]["store_path"],
        data_root(dir.path())
            .join("shared/accounts/store.json")
            .display()
            .to_string()
    );
    assert_eq!(
        json["account"]["secrets_dir"],
        secrets_root(dir.path())
            .join("shared/accounts")
            .display()
            .to_string()
    );
    assert_eq!(
        json["account"]["identity_path"],
        secrets_root(dir.path())
            .join("shared/identities/default.json")
            .display()
            .to_string()
    );
    assert_eq!(
        json["account"]["secret_backend"]["configured_primary"],
        "host_vault"
    );
    assert_eq!(
        json["account"]["secret_backend"]["configured_fallback"],
        "encrypted_file"
    );
    assert_eq!(json["account"]["secret_backend"]["state"], "ready");
    assert_eq!(
        json["account"]["secret_backend"]["active_backend"],
        "encrypted_file"
    );
    assert_eq!(json["account"]["secret_backend"]["used_fallback"], true);
    assert_eq!(json["signer"]["mode"], "local");
    assert_eq!(json["relay"]["count"], 0);
    assert_eq!(json["relay"]["publish_policy"], "any");
    assert_eq!(json["relay"]["source"], "defaults · local first");
    assert_eq!(
        json["local"]["root"],
        data_root(dir.path())
            .join("apps/cli/replica")
            .display()
            .to_string()
    );
    assert_eq!(
        json["local"]["replica_db_path"],
        data_root(dir.path())
            .join("apps/cli/replica/replica.sqlite")
            .display()
            .to_string()
    );
    assert_eq!(json["myc"]["executable"], "myc");
    assert_eq!(json["rpc"]["url"], "http://127.0.0.1:7070");
    assert_eq!(json["rpc"]["bridge_auth_configured"], false);
}

#[test]
fn config_show_json_reflects_environment_configuration() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_show_command_in(dir.path())
        .env("RADROOTS_OUTPUT", "json")
        .env("RADROOTS_LOG_FILTER", "debug")
        .env("RADROOTS_LOG_DIR", "logs/runtime")
        .env("RADROOTS_LOG_STDOUT", "false")
        .env("RADROOTS_ACCOUNT", "acct_demo")
        .env("RADROOTS_IDENTITY_PATH", "state/identity.json")
        .env("RADROOTS_SIGNER", "myc")
        .env("RADROOTS_RELAYS", "wss://relay.one,wss://relay.two")
        .env("RADROOTS_MYC_EXECUTABLE", "bin/myc")
        .env("RADROOTS_RPC_URL", "https://rpc.radroots.test/jsonrpc")
        .env("RADROOTS_RPC_BEARER_TOKEN", "secret")
        .args(["config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["output"]["format"], "json");
    assert_eq!(json["logging"]["filter"], "debug");
    assert_eq!(json["logging"]["directory"], "logs/runtime");
    assert_eq!(json["account"]["selector"], "acct_demo");
    assert_eq!(json["account"]["identity_path"], "state/identity.json");
    assert_eq!(
        json["account"]["secret_backend"]["active_backend"],
        "encrypted_file"
    );
    assert_eq!(json["signer"]["mode"], "myc");
    assert_eq!(json["relay"]["count"], 2);
    assert_eq!(json["relay"]["urls"][0], "wss://relay.one");
    assert_eq!(json["relay"]["source"], "environment · local first");
    assert_eq!(json["myc"]["executable"], "bin/myc");
    assert_eq!(json["rpc"]["url"], "https://rpc.radroots.test/jsonrpc");
    assert_eq!(json["rpc"]["bridge_auth_configured"], true);
}

#[test]
fn config_show_json_reflects_global_output_flags() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_show_command_in(dir.path())
        .args([
            "--json",
            "--trace",
            "--dry-run",
            "--no-color",
            "config",
            "show",
        ])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["output"]["format"], "json");
    assert_eq!(json["output"]["verbosity"], "trace");
    assert_eq!(json["output"]["color"], false);
    assert_eq!(json["output"]["dry_run"], true);
}

#[test]
fn config_show_json_reads_logging_from_default_env_file() {
    let temp = tempdir().expect("tempdir");
    let env_path = temp.path().join(".env");
    let logs_dir = temp.path().join("logs").join("radroots-cli");
    fs::write(
        &env_path,
        format!(
            "RADROOTS_CLI_LOGGING_FILTER=debug,radroots_cli=trace\nRADROOTS_CLI_LOGGING_OUTPUT_DIR={}\nRADROOTS_CLI_LOGGING_STDOUT=false\n",
            logs_dir.display()
        ),
    )
    .expect("write env file");

    let output = runtime_show_command_in(temp.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["logging"]["filter"], "debug,radroots_cli=trace");
    assert_eq!(json["logging"]["directory"], logs_dir.display().to_string());
    let current_file = json["logging"]["current_file"]
        .as_str()
        .expect("current log file");
    assert!(current_file.starts_with(logs_dir.display().to_string().as_str()));
    assert!(std::path::Path::new(current_file).exists());
}

#[test]
fn config_show_json_reads_workspace_relay_config() {
    let dir = tempdir().expect("tempdir");
    let config_dir = dir.path().join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(
        config_dir.join("config.toml"),
        "[relay]\nurls = [\"wss://relay.workspace\", \"wss://relay.backup\"]\npublish_policy = \"any\"\n",
    )
    .expect("write workspace config");

    let output = runtime_show_command_in(dir.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["config_files"]["workspace_present"], true);
    assert_eq!(json["relay"]["count"], 2);
    assert_eq!(json["relay"]["urls"][0], "wss://relay.workspace");
    assert_eq!(json["relay"]["urls"][1], "wss://relay.backup");
    assert_eq!(json["relay"]["source"], "workspace config · local first");
}

#[test]
fn config_show_reads_workspace_rpc_config() {
    let dir = tempdir().expect("tempdir");
    let config_dir = dir.path().join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(
        config_dir.join("config.toml"),
        "[rpc]\nurl = \"https://rpc.workspace.test/jsonrpc\"\n",
    )
    .expect("write workspace config");

    let output = runtime_show_command_in(dir.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["rpc"]["url"], "https://rpc.workspace.test/jsonrpc");
    assert_eq!(json["rpc"]["bridge_auth_configured"], false);
}

#[test]
fn config_show_rejects_ndjson_for_singular_output() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_show_command_in(dir.path())
        .args(["--ndjson", "config", "show"])
        .output()
        .expect("run config show");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("`config show` does not support --ndjson"));
}
