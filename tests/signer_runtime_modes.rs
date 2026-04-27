mod support;

use std::path::Path;

use serde_json::Value;
use support::RadrootsCliSandbox;

#[test]
fn local_signer_status_reports_unconfigured_without_account() {
    let sandbox = RadrootsCliSandbox::new();

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["schema_version"], "radroots.cli.output.v1");
    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["kind"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "local");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(
        value["result"]["signer_account_id"],
        serde_json::Value::Null
    );
    assert_eq!(value["result"]["binding"]["state"], "disabled");
    assert_eq!(value["result"]["local"], serde_json::Value::Null);
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn local_signer_status_reports_ready_after_account_create() {
    let sandbox = RadrootsCliSandbox::new();

    let created = sandbox.json_success(&["--format", "json", "account", "create"]);
    assert_eq!(created["operation_id"], "account.create");
    assert_eq!(created["result"]["state"], "created");
    assert_eq!(created["result"]["account"]["signer"], "local");
    assert_eq!(created["result"]["account"]["is_default"], true);
    let account_id = created["result"]["account"]["id"]
        .as_str()
        .expect("created account id");

    let status = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(status["operation_id"], "signer.status.get");
    assert_eq!(status["result"]["mode"], "local");
    assert_eq!(status["result"]["state"], "ready");
    assert_eq!(status["result"]["signer_account_id"], account_id);
    assert_eq!(status["result"]["local"]["account_id"], account_id);
    assert_eq!(status["result"]["local"]["availability"], "secret_backed");
    assert_eq!(status["result"]["local"]["secret_backed"], true);
    assert_eq!(status["result"]["local"]["backend"], "encrypted_file");
    assert_eq!(status["result"]["local"]["used_fallback"], false);
    assert_eq!(status["result"]["binding"]["state"], "disabled");
}

#[test]
fn myc_signer_status_reports_unavailable_for_missing_executable() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    configure_myc_mode(&sandbox, &missing_myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
    assert_contains(&value["result"]["myc"]["reason"], "not found");
}

#[cfg(unix)]
#[test]
fn myc_signer_status_reports_unavailable_for_command_failure() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = sandbox.write_fake_myc("myc-failure", "printf 'fake myc failed\\n' >&2\nexit 42");
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
    assert_contains(&value["result"]["myc"]["reason"], "status code 42");
    assert_contains(&value["result"]["myc"]["reason"], "fake myc failed");
}

#[cfg(unix)]
#[test]
fn myc_signer_status_reports_unavailable_for_invalid_json() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = sandbox.write_fake_myc("myc-invalid-json", "printf 'not json\\n'");
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
    assert_contains(&value["result"]["myc"]["reason"], "not valid JSON");
}

#[cfg(unix)]
#[test]
fn myc_signer_status_reports_unconfigured_when_ready_without_binding() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = sandbox.write_fake_myc(
        "myc-ready-no-binding",
        r#"printf '%s\n' '{"status_contract_version":1,"status":"ready","ready":true,"signer_backend":{"remote_session_count":0,"remote_sessions":[]}}'"#,
    );
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["myc"]["state"], "ready");
    assert_eq!(value["result"]["myc"]["ready"], true);
    assert_eq!(value["result"]["myc"]["remote_session_count"], 0);
    assert_eq!(value["result"]["binding"]["state"], "unconfigured");
    assert_contains(&value["result"]["binding"]["reason"], "signer.remote_nip46");
}

fn configure_myc_mode(sandbox: &RadrootsCliSandbox, executable: &Path) {
    sandbox.write_app_config(&format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
        toml_string(executable.display().to_string().as_str())
    ));
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn assert_contains(value: &Value, needle: &str) {
    let value = value.as_str().expect("string value");
    assert!(
        value.contains(needle),
        "expected `{value}` to contain `{needle}`"
    );
}
