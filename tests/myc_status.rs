use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use assert_cmd::prelude::*;
use radroots_identity::RadrootsIdentity;
use radroots_nostr_signer::prelude::RadrootsNostrSignerConnectionId;
use serde_json::{Value, json};
use tempfile::tempdir;

fn cli_command_in(workdir: &Path) -> Command {
    let mut command = Command::cargo_bin("radroots").expect("binary");
    command.current_dir(workdir);
    command.env("HOME", workdir.join("home"));
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

fn write_workspace_config(workdir: &Path, contents: &str) {
    let config_dir = workdir.join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write workspace config");
}

#[test]
fn myc_status_reports_ready_for_valid_full_status_payload() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(sample_status_payload(true).to_string()).as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "myc",
            "status",
        ])
        .output()
        .expect("run myc status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["source"], "myc status command · local first");
    assert_eq!(json["ready"], true);
    assert_eq!(json["service_status"], "healthy");
    assert_eq!(json["remote_session_count"], 1);
    assert_eq!(json["remote_sessions"][0]["permissions"][0], "sign_event");
    assert_eq!(json["local_signer"]["availability"], "secret_backed");
    assert_eq!(json["custody"]["signer"]["resolved"], true);
    assert_eq!(
        json["custody"]["user"]["selected_account_state"],
        "public_only"
    );
}

#[test]
fn myc_status_reports_unavailable_for_invalid_status_payload() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(dir.path(), "#!/bin/sh\nprintf '%s\\n' 'this is not json'\n");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "myc",
            "status",
        ])
        .output()
        .expect("run myc status");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "unavailable");
    assert_eq!(json["ready"], false);
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("not valid JSON"))
    );
}

#[test]
fn myc_status_reports_degraded_service_as_external_unavailable() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(sample_status_payload(false).to_string()).as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "myc",
            "status",
        ])
        .output()
        .expect("run myc status");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "degraded");
    assert_eq!(json["service_status"], "degraded");
    assert_eq!(json["ready"], false);
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("transport quorum is below target"))
    );
}

#[test]
fn signer_status_reports_degraded_myc_backend_as_external_unavailable() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(sample_status_payload(false).to_string()).as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["mode"], "myc");
    assert_eq!(json["state"], "degraded");
    assert_eq!(json["source"], "myc status command · local first");
    assert_eq!(json["signer_account_id"], Value::Null);
    assert_eq!(json["myc"]["state"], "degraded");
    assert_eq!(json["myc"]["service_status"], "degraded");
    assert_eq!(json["binding"]["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("transport quorum is below target"))
    );
}

#[test]
fn signer_status_reports_ready_for_configured_myc_managed_account_binding() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let payload = sample_status_payload(true);
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(payload.to_string()).as_str(),
    );
    let managed_account_ref = payload["signer_backend"]["local_signer"]["account_id"]
        .as_str()
        .expect("managed account ref");
    let signer_session_ref = payload["signer_backend"]["remote_sessions"][0]["connection_id"]
        .as_str()
        .expect("signer session ref");
    write_workspace_config(
        dir.path(),
        format!(
            r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
managed_account_ref = "{managed_account_ref}"
signer_session_ref = "{signer_session_ref}"
"#
        )
        .as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["mode"], "myc");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["source"], "workspace config [[capability_binding]]");
    assert_eq!(json["signer_account_id"], managed_account_ref);
    assert_eq!(json["binding"]["state"], "ready");
    assert_eq!(
        json["binding"]["resolved_signer_session_id"],
        signer_session_ref
    );
    assert_eq!(json["binding"]["managed_account_ref"], managed_account_ref);
    assert_eq!(json["binding"]["signer_session_ref"], signer_session_ref);
    assert_eq!(json["myc"]["remote_session_count"], 1);
}

#[test]
fn signer_status_reports_unconfigured_when_myc_binding_is_missing() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(sample_status_payload(true).to_string()).as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["binding"]["state"], "unconfigured");
    assert!(
        json["binding"]["reason"]
            .as_str()
            .is_some_and(|value| value.contains("configure [[capability_binding]]"))
    );
}

