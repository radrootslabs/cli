use assert_cmd::Command;
use serde_json::Value;

fn radroots() -> Command {
    Command::cargo_bin("radroots").expect("radroots binary")
}

#[test]
fn help_exposes_the_resource_command_tree() {
    let output = radroots().arg("--help").output().expect("help output");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(stdout.contains("health"));
    assert!(stdout.contains("profile"));
}

#[test]
fn health_uses_the_registry_backed_facade() {
    let output = radroots()
        .args(["health", "inspect", "--format", "json"])
        .output()
        .expect("health output");
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(value["schema"], "radroots.cli.output.v1");
    assert_eq!(value["operation_id"], "health.inspect");
    assert_eq!(value["status"], "ok");
    assert_eq!(value["data"]["ready"], true);
    assert_eq!(value["data"]["profile"]["artifact_graph"], "registry");
}

#[test]
fn retired_runtime_operations_fail_closed() {
    let output = radroots()
        .args(["account", "list", "--format", "json"])
        .output()
        .expect("account output");
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
    assert_eq!(value["operation_id"], "account.list");
    assert_eq!(value["status"], "error");
    assert_eq!(value["error"]["code"], "unsupported_operation");
}
