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

fn runtime_command_in(workdir: &Path) -> Command {
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
fn runtime_status_reports_active_managed_target_truth() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_command_in(dir.path())
        .args(["--json", "runtime", "status", "radrootsd"])
        .output()
        .expect("runtime status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["runtime_id"], "radrootsd");
    assert_eq!(json["runtime_group"], "active_managed_target");
    assert_eq!(json["management_posture"], "active_managed_target");
    assert_eq!(json["state"], "not_installed");
    assert_eq!(json["install_state"], "not_installed");
    assert_eq!(json["health_state"], "not_installed");
    assert_eq!(json["instance_id"], "local");
    assert_eq!(json["instance_source"], "bootstrap_default");
    assert_eq!(json["management_mode"], "interactive_user_managed");
}

#[test]
fn runtime_status_reports_defined_future_target_truth() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_command_in(dir.path())
        .args(["--json", "runtime", "status", "myc"])
        .output()
        .expect("runtime status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["runtime_id"], "myc");
    assert_eq!(json["runtime_group"], "defined_managed_target");
    assert_eq!(json["management_posture"], "defined_future_target");
    assert_eq!(json["state"], "defined_not_active");
    assert_eq!(json["lifecycle_actions"], Value::Array(vec![]));
}

#[test]
fn runtime_install_is_exposed_but_truthfully_deferred() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_command_in(dir.path())
        .args(["--json", "runtime", "install", "radrootsd"])
        .output()
        .expect("runtime install");

    assert_eq!(output.status.code(), Some(5));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["action"], "install");
    assert_eq!(json["runtime_id"], "radrootsd");
    assert_eq!(json["state"], "deferred");
    assert_eq!(json["mutates_bindings"], false);
    assert_eq!(json["next_step"], "rpv1-rpi.5");
}

#[test]
fn runtime_logs_reports_managed_log_locations() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_command_in(dir.path())
        .args(["--json", "runtime", "logs", "radrootsd"])
        .output()
        .expect("runtime logs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["runtime_id"], "radrootsd");
    assert_eq!(json["state"], "ready");
    assert!(json["stdout_log_path"]
        .as_str()
        .expect("stdout log path")
        .ends_with("shared/runtime-manager/radrootsd/local/stdout.log"));
    assert!(json["stderr_log_path"]
        .as_str()
        .expect("stderr log path")
        .ends_with("shared/runtime-manager/radrootsd/local/stderr.log"));
}

#[test]
fn runtime_config_show_uses_registered_instance_config_path() {
    let dir = tempdir().expect("tempdir");
    let registry_path = runtime_manager_registry_path(dir.path());
    let config_path = dir.path().join("managed").join("radrootsd-local.toml");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("create config dir");
    fs::write(
        &config_path,
        "[config.rpc]\naddr = \"127.0.0.1:7070\"\n[config.bridge]\nenabled = true\nbearer_token = \"redacted\"\n",
    )
    .expect("write config");
    fs::create_dir_all(registry_path.parent().expect("registry parent")).expect("registry dir");
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
binary_path = "{binary_path}"
config_path = "{config_path}"
logs_path = "{logs_path}"
run_path = "{run_path}"
installed_version = "0.1.0"
"#,
            binary_path = dir.path().join("bin/radrootsd").display(),
            config_path = config_path.display(),
            logs_path = dir.path().join("managed/logs/radrootsd-local").display(),
            run_path = dir.path().join("managed/run/radrootsd-local").display(),
        ),
    )
    .expect("write registry");

    let output = runtime_command_in(dir.path())
        .args(["--json", "runtime", "config", "show", "radrootsd"])
        .output()
        .expect("runtime config show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["runtime_id"], "radrootsd");
    assert_eq!(json["config_format"], "toml");
    assert_eq!(json["config_path"], config_path.display().to_string());
    assert_eq!(json["config_present"], true);
    assert_eq!(json["requires_bootstrap_secret"], true);
    assert_eq!(json["requires_config_bootstrap"], true);
}

#[test]
fn runtime_logs_rejects_bootstrap_only_runtime() {
    let dir = tempdir().expect("tempdir");
    let output = runtime_command_in(dir.path())
        .args(["--json", "runtime", "logs", "hyf"])
        .output()
        .expect("runtime logs");

    assert_eq!(output.status.code(), Some(5));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["runtime_id"], "hyf");
    assert_eq!(json["runtime_group"], "bootstrap_only");
    assert_eq!(json["state"], "unsupported");
}
