use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use assert_cmd::prelude::*;
use radroots_sql_core::{SqlExecutor, SqliteExecutor};
use serde_json::{Value, json};
use tempfile::tempdir;

fn data_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        workdir.join("local").join("Radroots").join("data")
    } else {
        workdir.join("home").join(".radroots").join("data")
    }
}

fn cli_command_in(workdir: &Path) -> Command {
    let mut command = Command::cargo_bin("radroots").expect("binary");
    command.current_dir(workdir);
    command.env("HOME", workdir.join("home"));
    command.env("APPDATA", workdir.join("roaming"));
    command.env("LOCALAPPDATA", workdir.join("local"));
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

fn listing_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("listing test lock")
}

#[test]
fn listing_new_scaffolds_a_toml_draft_with_account_and_farm_defaults() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let seller_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("seller pubkey");
    let account_id = account_json["account"]["id"].as_str().expect("account id");
    let farm_d_tag = "AAAAAAAAAAAAAAAAAAAAAw";
    seed_farm(dir.path(), seller_pubkey, farm_d_tag, "La Huerta");

    let output = cli_command_in(dir.path())
        .args(["--json", "listing", "new"])
        .output()
        .expect("run listing new");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "draft created");
    assert_eq!(json["selected_account_id"], account_id);
    assert_eq!(json["seller_pubkey"], seller_pubkey);
    assert_eq!(json["farm_d_tag"], farm_d_tag);
    let file = json["file"].as_str().expect("draft file");
    let contents = fs::read_to_string(file).expect("draft contents");
    assert!(contents.contains("kind = \"listing_draft_v1\""));
    assert!(contents.contains(&format!("seller_pubkey = \"{seller_pubkey}\"")));
    assert!(contents.contains(&format!("farm_d_tag = \"{farm_d_tag}\"")));
}

#[test]
fn listing_validate_resolves_selected_account_and_matching_farm() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let seller_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("seller pubkey")
        .to_owned();
    let farm_d_tag = "AAAAAAAAAAAAAAAAAAAAAw";
    seed_farm(dir.path(), seller_pubkey.as_str(), farm_d_tag, "La Huerta");

    let draft_path = dir.path().join("eggs.toml");
    fs::write(
        &draft_path,
        valid_listing_draft(
            "AAAAAAAAAAAAAAAAAAAAAg",
            "",
            "",
            "eggs",
            "Pasture eggs",
            "Protein",
            "Fresh pasture-raised eggs collected daily.",
            "12",
            "each",
            "4.50",
            "USD",
            "1",
            "each",
            "18",
            "pickup",
            "La Huerta del Sur",
        ),
    )
    .expect("write listing draft");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "listing",
            "validate",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing validate");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "valid");
    assert_eq!(json["valid"], true);
    assert_eq!(json["seller_pubkey"], seller_pubkey);
    assert_eq!(json["farm_d_tag"], farm_d_tag);
}

#[test]
fn listing_validate_reports_invalid_drafts_with_field_lines() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let draft_path = dir.path().join("invalid.toml");
    fs::write(
        &draft_path,
        valid_listing_draft(
            "AAAAAAAAAAAAAAAAAAAAAg",
            "AAAAAAAAAAAAAAAAAAAAAw",
            &"b".repeat(64),
            "eggs",
            "Pasture eggs",
            "Protein",
            "Fresh pasture-raised eggs collected daily.",
            "12",
            "each",
            "oops",
            "USD",
            "1",
            "each",
            "18",
            "pickup",
            "La Huerta del Sur",
        ),
    )
    .expect("write invalid draft");

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "listing",
            "validate",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing validate");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "invalid");
    assert_eq!(json["valid"], false);
    assert_eq!(json["issues"][0]["field"], "primary_bin.price_amount");
    assert!(json["issues"][0]["line"].as_u64().is_some());
}

