use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn signer_status_reports_local_ready_when_identity_exists() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("identity.json");

    let init = Command::cargo_bin("radroots")
        .expect("binary")
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "identity",
            "init",
        ])
        .output()
        .expect("run identity init");
    assert!(init.status.success());

    let output = Command::cargo_bin("radroots")
        .expect("binary")
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "--signer-backend",
            "local",
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["backend"], "local");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["reason"], Value::Null);
    assert_eq!(json["local"]["availability"], "secret_backed");
    assert_eq!(json["local"]["secret_backed"], true);
}

#[test]
fn signer_status_reports_local_unconfigured_when_identity_is_missing() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("missing-identity.json");

    let output = Command::cargo_bin("radroots")
        .expect("binary")
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "--signer-backend",
            "local",
            "signer",
            "status",
        ])
        .output()
        .expect("run signer status");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["backend"], "local");
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("local identity file was not found"))
    );
    assert_eq!(json["local"], Value::Null);
}
