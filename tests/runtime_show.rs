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

fn runtime_manager_registry_path(workdir: &Path) -> std::path::PathBuf {
    config_root(workdir).join("shared/runtime-manager/instances.toml")
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

fn binding_by_capability<'a>(json: &'a Value, capability_id: &str) -> &'a Value {
    json["capability_bindings"]
        .as_array()
        .expect("capability bindings array")
        .iter()
        .find(|binding| binding["capability_id"] == capability_id)
        .expect("binding present")
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
    assert_eq!(json["interaction"]["input_enabled"], true);
    assert_eq!(json["interaction"]["assume_yes"], false);
    assert_eq!(json["paths"]["profile"], "interactive_user");
    assert_eq!(json["paths"]["profile_source"], "default");
    assert_eq!(json["paths"]["root_source"], "host_defaults");
    assert_eq!(json["paths"]["repo_local_root"], Value::Null);
    assert_eq!(json["paths"]["repo_local_root_source"], Value::Null);
    assert_eq!(
        json["paths"]["subordinate_path_override_source"],
        "runtime_config"
    );
    assert_eq!(json["paths"]["allowed_profiles"][0], "interactive_user");
    assert_eq!(json["paths"]["allowed_profiles"][1], "repo_local");
    assert_eq!(json["paths"]["app_namespace"], "apps/cli");
    assert_eq!(
        json["paths"]["shared_accounts_namespace"],
        "shared/accounts"
    );
    assert_eq!(
        json["paths"]["shared_identities_namespace"],
        "shared/identities"
    );
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
            .join("infra/local/runtime/radroots/config.toml")
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
    assert_eq!(
        json["migration"]["posture"],
        "explicit_operator_import_required"
    );
    assert_eq!(json["migration"]["state"], "ready");
    assert_eq!(json["migration"]["silent_startup_relocation"], false);
    assert_eq!(
        json["migration"]["compatibility_window"],
        "detect_and_report_only"
    );
    assert_eq!(
        json["migration"]["detected_legacy_paths"],
        Value::Array(vec![])
    );
    assert_eq!(json["migration"]["actions"], Value::Array(vec![]));
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
        json["account"]["secret_backend"]["contract_default_backend"],
        "host_vault"
    );
    assert_eq!(
        json["account"]["secret_backend"]["contract_default_fallback"],
        "encrypted_file"
    );
    assert_eq!(
        json["account"]["secret_backend"]["allowed_backends"][0],
        "host_vault"
    );
    assert_eq!(
        json["account"]["secret_backend"]["allowed_backends"][1],
        "encrypted_file"
    );
    assert_eq!(
        json["account"]["secret_backend"]["host_vault_policy"],
        "desktop"
    );
    assert_eq!(
        json["account"]["secret_backend"]["uses_protected_store"],
        true
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
    assert_eq!(json["write_plane"]["provider_runtime_id"], "radrootsd");
    assert_eq!(
        json["write_plane"]["binding_model"],
        "daemon_backed_jsonrpc"
    );
    assert_eq!(json["write_plane"]["state"], "unconfigured");
    assert_eq!(json["write_plane"]["provenance"], "unavailable");
    assert_eq!(
        json["write_plane"]["source"],
        "no explicit capability binding or managed preferred default"
    );
    assert!(json["write_plane"]["target"].is_null());
    assert_eq!(json["write_plane"]["bridge_auth_configured"], false);
    assert_eq!(json["workflow"]["provider_runtime_id"], "rhi");
    assert_eq!(json["workflow"]["binding_model"], "out_of_process_worker");
    assert_eq!(json["workflow"]["state"], "not_configured");
    assert_eq!(json["workflow"]["provenance"], "unavailable");
    assert_eq!(json["workflow"]["source"], "no explicit capability binding");
    assert_eq!(json["workflow"]["hyf_helper_state"], "not_implied");
    assert_eq!(json["hyf_provider"]["provider_runtime_id"], "hyf");
    assert_eq!(json["hyf_provider"]["binding_model"], "stdio_service");
    assert_eq!(json["hyf_provider"]["state"], "disabled");
    assert_eq!(json["hyf_provider"]["provenance"], "disabled");
    assert_eq!(
        json["hyf_provider"]["source"],
        "hyf status control request · local first"
    );
    assert_eq!(json["rpc"]["url"], "http://127.0.0.1:7070");
    assert_eq!(json["rpc"]["bridge_auth_configured"], false);
    assert_eq!(
        json["resolved_providers"]
            .as_array()
            .expect("resolved providers")
            .len(),
        3
    );
    assert_eq!(
        json["capability_bindings"]
            .as_array()
            .expect("capability bindings")
            .len(),
        4
    );
    let signer = binding_by_capability(&json, "signer.remote_nip46");
    assert_eq!(signer["provider_runtime_id"], "myc");
    assert_eq!(signer["binding_model"], "session_authorized_remote_signer");
    assert_eq!(signer["state"], "disabled");
    assert_eq!(signer["source"], "independent local signer mode");
    let write = binding_by_capability(&json, "write_plane.trade_jsonrpc");
    assert_eq!(write["provider_runtime_id"], "radrootsd");
    assert_eq!(write["binding_model"], "daemon_backed_jsonrpc");
    assert_eq!(write["state"], "not_configured");
    let workflow = binding_by_capability(&json, "workflow.trade");
    assert_eq!(workflow["provider_runtime_id"], "rhi");
    assert_eq!(workflow["binding_model"], "out_of_process_worker");
    assert_eq!(workflow["state"], "not_configured");
    let inference = binding_by_capability(&json, "inference.hyf_stdio");
    assert_eq!(inference["provider_runtime_id"], "hyf");
    assert_eq!(inference["binding_model"], "stdio_service");
    assert_eq!(inference["state"], "disabled");
    assert_eq!(inference["source"], "hyf disabled by config");
}