#[test]
fn listing_get_reads_real_local_rows_and_reports_missing() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000301",
        "pasture-eggs",
        "protein",
        "Pasture Eggs",
        "Fresh pasture-raised eggs collected daily.",
        36,
        18,
        Some("Marshall"),
    );

    let json_output = cli_command_in(dir.path())
        .args(["--json", "listing", "get", "pasture-eggs"])
        .output()
        .expect("run listing get");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["product_key"], "pasture-eggs");
    assert_eq!(json["title"], "Pasture Eggs");
    assert_eq!(json["location_primary"], "Marshall");
    assert_eq!(json["provenance"]["origin"], "local_replica.trade_product");

    let human_output = cli_command_in(dir.path())
        .args(["listing", "get", "pasture-eggs"])
        .output()
        .expect("run human listing get");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("listing ·"));
    assert!(stdout.contains("Pasture Eggs"));
    assert!(stdout.contains("provenance: local replica"));

    let missing_output = cli_command_in(dir.path())
        .args(["--json", "listing", "get", "missing-listing"])
        .output()
        .expect("run missing listing get");
    assert!(missing_output.status.success());
    let missing_json: Value =
        serde_json::from_slice(missing_output.stdout.as_slice()).expect("json");
    assert_eq!(missing_json["state"], "missing");
}

