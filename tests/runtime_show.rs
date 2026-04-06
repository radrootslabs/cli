use std::process::Command;

use assert_cmd::prelude::*;
use serde_json::Value;

#[test]
fn runtime_show_json_reports_default_bootstrap_state() {
    let output = Command::cargo_bin("radroots")
        .expect("binary")
        .args(["--json", "runtime", "show"])
        .output()
        .expect("run runtime show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["output_format"], "json");
    assert_eq!(json["logging"]["initialized"], true);
    assert_eq!(json["logging"]["stdout"], false);
    assert_eq!(json["logging"]["directory"], Value::Null);
    assert_eq!(json["identity"]["path"], "identity.json");
    assert_eq!(json["signer"]["backend"], "local");
    assert_eq!(json["myc"]["executable"], "myc");
}

#[test]
fn runtime_show_json_reflects_environment_configuration() {
    let output = Command::cargo_bin("radroots")
        .expect("binary")
        .env("RADROOTS_OUTPUT", "json")
        .env("RADROOTS_LOG_FILTER", "debug")
        .env("RADROOTS_LOG_DIR", "logs/runtime")
        .env("RADROOTS_LOG_STDOUT", "false")
        .env("RADROOTS_IDENTITY_PATH", "state/identity.json")
        .env("RADROOTS_SIGNER_BACKEND", "myc")
        .env("RADROOTS_MYC_EXECUTABLE", "bin/myc")
        .args(["runtime", "show"])
        .output()
        .expect("run runtime show");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let json: Value = serde_json::from_str(stdout.as_str()).expect("json output");
    assert_eq!(json["logging"]["filter"], "debug");
    assert_eq!(json["logging"]["directory"], "logs/runtime");
    assert_eq!(json["identity"]["path"], "state/identity.json");
    assert_eq!(json["signer"]["backend"], "myc");
    assert_eq!(json["myc"]["executable"], "bin/myc");
}