#[test]
fn config_show_machine_output_rejects_stdout_logging() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_show_command_in(dir.path())
        .args(["--json", "--log-stdout", "config", "show"])
        .output()
        .expect("run config show");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("stdout logging"));
    assert!(stderr.contains("json output"));

    let env_output = runtime_show_command_in(dir.path())
        .env("RADROOTS_CLI_LOGGING_STDOUT", "true")
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert_eq!(env_output.status.code(), Some(2));
    assert!(env_output.stdout.is_empty());
    let stderr = String::from_utf8(env_output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("RADROOTS_CLI_LOGGING_STDOUT"));
}

#[test]
fn config_show_json_reports_detected_legacy_cli_paths_without_moving_them() {
    let dir = tempdir().expect("tempdir");
    let home = dir.path().join("home");
    let old_config = home.join(".config/radroots/config.toml");
    let old_state_root = home.join(".local/share/radroots");
    fs::create_dir_all(old_config.parent().expect("old config parent")).expect("old config dir");
    fs::create_dir_all(old_state_root.join("accounts")).expect("old state dir");
    fs::write(&old_config, "[relay]\nurls = []\n").expect("old config");
    fs::write(old_state_root.join("accounts/store.json"), "{}").expect("old store");

    let output = runtime_show_command_in(dir.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");

    assert_eq!(json["migration"]["state"], "legacy_state_detected");
    assert_eq!(json["migration"]["silent_startup_relocation"], false);
    let detected = json["migration"]["detected_legacy_paths"]
        .as_array()
        .expect("detected legacy paths array");
    assert_eq!(detected.len(), 2);
    assert_eq!(detected[0]["id"], "cli_user_config_v0");
    assert_eq!(detected[0]["path"], old_config.display().to_string());
    assert_eq!(
        detected[0]["destination"],
        config_root(dir.path())
            .join("apps/cli/config.toml")
            .display()
            .to_string()
    );
    assert_eq!(detected[1]["id"], "cli_user_state_root_v0");
    assert_eq!(detected[1]["path"], old_state_root.display().to_string());
    assert!(
        json["migration"]["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action
                .as_str()
                .is_some_and(|value| value.contains("startup did not move legacy data")))
    );
}

#[test]
fn config_show_json_reports_repo_local_paths_when_requested() {
    let dir = tempdir().expect("tempdir");
    let repo_local_root = dir.path().join(".local/radroots/dev");
    let output = runtime_show_command_in(dir.path())
        .env("RADROOTS_CLI_PATHS_PROFILE", "repo_local")
        .env("RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT", &repo_local_root)
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");

    assert_eq!(json["paths"]["profile"], "repo_local");
    assert_eq!(
        json["paths"]["profile_source"],
        "process_env:RADROOTS_CLI_PATHS_PROFILE"
    );
    assert_eq!(json["paths"]["root_source"], "repo_local_root");
    assert_eq!(
        json["paths"]["repo_local_root"],
        repo_local_root.display().to_string()
    );
    assert_eq!(
        json["paths"]["repo_local_root_source"],
        "process_env:RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT"
    );
    assert_eq!(json["paths"]["allowed_profiles"][0], "interactive_user");
    assert_eq!(json["paths"]["allowed_profiles"][1], "repo_local");
    assert_eq!(
        json["paths"]["app_config_path"],
        repo_local_root
            .join("config/apps/cli/config.toml")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["workspace_config_path"],
        repo_local_root.join("config.toml").display().to_string()
    );
    assert_eq!(
        json["paths"]["app_data_root"],
        repo_local_root.join("data/apps/cli").display().to_string()
    );
    assert_eq!(
        json["paths"]["app_logs_root"],
        repo_local_root.join("logs/apps/cli").display().to_string()
    );
    assert_eq!(
        json["paths"]["shared_accounts_data_root"],
        repo_local_root
            .join("data/shared/accounts")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["shared_accounts_secrets_root"],
        repo_local_root
            .join("secrets/shared/accounts")
            .display()
            .to_string()
    );
    assert_eq!(
        json["paths"]["default_identity_path"],
        repo_local_root
            .join("secrets/shared/identities/default.json")
            .display()
            .to_string()
    );
}

#[test]
fn config_show_json_reads_repo_local_workspace_config_from_explicit_root() {
    let dir = tempdir().expect("tempdir");
    let repo_local_root = dir.path().join("repo-runtime");
    fs::create_dir_all(&repo_local_root).expect("repo-local root");
    fs::write(
        repo_local_root.join("config.toml"),
        "[relay]\nurls = [\"wss://relay.repo-local\"]\npublish_policy = \"any\"\n",
    )
    .expect("write repo-local workspace config");

    let output = runtime_show_command_in(dir.path())
        .env("RADROOTS_CLI_PATHS_PROFILE", "repo_local")
        .env("RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT", &repo_local_root)
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(
        json["paths"]["workspace_config_path"],
        repo_local_root.join("config.toml").display().to_string()
    );
    assert_eq!(json["config_files"]["workspace_present"], true);
    assert_eq!(json["relay"]["count"], 1);
    assert_eq!(json["relay"]["urls"][0], "wss://relay.repo-local");
    assert_eq!(json["relay"]["source"], "workspace config · local first");
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
            "--output",
            "json",
            "--trace",
            "--dry-run",
            "--no-color",
            "--no-input",
            "--yes",
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
    assert_eq!(json["interaction"]["input_enabled"], false);
    assert_eq!(json["interaction"]["assume_yes"], true);
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
    let config_dir = dir.path().join("infra/local/runtime/radroots");
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
    let config_dir = dir.path().join("infra/local/runtime/radroots");
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
fn config_show_reports_explicit_capability_bindings() {
    let dir = tempdir().expect("tempdir");
    let workspace_config_dir = dir.path().join("infra/local/runtime/radroots");
    let user_config_dir = config_root(dir.path()).join("apps/cli");
    fs::create_dir_all(&workspace_config_dir).expect("workspace config dir");
    fs::create_dir_all(&user_config_dir).expect("user config dir");
    fs::write(
        workspace_config_dir.join("config.toml"),
        r#"
[[capability_binding]]
capability = "write_plane.trade_jsonrpc"
provider = "radrootsd"
target_kind = "explicit_endpoint"
target = "https://rpc.workspace.test/jsonrpc"

[[capability_binding]]
capability = "inference.hyf_stdio"
provider = "hyf"
target_kind = "managed_instance"
target = "workspace-hyf"
"#,
    )
    .expect("write workspace config");
    fs::write(
        user_config_dir.join("config.toml"),
        r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
managed_account_ref = "acct_demo"
signer_session_ref = "session_demo"

[[capability_binding]]
capability = "workflow.trade"
provider = "rhi"
target_kind = "managed_instance"
target = "workflow-default"

[[capability_binding]]
capability = "inference.hyf_stdio"
provider = "hyf"
target_kind = "explicit_endpoint"
target = "bin/hyfd-user"
"#,
    )
    .expect("write user config");

    let output = runtime_show_command_in(dir.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");

    let signer = binding_by_capability(&json, "signer.remote_nip46");
    assert_eq!(signer["state"], "configured");
    assert_eq!(signer["source"], "user config [[capability_binding]]");
    assert_eq!(signer["target_kind"], "managed_instance");
    assert_eq!(signer["target"], "default");
    assert_eq!(signer["managed_account_ref"], "acct_demo");
    assert_eq!(signer["signer_session_ref"], "session_demo");

    let write = binding_by_capability(&json, "write_plane.trade_jsonrpc");
    assert_eq!(write["state"], "configured");
    assert_eq!(write["source"], "workspace config [[capability_binding]]");
    assert_eq!(write["target_kind"], "explicit_endpoint");
    assert_eq!(write["target"], "https://rpc.workspace.test/jsonrpc");

    let workflow = binding_by_capability(&json, "workflow.trade");
    assert_eq!(workflow["state"], "configured");
    assert_eq!(workflow["source"], "user config [[capability_binding]]");
    assert_eq!(workflow["target_kind"], "managed_instance");
    assert_eq!(workflow["target"], "workflow-default");
    assert_eq!(json["workflow"]["provider_runtime_id"], "rhi");
    assert_eq!(json["workflow"]["state"], "unavailable");
    assert_eq!(json["workflow"]["provenance"], "managed_default");
    assert_eq!(
        json["workflow"]["source"],
        "user config [[capability_binding]]"
    );
    assert_eq!(json["workflow"]["target_kind"], "managed_instance");
    assert_eq!(json["workflow"]["target"], "workflow-default");
    assert_eq!(json["workflow"]["hyf_helper_state"], "not_implied");
    assert!(
        json["workflow"]["hyf_helper_detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("do not imply"))
    );
    assert_eq!(json["write_plane"]["state"], "unconfigured");
    assert_eq!(json["write_plane"]["provenance"], "explicit_binding");
    assert_eq!(
        json["write_plane"]["source"],
        "workspace config [[capability_binding]]"
    );
    assert_eq!(json["write_plane"]["target_kind"], "explicit_endpoint");
    assert_eq!(
        json["write_plane"]["target"],
        "https://rpc.workspace.test/jsonrpc"
    );
    assert_eq!(json["write_plane"]["bridge_auth_configured"], false);
    assert_eq!(json["hyf_provider"]["provider_runtime_id"], "hyf");
    assert_eq!(json["hyf_provider"]["provenance"], "explicit_binding");
    assert_eq!(json["hyf_provider"]["target_kind"], "explicit_endpoint");
    assert_eq!(json["hyf_provider"]["target"], "bin/hyfd-user");
    assert_eq!(json["hyf_provider"]["executable"], "bin/hyfd-user");

    let inference = binding_by_capability(&json, "inference.hyf_stdio");
    assert_eq!(inference["state"], "configured");
    assert_eq!(inference["source"], "user config [[capability_binding]]");
    assert_eq!(inference["target_kind"], "explicit_endpoint");
    assert_eq!(inference["target"], "bin/hyfd-user");
}

#[test]
fn config_show_uses_managed_default_write_plane_when_local_instance_exists() {
    let dir = tempdir().expect("tempdir");
    let registry_path = runtime_manager_registry_path(dir.path());
    fs::create_dir_all(registry_path.parent().expect("registry parent")).expect("registry dir");
    let managed_config_path = dir.path().join("managed-radrootsd.toml");
    let bridge_token_path = dir.path().join("managed-bridge-token.txt");
    fs::write(
        &managed_config_path,
        "[metadata]\nname = \"managed-radrootsd\"\n",
    )
    .expect("write managed config");
    fs::write(&bridge_token_path, "managed-bridge-token").expect("write managed token");
    fs::write(
        &registry_path,
        format!(
            r#"schema = "radroots_runtime-instance-registry"
schema_version = 1

[[instances]]
runtime_id = "radrootsd"
instance_id = "local"
management_mode = "interactive_user_managed"
install_state = "configured"
binary_path = "/tmp/radrootsd"
config_path = "{}"
logs_path = "/tmp/radrootsd/logs"
run_path = "/tmp/radrootsd/run"
installed_version = "0.1.0"
health_endpoint = "http://127.0.0.1:7444"
secret_material_ref = "{}"
"#,
            managed_config_path.display(),
            bridge_token_path.display()
        ),
    )
    .expect("write managed registry");

    let output = runtime_show_command_in(dir.path())
        .args(["--json", "config", "show"])
        .output()
        .expect("run config show");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["write_plane"]["state"], "configured");
    assert_eq!(json["write_plane"]["provenance"], "managed_default");
    assert_eq!(
        json["write_plane"]["source"],
        "managed preferred radrootsd instance"
    );
    assert_eq!(json["write_plane"]["target_kind"], "managed_instance");
    assert_eq!(json["write_plane"]["target"], "local");
    assert_eq!(json["write_plane"]["bridge_auth_configured"], true);
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
