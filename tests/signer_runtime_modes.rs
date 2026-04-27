mod support;

use std::path::Path;

use radroots_identity::{RadrootsIdentity, RadrootsIdentityPublic};
use serde_json::{Value, json};
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
    let myc = write_fake_myc_status(
        &sandbox,
        "myc-ready-no-binding",
        ready_myc_payload(Vec::new()),
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

#[cfg(unix)]
#[test]
fn myc_binding_reports_unsupported_target_kind() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = write_fake_myc_status(
        &sandbox,
        "myc-ready-unsupported",
        ready_myc_payload(Vec::new()),
    );
    configure_myc_mode_with_binding(&sandbox, &myc, "explicit_endpoint", "default", None, None);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["binding"]["state"], "unsupported");
    assert_contains(&value["result"]["binding"]["reason"], "target_kind");
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_no_authorized_sessions() {
    let sandbox = RadrootsCliSandbox::new();
    let signer = identity_public(2);
    let user = identity_public(3);
    let payload = ready_myc_payload(vec![remote_session("session-ping", &signer, &user, "ping")]);
    let myc = write_fake_myc_status(&sandbox, "myc-no-authorized", payload);
    configure_myc_mode_with_binding(&sandbox, &myc, "managed_instance", "default", None, None);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["binding"]["state"], "unavailable");
    assert_eq!(value["result"]["binding"]["matched_session_count"], 0);
    assert_contains(
        &value["result"]["binding"]["reason"],
        "no authorized remote signer session",
    );
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_ambiguous_authorized_sessions() {
    let sandbox = RadrootsCliSandbox::new();
    let signer = identity_public(4);
    let user_one = identity_public(5);
    let user_two = identity_public(6);
    let payload = ready_myc_payload(vec![
        remote_session("session-one", &signer, &user_one, "sign_event"),
        remote_session("session-two", &signer, &user_two, "sign_event"),
    ]);
    let myc = write_fake_myc_status(&sandbox, "myc-ambiguous", payload);
    configure_myc_mode_with_binding(&sandbox, &myc, "managed_instance", "default", None, None);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["binding"]["state"], "ambiguous");
    assert_eq!(value["result"]["binding"]["matched_session_count"], 2);
    assert_contains(
        &value["result"]["binding"]["reason"],
        "multiple authorized remote signer sessions",
    );
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_ready_for_one_authorized_session() {
    let sandbox = RadrootsCliSandbox::new();
    let signer = identity_public(7);
    let user = identity_public(8);
    let payload = ready_myc_payload(vec![remote_session(
        "session-ready",
        &signer,
        &user,
        "sign_event",
    )]);
    let myc = write_fake_myc_status(&sandbox, "myc-ready-bound", payload);
    configure_myc_mode_with_binding(
        &sandbox,
        &myc,
        "managed_instance",
        "default",
        Some(user.id.as_str()),
        None,
    );

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "ready");
    assert_eq!(value["result"]["signer_account_id"], user.id.as_str());
    assert_eq!(value["result"]["binding"]["state"], "ready");
    assert_eq!(
        value["result"]["binding"]["resolved_signer_session_id"],
        "session-ready"
    );
    assert_eq!(value["result"]["binding"]["matched_session_count"], 1);
    assert_eq!(
        value["result"]["write_kinds"]
            .as_array()
            .expect("write kinds")
            .iter()
            .filter(|kind| kind["ready"] == true)
            .count(),
        4
    );
}

fn configure_myc_mode(sandbox: &RadrootsCliSandbox, executable: &Path) {
    sandbox.write_app_config(&format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
        toml_string(executable.display().to_string().as_str())
    ));
}

fn configure_myc_mode_with_binding(
    sandbox: &RadrootsCliSandbox,
    executable: &Path,
    target_kind: &str,
    target: &str,
    managed_account_ref: Option<&str>,
    signer_session_ref: Option<&str>,
) {
    let mut raw = format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n\n[[capability_binding]]\ncapability = \"signer.remote_nip46\"\nprovider = \"myc\"\ntarget_kind = \"{}\"\ntarget = \"{}\"\n",
        toml_string(executable.display().to_string().as_str()),
        toml_string(target_kind),
        toml_string(target)
    );
    if let Some(value) = managed_account_ref {
        raw.push_str(&format!(
            "managed_account_ref = \"{}\"\n",
            toml_string(value)
        ));
    }
    if let Some(value) = signer_session_ref {
        raw.push_str(&format!(
            "signer_session_ref = \"{}\"\n",
            toml_string(value)
        ));
    }
    sandbox.write_app_config(raw.as_str());
}

#[cfg(unix)]
fn write_fake_myc_status(
    sandbox: &RadrootsCliSandbox,
    name: &str,
    payload: Value,
) -> std::path::PathBuf {
    let raw = serde_json::to_string(&payload).expect("myc status payload");
    sandbox.write_fake_myc(
        name,
        format!("printf '%s\\n' '{}'", shell_single_quoted(raw.as_str())).as_str(),
    )
}

fn ready_myc_payload(remote_sessions: Vec<Value>) -> Value {
    json!({
        "status_contract_version": 1,
        "status": "ready",
        "ready": true,
        "signer_backend": {
            "remote_session_count": remote_sessions.len(),
            "remote_sessions": remote_sessions
        }
    })
}

fn remote_session(
    connection_id: &str,
    signer_identity: &RadrootsIdentityPublic,
    user_identity: &RadrootsIdentityPublic,
    permissions: &str,
) -> Value {
    json!({
        "connection_id": connection_id,
        "signer_identity": signer_identity,
        "user_identity": user_identity,
        "relays": ["wss://relay.example.test"],
        "permissions": permissions
    })
}

fn identity_public(seed: u8) -> RadrootsIdentityPublic {
    let secret = [seed; 32];
    RadrootsIdentity::from_secret_key_bytes(&secret)
        .expect("fixture identity")
        .to_public()
}

fn shell_single_quoted(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
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
