use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use assert_cmd::prelude::*;
use radroots_identity::RadrootsIdentity;
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
    assert_eq!(json["ready"], true);
    assert_eq!(json["service_status"], "healthy");
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
            "--signer-backend",
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
    assert_eq!(json["backend"], "myc");
    assert_eq!(json["state"], "degraded");
    assert_eq!(json["myc"]["state"], "degraded");
    assert_eq!(json["myc"]["service_status"], "degraded");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("transport quorum is below target"))
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
    let signer_identity = RadrootsIdentity::generate().to_public();
    let user_identity = RadrootsIdentity::generate().to_public();
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
                "account_id": signer_identity.id,
                "public_identity": signer_identity,
                "availability": "SecretBacked"
            }
        },
        "custody": {
            "signer": {
                "resolved": true,
                "selected_account_id": "signer-account",
                "selected_account_state": "ready",
                "identity_id": signer_identity.id,
                "public_key_hex": signer_identity.public_key_hex
            },
            "user": {
                "resolved": true,
                "selected_account_id": "user-account",
                "selected_account_state": "public_only",
                "identity_id": user_identity.id,
                "public_key_hex": user_identity.public_key_hex
            }
        }
    })
}
