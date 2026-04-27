use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;

fn radroots() -> Command {
    Command::cargo_bin("radroots").expect("binary")
}

#[test]
fn root_help_exposes_only_mvp_namespaces() {
    let output = radroots().arg("--help").output().expect("run root help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    for namespace in [
        "workspace",
        "health",
        "config",
        "account",
        "signer",
        "relay",
        "store",
        "sync",
        "runtime",
        "job",
        "farm",
        "listing",
        "market",
        "basket",
        "order",
    ] {
        assert!(
            help_lists(&stdout, namespace),
            "root help should contain `{namespace}`"
        );
    }

    for removed in [
        "setup", "status", "doctor", "sell", "find", "local", "net", "myc", "rpc", "product",
        "message", "approval", "agent",
    ] {
        assert!(
            !help_lists(&stdout, removed),
            "root help should not contain `{removed}`"
        );
    }
}

fn help_lists(stdout: &str, command: &str) -> bool {
    stdout.lines().any(|line| {
        let line = line.trim_start();
        line == command || line.starts_with(&format!("{command} "))
    })
}

#[test]
fn removed_global_flags_are_rejected_publicly() {
    for args in [
        ["--output", "json", "workspace", "get"].as_slice(),
        ["--json", "workspace", "get"].as_slice(),
        ["--ndjson", "workspace", "get"].as_slice(),
        ["--yes", "workspace", "get"].as_slice(),
        ["--non-interactive", "workspace", "get"].as_slice(),
    ] {
        let output = radroots().args(args).output().expect("run removed flag");

        assert!(!output.status.success(), "`{args:?}` should be rejected");
        let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
        assert!(stderr.contains("unexpected argument") || stderr.contains("unrecognized"));
    }
}

#[test]
fn removed_command_families_are_rejected_publicly() {
    for command in [
        "setup", "status", "doctor", "sell", "find", "local", "net", "myc", "rpc", "product",
        "message", "approval", "agent",
    ] {
        let output = radroots()
            .arg(command)
            .output()
            .expect("run removed command");

        assert!(!output.status.success(), "`{command}` should be rejected");
        let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
        assert!(stderr.contains("unrecognized subcommand"));
    }
}

#[test]
fn target_command_outputs_standard_json_envelope() {
    let output = radroots()
        .args(["--format", "json", "workspace", "get"])
        .output()
        .expect("run workspace get");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["schema_version"], "radroots.cli.output.v1");
    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["kind"], "workspace.get");
    assert_eq!(value["dry_run"], false);
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn unsupported_ndjson_returns_structured_invalid_input() {
    let output = radroots()
        .args(["--format", "ndjson", "workspace", "get"])
        .output()
        .expect("run workspace get ndjson");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["errors"][0]["code"], "invalid_input");
    assert_eq!(value["errors"][0]["exit_code"], 2);
}

#[test]
fn required_approval_missing_token_returns_structured_error() {
    let output = radroots()
        .args(["--format", "json", "order", "submit"])
        .output()
        .expect("run order submit");

    assert_eq!(output.status.code(), Some(6));
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "approval_required");
    assert_eq!(value["errors"][0]["exit_code"], 6);
}