#[test]
fn listing_publish_and_update_use_durable_bridge_publish() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let seller_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("seller pubkey")
        .to_owned();
    let farm_d_tag = "AAAAAAAAAAAAAAAAAAAAAw";
    seed_farm(dir.path(), seller_pubkey.as_str(), farm_d_tag, "La Huerta");

    let draft_path = dir.path().join("eggs.toml");
    fs::write(
        &draft_path,
        valid_listing_draft(
            "AAAAAAAAAAAAAAAAAAAAAg",
            "",
            "",
            "eggs",
            "Pasture eggs",
            "Protein",
            "Fresh pasture-raised eggs collected daily.",
            "12",
            "each",
            "4.50",
            "USD",
            "1",
            "each",
            "18",
            "pickup",
            "La Huerta del Sur",
        ),
    )
    .expect("write listing draft");

    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, auth_header| {
        recorded.lock().expect("recorded").push(body.clone());
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => {
                assert_eq!(auth_header, None);
                MockRpcResponse::success(json!([sample_session(
                    "sess_publish_01",
                    seller_pubkey.as_str(),
                    &["sign_event"],
                    true
                )]))
            }
            "bridge.listing.publish" => {
                assert_eq!(auth_header.as_deref(), Some("Bearer bridge-secret"));
                MockRpcResponse::success(json!({
                    "deduplicated": false,
                    "job": sample_listing_job(
                        "job_listing_01",
                        "published",
                        "event_listing_01",
                        "30402:deadbeef:AAAAAAAAAAAAAAAAAAAAAg",
                        "sess_publish_01"
                    )
                }))
            }
            other => MockRpcResponse::rpc_error(-32601, &format!("unexpected method: {other}")),
        }
    });

    let publish_output = cli_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge-secret")
        .args([
            "--json",
            "listing",
            "publish",
            "--idempotency-key",
            "publish-key",
            "--signer-session-id",
            "sess_publish_01",
            "--print-job",
            "--print-event",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing publish");
    assert!(publish_output.status.success());
    let publish_json: Value =
        serde_json::from_slice(publish_output.stdout.as_slice()).expect("publish json");
    assert_eq!(publish_json["operation"], "publish");
    assert_eq!(publish_json["job_id"], "job_listing_01");
    assert_eq!(publish_json["job_status"], "published");
    assert_eq!(publish_json["event_id"], "event_listing_01");
    assert_eq!(publish_json["event"]["kind"], 30402);
    assert_eq!(publish_json["signer_mode"], "nip46_session");
    assert_eq!(publish_json["signer_session_id"], "sess_publish_01");
    assert_eq!(publish_json["job"]["rpc_method"], "bridge.listing.publish");
    assert_eq!(publish_json["job"]["signer_mode"], "nip46_session");
    assert_eq!(publish_json["job"]["signer_session_id"], "sess_publish_01");
    assert_eq!(
        publish_json["requested_signer_session_id"],
        "sess_publish_01"
    );
    assert_eq!(
        publish_json["job"]["requested_signer_session_id"],
        "sess_publish_01"
    );

    let update_output = cli_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge-secret")
        .args([
            "--json",
            "listing",
            "update",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing update");
    assert!(update_output.status.success());
    let update_json: Value =
        serde_json::from_slice(update_output.stdout.as_slice()).expect("update json");
    assert_eq!(update_json["operation"], "update");

    let recorded = requests.lock().expect("requests");
    assert_eq!(recorded.len(), 4);
    assert_eq!(recorded[0]["method"], "nip46.session.list");
    assert_eq!(recorded[1]["params"]["kind"], 30402);
    assert_eq!(recorded[1]["params"]["idempotency_key"], "publish-key");
    assert_eq!(
        recorded[1]["params"]["signer_session_id"],
        "sess_publish_01"
    );
    assert_eq!(recorded[2]["method"], "nip46.session.list");
    assert_eq!(recorded[3]["params"]["kind"], 30402);
    assert_eq!(
        recorded[3]["params"]["signer_session_id"],
        "sess_publish_01"
    );
}

#[test]
fn listing_archive_and_dry_run_are_truthful() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let seller_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("seller pubkey")
        .to_owned();
    seed_farm(
        dir.path(),
        seller_pubkey.as_str(),
        "AAAAAAAAAAAAAAAAAAAAAw",
        "La Huerta",
    );

    let draft_path = dir.path().join("archive.toml");
    fs::write(
        &draft_path,
        valid_listing_draft(
            "AAAAAAAAAAAAAAAAAAAAAg",
            "",
            "",
            "eggs",
            "Pasture eggs",
            "Protein",
            "Fresh pasture-raised eggs collected daily.",
            "12",
            "each",
            "4.50",
            "USD",
            "1",
            "each",
            "18",
            "pickup",
            "La Huerta del Sur",
        ),
    )
    .expect("write listing draft");

    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, _auth_header| {
        recorded.lock().expect("recorded").push(body.to_string());
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_archive_01",
                seller_pubkey.as_str(),
                &["sign_event"],
                true
            )])),
            "bridge.listing.publish" => MockRpcResponse::success(json!({
                "deduplicated": false,
                "job": sample_listing_job(
                    "job_listing_archive",
                    "published",
                    "event_listing_archive",
                    "30402:deadbeef:AAAAAAAAAAAAAAAAAAAAAg",
                    "sess_archive_01"
                )
            })),
            other => MockRpcResponse::rpc_error(-32601, &format!("unexpected method: {other}")),
        }
    });

    let archive_output = cli_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge-secret")
        .args([
            "--json",
            "listing",
            "archive",
            "--signer-session-id",
            "sess_archive_01",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing archive");
    assert!(archive_output.status.success());
    let archive_json: Value =
        serde_json::from_slice(archive_output.stdout.as_slice()).expect("archive json");
    assert_eq!(archive_json["operation"], "archive");
    assert_eq!(archive_json["job_status"], "published");
    assert_eq!(archive_json["signer_mode"], "nip46_session");
    assert_eq!(archive_json["signer_session_id"], "sess_archive_01");
    assert_eq!(
        archive_json["requested_signer_session_id"],
        "sess_archive_01"
    );

    let dry_run_output = cli_command_in(dir.path())
        .args([
            "--json",
            "--dry-run",
            "listing",
            "publish",
            "--signer-session-id",
            "sess_dry_run_01",
            "--print-event",
            "--print-job",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing publish dry run");
    assert!(dry_run_output.status.success());
    let dry_run_json: Value =
        serde_json::from_slice(dry_run_output.stdout.as_slice()).expect("dry run json");
    assert_eq!(dry_run_json["state"], "dry_run");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["job"]["state"], "not_submitted");
    assert!(dry_run_json["signer_mode"].is_null());
    assert!(dry_run_json["signer_session_id"].is_null());
    assert_eq!(dry_run_json["job"]["signer_mode"], "local");
    assert!(dry_run_json["job"]["signer_session_id"].is_null());
    assert_eq!(dry_run_json["event"]["kind"], 30402);
    assert!(dry_run_json["event"]["event_id"].is_null());
    assert_eq!(
        dry_run_json["requested_signer_session_id"],
        "sess_dry_run_01"
    );
    assert_eq!(
        dry_run_json["job"]["requested_signer_session_id"],
        "sess_dry_run_01"
    );

    let recorded = requests.lock().expect("requests");
    assert_eq!(recorded.len(), 2);
    assert!(recorded[1].contains("archived"));
}

#[test]
fn listing_publish_without_matching_signer_session_exits_unconfigured() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let seller_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("seller pubkey")
        .to_owned();
    seed_farm(
        dir.path(),
        seller_pubkey.as_str(),
        "AAAAAAAAAAAAAAAAAAAAAw",
        "La Huerta",
    );

    let draft_path = dir.path().join("no-session.toml");
    fs::write(
        &draft_path,
        valid_listing_draft(
            "AAAAAAAAAAAAAAAAAAAAAg",
            "",
            "",
            "eggs",
            "Pasture eggs",
            "Protein",
            "Fresh pasture-raised eggs collected daily.",
            "12",
            "each",
            "4.50",
            "USD",
            "1",
            "each",
            "18",
            "pickup",
            "La Huerta del Sur",
        ),
    )
    .expect("write listing draft");

    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, _auth_header| {
        recorded.lock().expect("recorded").push(body.clone());
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_other_01",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &["sign_event"],
                true
            )])),
            other => MockRpcResponse::rpc_error(-32601, &format!("unexpected method: {other}")),
        }
    });

    let publish_output = cli_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge-secret")
        .args([
            "--json",
            "listing",
            "publish",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing publish");
    assert_eq!(publish_output.status.code(), Some(3));
    let publish_json: Value =
        serde_json::from_slice(publish_output.stdout.as_slice()).expect("publish json");
    assert_eq!(publish_json["state"], "unconfigured");
    assert!(
        publish_json["reason"]
            .as_str()
            .expect("reason")
            .contains("no authorized signer session matched seller pubkey")
    );

    let recorded = requests.lock().expect("requests");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["method"], "nip46.session.list");
}