#[test]
fn signer_status_reports_unsupported_for_explicit_endpoint_binding() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(sample_status_payload(true).to_string()).as_str(),
    );
    write_workspace_config(
        dir.path(),
        r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "https://myc.example.invalid"
"#,
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["binding"]["state"], "unsupported");
    assert!(
        json["binding"]["reason"]
            .as_str()
            .is_some_and(|value| value.contains("only supports target_kind `managed_instance`"))
    );
}

#[test]
fn signer_status_reports_ambiguous_for_accountless_myc_binding() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let payload = sample_multi_session_status_payload();
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(payload.to_string()).as_str(),
    );
    write_workspace_config(
        dir.path(),
        r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
"#,
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["binding"]["state"], "ambiguous");
    assert_eq!(json["binding"]["matched_session_count"], 2);
}

#[test]
fn signer_status_reports_unauthorized_for_session_without_sign_event_permission() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let payload = sample_status_payload_with_permissions(true, &["nip44_encrypt"]);
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(payload.to_string()).as_str(),
    );
    let managed_account_ref = payload["signer_backend"]["local_signer"]["account_id"]
        .as_str()
        .expect("managed account ref");
    let signer_session_ref = payload["signer_backend"]["remote_sessions"][0]["connection_id"]
        .as_str()
        .expect("signer session ref");
    write_workspace_config(
        dir.path(),
        format!(
            r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
managed_account_ref = "{managed_account_ref}"
signer_session_ref = "{signer_session_ref}"
"#
        )
        .as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["binding"]["state"], "unauthorized");
    assert!(
        json["binding"]["reason"]
            .as_str()
            .is_some_and(|value| value.contains("not approved for `sign_event`"))
    );
}

#[test]
fn signer_status_reports_unavailable_for_missing_bound_session() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let payload = sample_status_payload(true);
    let executable = write_fake_myc(
        dir.path(),
        successful_status_script(payload.to_string()).as_str(),
    );
    let managed_account_ref = payload["signer_backend"]["local_signer"]["account_id"]
        .as_str()
        .expect("managed account ref");
    write_workspace_config(
        dir.path(),
        format!(
            r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
managed_account_ref = "{managed_account_ref}"
signer_session_ref = "missing-session"
"#
        )
        .as_str(),
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unavailable");
    assert_eq!(json["binding"]["state"], "unavailable");
    assert!(
        json["binding"]["reason"]
            .as_str()
            .is_some_and(|value| value.contains("missing-session"))
    );
}

#[test]
fn myc_status_reports_unavailable_when_executable_is_missing() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let missing = dir.path().join("missing-myc");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--myc-executable",
            missing.to_str().expect("missing path"),
            "myc",
            "status",
        ])
        .output()
        .expect("run myc status");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "unavailable");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("was not found"))
    );
}

#[test]
fn myc_status_reports_unavailable_for_non_zero_exit() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        "#!/bin/sh\nprintf '%s\\n' 'transport unavailable' >&2\nexit 42\n",
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "myc",
            "status",
        ])
        .output()
        .expect("run myc status");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "unavailable");
    let reason = json["reason"].as_str().expect("reason string");
    assert!(reason.contains("status code 42") || reason.contains("transport unavailable"));
}

#[test]
fn myc_status_reports_unavailable_for_timeout() {
    let _guard = myc_test_guard();
    let dir = tempdir().expect("tempdir");
    let executable = write_fake_myc(
        dir.path(),
        "#!/bin/sh\nif [ \"$1\" != \"status\" ] || [ \"$2\" != \"--view\" ] || [ \"$3\" != \"full\" ]; then\n  echo \"unexpected args: $*\" >&2\n  exit 64\nfi\nexec sleep 5\n",
    );

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "--myc-executable",
            executable.to_str().expect("executable path"),
            "myc",
            "status",
        ])
        .output()
        .expect("run myc status");

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["state"], "unavailable");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("timed out"))
    );
}

fn write_fake_myc(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
    let path = dir.join("fake-myc");
    fs::write(&path, script).expect("write fake myc");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod fake myc");
    path
}

fn myc_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("lock myc integration tests")
}

