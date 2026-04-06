use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn identity_init_json_creates_identity_file() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("identity.json");

    let output = Command::cargo_bin("radroots")
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

    assert!(output.status.success());
    assert!(identity_path.exists());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["path"], identity_path.display().to_string());
    assert_eq!(json["created"], true);
    assert!(json["public_identity"]["id"].is_string());
    assert!(json["public_identity"]["public_key_hex"].is_string());
    assert!(json["public_identity"]["public_key_npub"].is_string());
    assert!(json.get("secret_key").is_none());
}

#[test]
fn identity_show_json_reads_existing_public_identity() {
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
            "identity",
            "show",
        ])
        .output()
        .expect("run identity show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["path"], identity_path.display().to_string());
    assert_eq!(json["state"], "ready");
    assert!(json["public_identity"]["id"].is_string());
    assert!(json["public_identity"]["public_key_hex"].is_string());
    assert!(json["public_identity"]["public_key_npub"].is_string());
    assert!(json.get("secret_key").is_none());
}

#[test]
fn identity_show_json_reports_unconfigured_without_creating_identity() {
    let dir = tempdir().expect("tempdir");
    let identity_path = dir.path().join("missing-identity.json");

    let output = Command::cargo_bin("radroots")
        .expect("binary")
        .args([
            "--json",
            "--identity-path",
            identity_path.to_str().expect("identity path"),
            "--allow-generate-identity",
            "identity",
            "show",
        ])
        .output()
        .expect("run identity show");

    assert_eq!(output.status.code(), Some(3));
    assert!(!identity_path.exists());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["path"], identity_path.display().to_string());
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|value| value.contains("local identity file was not found"))
    );
    assert_eq!(json.get("public_identity"), None);
}
