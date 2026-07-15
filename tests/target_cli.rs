use assert_cmd::Command;
use radroots_runtime_contract_v1::RUNTIME_OPERATION_DESCRIPTORS_V1;
use serde_json::Value;

const UUID_V7: &str = "01890f78-9abc-7def-8abc-123456789abc";

fn radroots() -> Command {
    Command::cargo_bin("radroots").expect("radroots binary")
}

fn json_output(args: &[&str]) -> Value {
    let output = radroots()
        .args(args)
        .assert()
        .success()
        .get_output()
        .clone();
    serde_json::from_slice(&output.stdout).expect("json stdout")
}

fn json_failure(args: &[&str]) -> Value {
    let output = radroots()
        .args(args)
        .assert()
        .failure()
        .get_output()
        .clone();
    serde_json::from_slice(&output.stdout).expect("json failure stdout")
}

#[test]
fn generated_v1_read_operation_runs_from_binary() {
    let value = json_output(&["--format", "json", "profile", "inspect"]);

    assert_eq!(value["operation_id"], "profile.inspect");
    assert_eq!(value["kind"], "profile.inspect");
    assert_eq!(value["status"], "ok");
}

#[test]
fn binary_rejects_retired_command_surfaces() {
    for args in [
        &["workspace", "get"][..],
        &["config", "get"],
        &["mesh", "status"],
        &["trade", "submit"],
        &["listing", "archive"],
        &["validation", "receipt", "list"],
    ] {
        radroots().args(args).assert().failure();
    }
}

#[test]
fn reads_reject_idempotency_keys() {
    let value = json_failure(&[
        "--format",
        "json",
        "--idempotency-key",
        UUID_V7,
        "profile",
        "inspect",
    ]);

    assert_eq!(value["operation_id"], "profile.inspect");
    assert_eq!(value["status"], "error");
    assert_eq!(value["reason_code"], "invalid_input");
    assert!(
        value["errors"][0]["message"]
            .as_str()
            .expect("error message")
            .contains("forbids idempotency")
    );
}

#[test]
fn mutations_require_uuid_v7_idempotency_keys() {
    let missing = json_failure(&["--format", "json", "--yes", "profile", "reset"]);
    assert_eq!(missing["operation_id"], "profile.reset");
    assert!(
        missing["errors"][0]["message"]
            .as_str()
            .expect("missing idempotency message")
            .contains("requires UUIDv7 idempotency")
    );

    let invalid = json_failure(&[
        "--format",
        "json",
        "--yes",
        "--idempotency-key",
        "profile-reset",
        "profile",
        "reset",
    ]);
    assert_eq!(invalid["operation_id"], "profile.reset");
    assert!(
        invalid["errors"][0]["message"]
            .as_str()
            .expect("invalid idempotency message")
            .contains("invalid UUIDv7 idempotency")
    );
}

#[test]
fn approval_required_operations_accept_explicit_yes_confirmation() {
    let value = json_output(&[
        "--format",
        "json",
        "--dry-run",
        "--yes",
        "--idempotency-key",
        UUID_V7,
        "profile",
        "reset",
    ]);

    assert_eq!(value["operation_id"], "profile.reset");
    assert_eq!(value["status"], "ok");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["idempotency_key"], UUID_V7);
}

#[test]
fn binary_v1_catalog_has_no_retired_operation_ids() {
    let operation_ids = RUNTIME_OPERATION_DESCRIPTORS_V1
        .iter()
        .map(|descriptor| descriptor.operation_id.as_str())
        .collect::<Vec<_>>();

    for retired in [
        vec!["workspace", "get"],
        vec!["config", "get"],
        vec!["mesh", "status"],
        vec!["trade", "submit"],
        vec!["listing", "archive"],
        vec!["validation", "receipt", "list"],
    ]
    .into_iter()
    .map(|parts| parts.join("."))
    {
        assert!(!operation_ids.contains(&retired.as_str()), "{retired}");
    }
}