#[test]
fn listing_publish_rejects_requested_session_that_mismatches_seller_pubkey() {
    let _guard = listing_test_guard();
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let account_output = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let seller_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("seller pubkey")
        .to_owned();
    seed_farm(
        dir.path(),
        seller_pubkey.as_str(),
        "AAAAAAAAAAAAAAAAAAAAAw",
        "La Huerta",
    );

    let draft_path = dir.path().join("mismatch-session.toml");
    fs::write(
        &draft_path,
        valid_listing_draft(
            "AAAAAAAAAAAAAAAAAAAAAg",
            "",
            "",
            "eggs",
            "Pasture eggs",
            "Protein",
            "Fresh pasture-raised eggs collected daily.",
            "12",
            "each",
            "4.50",
            "USD",
            "1",
            "each",
            "18",
            "pickup",
            "La Huerta del Sur",
        ),
    )
    .expect("write listing draft");

    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, _auth_header| {
        recorded.lock().expect("recorded").push(body.clone());
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_wrong_01",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &["sign_event"],
                true
            )])),
            other => MockRpcResponse::rpc_error(-32601, &format!("unexpected method: {other}")),
        }
    });

    let publish_output = cli_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge-secret")
        .args([
            "--json",
            "listing",
            "publish",
            "--signer-session-id",
            "sess_wrong_01",
            draft_path.to_str().expect("draft path"),
        ])
        .output()
        .expect("run listing publish");
    assert_eq!(publish_output.status.code(), Some(3));
    let publish_json: Value =
        serde_json::from_slice(publish_output.stdout.as_slice()).expect("publish json");
    assert_eq!(publish_json["state"], "unconfigured");
    assert!(
        publish_json["reason"]
            .as_str()
            .expect("reason")
            .contains("does not match seller pubkey")
    );

    let recorded = requests.lock().expect("requests");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["method"], "nip46.session.list");
}

fn seed_farm(workdir: &Path, pubkey: &str, d_tag: &str, name: &str) {
    let replica_db = data_root(workdir).join("apps/cli/replica/replica.sqlite");
    let executor = SqliteExecutor::open(&replica_db).expect("open replica db");
    let now = "2026-04-07T00:00:00.000Z";
    executor
        .exec(
            "INSERT INTO farm (id, created_at, updated_at, d_tag, pubkey, name, about, website, picture, banner, location_primary, location_city, location_region, location_country) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
            json!([
                "11111111-1111-1111-1111-111111111111",
                now,
                now,
                d_tag,
                pubkey,
                name,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null
            ])
            .to_string()
            .as_str(),
        )
        .expect("insert farm");
}

