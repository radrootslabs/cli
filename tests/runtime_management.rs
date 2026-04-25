use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use assert_cmd::prelude::*;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::Value;
use tar::Builder;
use tempfile::tempdir;

fn appdata_root(workdir: &Path) -> PathBuf {
    workdir.join("roaming").join("Radroots")
}

fn localappdata_root(workdir: &Path) -> PathBuf {
    workdir.join("local").join("Radroots")
}

fn interactive_root(workdir: &Path) -> PathBuf {
    if cfg!(windows) {
        localappdata_root(workdir)
    } else {
        workdir.join("home").join(".radroots")
    }
}

fn config_root(workdir: &Path) -> PathBuf {
    if cfg!(windows) {
        appdata_root(workdir).join("config")
    } else {
        interactive_root(workdir).join("config")
    }
}

fn cache_root(workdir: &Path) -> PathBuf {
    if cfg!(windows) {
        localappdata_root(workdir).join("cache")
    } else {
        interactive_root(workdir).join("cache")
    }
}

fn runtime_manager_registry_path(workdir: &Path) -> PathBuf {
    config_root(workdir).join("shared/runtime-manager/instances.toml")
}

fn runtime_manager_artifact_cache_dir(workdir: &Path) -> PathBuf {
    cache_root(workdir)
        .join("shared/runtime-manager/artifacts")
        .join("radrootsd")
        .join("stable")
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
        "RADROOTS_MYC_STATUS_TIMEOUT_MS",
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command.env("RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE", "false");
    command
}

#[cfg(not(windows))]
fn current_server_target_id() -> &'static str {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
        "x86_64-apple-darwin"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "x86_64-unknown-linux-gnu"
    } else {
        panic!("unsupported host target for runtime-management tests")
    }
}