fn successful_status_script(payload_json: String) -> String {
    format!(
        "#!/bin/sh\nif [ \"$1\" != \"status\" ] || [ \"$2\" != \"--view\" ] || [ \"$3\" != \"full\" ]; then\n  echo \"unexpected args: $*\" >&2\n  exit 64\nfi\ncat <<'JSON'\n{payload_json}\nJSON\n"
    )
}

fn sample_status_payload(ready: bool) -> Value {
    sample_status_payload_with_permissions(ready, &["sign_event"])
}

fn sample_status_payload_with_permissions(ready: bool, permissions: &[&str]) -> Value {
    let signer_identity = RadrootsIdentity::generate().to_public();
    let user_identity = RadrootsIdentity::generate().to_public();
    let session_id = RadrootsNostrSignerConnectionId::new_v7().to_string();
    let signer_id = signer_identity.id.clone();
    let signer_public_key_hex = signer_identity.public_key_hex.clone();
    let user_id = user_identity.id.clone();
    let user_public_key_hex = user_identity.public_key_hex.clone();
    let service_status = if ready { "healthy" } else { "degraded" };
    let reasons = if ready {
        Vec::<String>::new()
    } else {
        vec!["transport quorum is below target".to_owned()]
    };

    json!({
        "status": service_status,
        "ready": ready,
        "reasons": reasons,
        "signer_backend": {
            "local_signer": {
                "account_id": signer_id,
                "public_identity": signer_identity.clone(),
                "availability": "SecretBacked"
            },
            "remote_session_count": 1,
            "remote_sessions": [
                {
                    "connection_id": session_id,
                    "signer_identity": signer_identity,
                    "user_identity": user_identity.clone(),
                    "relays": ["wss://relay.example.test"],
                    "permissions": permissions.join(",")
                }
            ]
        },
        "custody": {
            "signer": {
                "resolved": true,
                "selected_account_id": "signer-account",
                "selected_account_state": "ready",
                "identity_id": signer_id,
                "public_key_hex": signer_public_key_hex
            },
            "user": {
                "resolved": true,
                "selected_account_id": "user-account",
                "selected_account_state": "public_only",
                "identity_id": user_id,
                "public_key_hex": user_public_key_hex
            }
        }
    })
}

fn sample_multi_session_status_payload() -> Value {
    let first_signer = RadrootsIdentity::generate().to_public();
    let first_user = RadrootsIdentity::generate().to_public();
    let second_signer = RadrootsIdentity::generate().to_public();
    let second_user = RadrootsIdentity::generate().to_public();
    let first_signer_id = first_signer.id.clone();
    let first_signer_public_key_hex = first_signer.public_key_hex.clone();
    let first_user_id = first_user.id.clone();
    let first_user_public_key_hex = first_user.public_key_hex.clone();

    json!({
        "status": "healthy",
        "ready": true,
        "reasons": [],
        "signer_backend": {
            "local_signer": {
                "account_id": first_signer_id,
                "public_identity": first_signer.clone(),
                "availability": "SecretBacked"
            },
            "remote_session_count": 2,
            "remote_sessions": [
                {
                    "connection_id": RadrootsNostrSignerConnectionId::new_v7().to_string(),
                    "signer_identity": first_signer,
                    "user_identity": first_user.clone(),
                    "relays": ["wss://relay.example.test"],
                    "permissions": "sign_event"
                },
                {
                    "connection_id": RadrootsNostrSignerConnectionId::new_v7().to_string(),
                    "signer_identity": second_signer,
                    "user_identity": second_user,
                    "relays": ["wss://relay-secondary.example.test"],
                    "permissions": "sign_event"
                }
            ]
        },
        "custody": {
            "signer": {
                "resolved": true,
                "selected_account_id": "signer-account",
                "selected_account_state": "ready",
                "identity_id": first_signer_id,
                "public_key_hex": first_signer_public_key_hex
            },
            "user": {
                "resolved": true,
                "selected_account_id": "user-account",
                "selected_account_state": "public_only",
                "identity_id": first_user_id,
                "public_key_hex": first_user_public_key_hex
            }
        }
    })
}