#[derive(Debug, Clone)]
struct MockRpcRequest {
    body: Value,
    auth_header: Option<String>,
}

#[derive(Debug, Clone)]
struct MockRpcResponse {
    body: Value,
}

impl MockRpcResponse {
    fn success(result: Value) -> Self {
        Self {
            body: json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result,
            }),
        }
    }

    fn rpc_error(code: i64, message: &str) -> Self {
        Self {
            body: json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {
                    "code": code,
                    "message": message,
                }
            }),
        }
    }
}

struct MockRpcServer {
    address: String,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockRpcServer {
    fn start<F>(handler: F) -> Self
    where
        F: Fn(Value, Option<String>) -> MockRpcResponse + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc listener");
        listener
            .set_nonblocking(true)
            .expect("set listener nonblocking");
        let address = listener.local_addr().expect("local addr").to_string();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let handler: Arc<dyn Fn(Value, Option<String>) -> MockRpcResponse + Send + Sync> =
            Arc::new(handler);
        let handle = thread::spawn(move || {
            while !shutdown_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Ok(request) = read_request(&mut stream) {
                            let response =
                                handler(request.body.clone(), request.auth_header.clone());
                            let _ = write_response(&mut stream, &response);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            shutdown,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for MockRpcServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.address);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join mock rpc server");
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<MockRpcRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("set mock rpc read timeout: {error}"))?;
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read mock rpc request: {error}"))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_subslice(&buffer, b"\r\n\r\n").map(|index| index + 4);
            if let Some(end) = header_end {
                content_length = parse_content_length(&buffer[..end])?;
                if buffer.len() >= end + content_length {
                    break;
                }
            }
        } else if let Some(end) = header_end {
            if buffer.len() >= end + content_length {
                break;
            }
        }
    }

    let end = header_end.ok_or_else(|| "mock rpc request missing headers".to_owned())?;
    let headers = std::str::from_utf8(&buffer[..end])
        .map_err(|error| format!("mock rpc headers not utf-8: {error}"))?;
    let auth_header = parse_header(headers, "authorization");
    let body = std::str::from_utf8(&buffer[end..end + content_length])
        .map_err(|error| format!("mock rpc body not utf-8: {error}"))?;
    let json: Value =
        serde_json::from_str(body).map_err(|error| format!("parse mock rpc body: {error}"))?;

    Ok(MockRpcRequest {
        body: json,
        auth_header,
    })
}

fn parse_content_length(headers: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(headers)
        .map_err(|error| format!("header utf-8 parse failed: {error}"))?;
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| format!("content-length parse failed: {error}"));
            }
        }
    }
    Ok(0)
}