#[cfg(not(windows))]
fn write_cached_radrootsd_artifact(workdir: &Path) -> PathBuf {
    let artifact_dir = runtime_manager_artifact_cache_dir(workdir);
    fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let file_name = format!(
        "radrootsd-0.1.0-alpha.2-{}.tar.gz",
        current_server_target_id()
    );
    let archive_path = artifact_dir.join(file_name);
    let script = artifact_dir.join("radrootsd");
    fs::write(
        &script,
        "#!/bin/sh\nprintf 'managed radrootsd started\\n' >> \"${TMPDIR:-/tmp}/radrootsd-managed.log\"\nsleep 30\n",
    )
    .expect("write script");
    let file = File::create(&archive_path).expect("archive file");
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    builder
        .append_path_with_name(&script, "radrootsd/bin/radrootsd")
        .expect("append script");
    builder.finish().expect("finish archive");
    archive_path
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

#[cfg(not(windows))]
#[test]
fn runtime_manages_radrootsd_lifecycle_end_to_end() {
    let dir = tempdir().expect("tempdir");
    let artifact_path = write_cached_radrootsd_artifact(dir.path());

    let install = runtime_command_in(dir.path())
        .args(["--json", "runtime", "install", "radrootsd"])
        .output()
        .expect("runtime install");
    assert!(install.status.success());
    let install_json: Value =
        serde_json::from_slice(install.stdout.as_slice()).expect("install json");
    assert_eq!(install_json["action"], "install");
    assert_eq!(install_json["state"], "configured");
    assert!(
        install_json["detail"]
            .as_str()
            .expect("detail")
            .contains(artifact_path.display().to_string().as_str())
    );

    let registry_path = runtime_manager_registry_path(dir.path());
    let registry_raw = fs::read_to_string(&registry_path).expect("registry");
    assert!(registry_raw.contains("health_endpoint = \"http://127.0.0.1:7070\""));
    assert!(registry_raw.contains("secret_material_ref = "));

    let start = runtime_command_in(dir.path())
        .args(["--json", "runtime", "start", "radrootsd"])
        .output()
        .expect("runtime start");
    assert!(start.status.success());
    let start_json: Value = serde_json::from_slice(start.stdout.as_slice()).expect("start json");
    assert_eq!(start_json["state"], "running");

    thread::sleep(Duration::from_millis(150));

    let running_status = runtime_command_in(dir.path())
        .args(["--json", "runtime", "status", "radrootsd"])
        .output()
        .expect("runtime status");
    assert!(running_status.status.success());
    let running_json: Value =
        serde_json::from_slice(running_status.stdout.as_slice()).expect("status json");
    assert_eq!(running_json["state"], "configured");
    assert_eq!(running_json["health_state"], "running");

    let config_set = runtime_command_in(dir.path())
        .args([
            "--json",
            "runtime",
            "config",
            "set",
            "radrootsd",
            "config.rpc.addr",
            "127.0.0.1:7444",
        ])
        .output()
        .expect("runtime config set");
    assert!(config_set.status.success());
    let config_set_json: Value =
        serde_json::from_slice(config_set.stdout.as_slice()).expect("config set json");
    assert_eq!(config_set_json["state"], "configured");

    let updated_registry_raw = fs::read_to_string(&registry_path).expect("updated registry");
    assert!(updated_registry_raw.contains("health_endpoint = \"http://127.0.0.1:7444\""));

    let stop = runtime_command_in(dir.path())
        .args(["--json", "runtime", "stop", "radrootsd"])
        .output()
        .expect("runtime stop");
    assert!(stop.status.success());
    let stop_json: Value = serde_json::from_slice(stop.stdout.as_slice()).expect("stop json");
    assert_eq!(stop_json["state"], "stopped");

    let uninstall = runtime_command_in(dir.path())
        .args(["--json", "runtime", "uninstall", "radrootsd"])
        .output()
        .expect("runtime uninstall");
    assert!(uninstall.status.success());
    let uninstall_json: Value =
        serde_json::from_slice(uninstall.stdout.as_slice()).expect("uninstall json");
    assert_eq!(uninstall_json["state"], "uninstalled");

    let final_status = runtime_command_in(dir.path())
        .args(["--json", "runtime", "status", "radrootsd"])
        .output()
        .expect("runtime status after uninstall");
    assert!(final_status.status.success());
    let final_json: Value =
        serde_json::from_slice(final_status.stdout.as_slice()).expect("final status json");
    assert_eq!(final_json["state"], "not_installed");
    assert_eq!(final_json["health_state"], "not_installed");
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
    assert!(
        json["stdout_log_path"]
            .as_str()
            .expect("stdout log path")
            .ends_with("shared/runtime-manager/radrootsd/local/stdout.log")
    );
    assert!(
        json["stderr_log_path"]
            .as_str()
            .expect("stderr log path")
            .ends_with("shared/runtime-manager/radrootsd/local/stderr.log")
    );
}

#[test]
fn runtime_config_show_uses_registered_instance_config_path() {
    let dir = tempdir().expect("tempdir");
    let registry_path = runtime_manager_registry_path(dir.path());
    let config_path = dir.path().join("managed").join("radrootsd-local.toml");
    let token_path = dir.path().join("managed").join("bridge-token.txt");
    fs::create_dir_all(config_path.parent().expect("config parent")).expect("create config dir");
    fs::write(
        &config_path,
        "[metadata]\nname = \"managed-radrootsd\"\n[config.rpc]\naddr = \"127.0.0.1:7070\"\n[config.bridge]\nenabled = true\nbearer_token = \"redacted\"\n",
    )
    .expect("write config");
    fs::write(&token_path, "redacted").expect("write token");
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
health_endpoint = "http://127.0.0.1:7070"
secret_material_ref = "{secret_material_ref}"
"#,
            binary_path = dir.path().join("bin/radrootsd").display(),
            config_path = config_path.display(),
            logs_path = dir.path().join("managed/logs/radrootsd-local").display(),
            run_path = dir.path().join("managed/run/radrootsd-local").display(),
            secret_material_ref = token_path.display(),
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
