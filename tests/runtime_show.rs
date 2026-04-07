use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn runtime_show_command_in(workdir: &Path) -> Command {
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
        "RADROOTS_SIGNER_BACKEND",
        "RADROOTS_MYC_EXECUTABLE",
    ] {
        command.env_remove(key);
    }
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
    assert_eq!(
        json["paths"]["user_config_path"],
        dir.path()
            .join("home")
            .join(".config/radroots/config.toml")
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
        json["paths"]["user_state_root"],
        dir.path()
            .join("home")
            .join(".local/share/radroots")
            .display()
            .to_string()
    );
    assert_eq!(json["logging"]["initialized"], true);
    assert_eq!(json["logging"]["stdout"], false);
    assert_eq!(json["logging"]["directory"], Value::Null);
    assert_eq!(json["config_files"]["user_present"], false);
    assert_eq!(json["config_files"]["workspace_present"], false);
    assert_eq!(json["account"]["selector"], Value::Null);
    assert_eq!(
        json["account"]["store_path"],
        dir.path()
            .join("home")
            .join(".local/share/radroots/accounts/store.json")
            .display()
            .to_string()
    );
    assert_eq!(
        json["account"]["secrets_dir"],
        dir.path()
            .join("home")
            .join(".local/share/radroots/accounts/secrets")
            .display()
            .to_string()
    );
    assert_eq!(json["account"]["legacy_identity_path"], "identity.json");
    assert_eq!(json["signer"]["backend"], "local");
    assert_eq!(json["myc"]["executable"], "myc");
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
        .env("RADROOTS_SIGNER_BACKEND", "myc")
        .env("RADROOTS_MYC_EXECUTABLE", "bin/myc")
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
    assert_eq!(
        json["account"]["legacy_identity_path"],
        "state/identity.json"
    );
    assert_eq!(json["signer"]["backend"], "myc");
    assert_eq!(json["myc"]["executable"], "bin/myc");
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