fn parse_header(headers: &str, wanted: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case(wanted) {
            Some(value.trim().to_owned())
        } else {
            None
        }
    })
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_response(stream: &mut TcpStream, response: &MockRpcResponse) -> Result<(), String> {
    let body = response.body.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .map_err(|error| format!("write mock rpc response: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("flush mock rpc response: {error}"))
}

fn sample_listing_job(
    job_id: &str,
    status: &str,
    event_id: &str,
    event_addr: &str,
    signer_session_id: &str,
) -> Value {
    json!({
        "job_id": job_id,
        "command": "bridge.listing.publish",
        "idempotency_key": "publish-key",
        "status": status,
        "terminal": status != "accepted",
        "recovered_after_restart": false,
        "requested_at_unix": 1_712_720_000,
        "completed_at_unix": 1_712_720_010,
        "signer_mode": "nip46_session",
        "signer_session_id": signer_session_id,
        "event_kind": 30402,
        "event_id": event_id,
        "event_addr": event_addr,
        "delivery_policy": "best_effort",
        "delivery_quorum": 2,
        "relay_count": 2,
        "acknowledged_relay_count": 2,
        "required_acknowledged_relay_count": 2,
        "attempt_count": 1,
        "attempt_summaries": ["attempt 1: relay.one accepted"],
        "relay_results": [],
        "relay_outcome_summary": "published to 2 relays"
    })
}

fn sample_session(
    session_id: &str,
    signer_pubkey: &str,
    permissions: &[&str],
    authorized: bool,
) -> Value {
    json!({
        "session_id": session_id,
        "role": "remote_signer",
        "client_pubkey": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "signer_pubkey": signer_pubkey,
        "user_pubkey": Value::Null,
        "relays": ["wss://relay.one"],
        "permissions": permissions,
        "auth_required": false,
        "authorized": authorized,
        "expires_in_secs": Value::Null
    })
}

fn seed_trade_product(
    workdir: &Path,
    product_id: &str,
    key: &str,
    category: &str,
    title: &str,
    summary: &str,
    qty_amt: i64,
    qty_avail: i64,
    location_label: Option<&str>,
) {
    let replica_db = data_root(workdir).join("apps/cli/replica/replica.sqlite");
    let executor = SqliteExecutor::open(&replica_db).expect("open replica db");
    let now = "2026-04-07T00:00:00.000Z";
    executor
        .exec(
            "INSERT INTO trade_product (id, created_at, updated_at, key, category, title, summary, process, lot, profile, year, qty_amt, qty_unit, qty_label, qty_avail, price_amt, price_currency, price_qty_amt, price_qty_unit, notes) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
            json!([
                product_id,
                now,
                now,
                key,
                category,
                title,
                summary,
                "fresh",
                "lot-a",
                "standard",
                2026,
                qty_amt,
                "each",
                "dozen",
                qty_avail,
                4.5,
                "USD",
                1,
                "each",
                Value::Null
            ])
            .to_string()
            .as_str(),
        )
        .expect("insert trade product");

    if let Some(location_label) = location_label {
        let location_id = format!("11111111-1111-1111-1111-{}", &product_id[24..]);
        executor
            .exec(
                "INSERT INTO gcs_location (id, created_at, updated_at, d_tag, lat, lng, geohash, point, polygon, accuracy, altitude, tag_0, label, area, elevation, soil, climate, gc_id, gc_name, gc_admin1_id, gc_admin1_name, gc_country_id, gc_country_name) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
                json!([
                    location_id,
                    now,
                    now,
                    format!("location-{product_id}"),
                    35.0,
                    -82.0,
                    "dnrj",
                    "POINT(-82 35)",
                    "POLYGON EMPTY",
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    location_label,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    location_label,
                    Value::Null,
                    Value::Null,
                    Value::Null,
                    "USA"
                ])
                .to_string()
                .as_str(),
            )
            .expect("insert gcs location");
        executor
            .exec(
                "INSERT INTO trade_product_location (tb_tp, tb_gl) VALUES (?, ?);",
                json!([product_id, location_id]).to_string().as_str(),
            )
            .expect("insert trade product location");
    }
}

fn valid_listing_draft(
    d_tag: &str,
    farm_d_tag: &str,
    seller_pubkey: &str,
    key: &str,
    title: &str,
    category: &str,
    summary: &str,
    quantity_amount: &str,
    quantity_unit: &str,
    price_amount: &str,
    price_currency: &str,
    price_per_amount: &str,
    price_per_unit: &str,
    available: &str,
    delivery_method: &str,
    location_primary: &str,
) -> String {
    format!(
        "version = 1\nkind = \"listing_draft_v1\"\n\n[listing]\nd_tag = \"{d_tag}\"\nfarm_d_tag = \"{farm_d_tag}\"\nseller_pubkey = \"{seller_pubkey}\"\n\n[product]\nkey = \"{key}\"\ntitle = \"{title}\"\ncategory = \"{category}\"\nsummary = \"{summary}\"\n\n[primary_bin]\nbin_id = \"bin-1\"\nquantity_amount = \"{quantity_amount}\"\nquantity_unit = \"{quantity_unit}\"\nprice_amount = \"{price_amount}\"\nprice_currency = \"{price_currency}\"\nprice_per_amount = \"{price_per_amount}\"\nprice_per_unit = \"{price_per_unit}\"\nlabel = \"dozen\"\n\n[inventory]\navailable = \"{available}\"\n\n[availability]\nkind = \"status\"\nstatus = \"active\"\n\n[delivery]\nmethod = \"{delivery_method}\"\n\n[location]\nprimary = \"{location_primary}\"\n"
    )
}
