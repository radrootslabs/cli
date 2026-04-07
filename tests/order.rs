use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

fn order_command_in(workdir: &Path) -> Command {
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
        "RADROOTS_SIGNER",
        "RADROOTS_RELAYS",
        "RADROOTS_MYC_EXECUTABLE",
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command
}

fn order_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("order test lock")
}

#[test]
fn order_new_creates_a_local_draft_with_selected_account_defaults() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let account_id = account_json["account"]["id"].as_str().expect("account id");
    let buyer_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("buyer pubkey");

    let output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
            "--qty",
            "2",
        ])
        .output()
        .expect("run order new");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("order json");
    assert_eq!(json["state"], "draft_created");
    assert_eq!(json["buyer_account_id"], account_id);
    assert_eq!(json["buyer_pubkey"], buyer_pubkey);
    assert_eq!(
        json["seller_pubkey"],
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    );
    assert_eq!(json["ready_for_submit"], true);
    assert_eq!(json["items"][0]["bin_id"], "bin-1");
    assert_eq!(json["items"][0]["bin_count"], 2);

    let file = json["file"].as_str().expect("draft file");
    assert!(file.contains(".local/share/radroots/orders/drafts/ord_"));
    let contents = fs::read_to_string(file).expect("read order draft");
    assert!(contents.contains("kind = \"order_draft_v1\""));
    assert!(contents.contains("listing_lookup = \"pasture-eggs\""));
    assert!(contents.contains(&format!("buyer_account_id = \"{account_id}\"")));
}

#[test]
fn order_get_and_ls_read_local_drafts_and_report_missing() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let first = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run first order new");
    assert!(first.status.success());
    let first_json: Value = serde_json::from_slice(first.stdout.as_slice()).expect("first json");
    let first_order_id = first_json["order_id"].as_str().expect("first order id");

    let second = order_command_in(dir.path())
        .args(["--json", "order", "new", "--listing", "carrots"])
        .output()
        .expect("run second order new");
    assert!(second.status.success());
    let second_json: Value = serde_json::from_slice(second.stdout.as_slice()).expect("second json");
    let second_order_id = second_json["order_id"].as_str().expect("second order id");

    let get_output = order_command_in(dir.path())
        .args(["--json", "order", "get", first_order_id])
        .output()
        .expect("run order get");
    assert!(get_output.status.success());
    let get_json: Value = serde_json::from_slice(get_output.stdout.as_slice()).expect("get json");
    assert_eq!(get_json["state"], "ready");
    assert_eq!(get_json["order_id"], first_order_id);
    assert_eq!(get_json["listing_lookup"], "pasture-eggs");

    let missing_output = order_command_in(dir.path())
        .args(["--json", "order", "get", "ord_missing"])
        .output()
        .expect("run missing order get");
    assert!(missing_output.status.success());
    let missing_json: Value =
        serde_json::from_slice(missing_output.stdout.as_slice()).expect("missing json");
    assert_eq!(missing_json["state"], "missing");

    let human_list = order_command_in(dir.path())
        .args(["order", "ls"])
        .output()
        .expect("run human order ls");
    assert!(human_list.status.success());
    let human_text = String::from_utf8(human_list.stdout).expect("human text");
    assert!(human_text.contains("orders · 2 local drafts"));
    assert!(human_text.contains(first_order_id));
    assert!(human_text.contains(second_order_id));

    let ndjson_output = order_command_in(dir.path())
        .args(["--ndjson", "order", "ls"])
        .output()
        .expect("run ndjson order ls");
    assert!(ndjson_output.status.success());
    let ndjson = String::from_utf8(ndjson_output.stdout).expect("ndjson text");
    let lines = ndjson.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|line| line.contains(first_order_id)));
    assert!(lines.iter().any(|line| line.contains(second_order_id)));
}

#[test]
fn order_get_surfaces_recorded_job_metadata_from_the_local_draft_store() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let drafts_dir = dir.path().join("home/.local/share/radroots/orders/drafts");
    fs::create_dir_all(&drafts_dir).expect("create drafts dir");
    let draft_path = drafts_dir.join("ord_AAAAAAAAAAAAAAAAAAAAAg.toml");
    fs::write(
        &draft_path,
        r#"version = 1
kind = "order_draft_v1"
listing_lookup = "fresh-eggs"
buyer_account_id = "acct_demo"

[order]
order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg"
listing_addr = "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg"
buyer_pubkey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
seller_pubkey = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"

[[order.items]]
bin_id = "bin-1"
bin_count = 2

[submission]
job_id = "job_order_01"
"#,
    )
    .expect("write order draft");

    let output = order_command_in(dir.path())
        .args(["--json", "order", "get", "ord_AAAAAAAAAAAAAAAAAAAAAg"])
        .output()
        .expect("run order get");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "submitted");
    assert_eq!(json["job"]["job_id"], "job_order_01");
    assert_eq!(json["job"]["state"], "recorded");
    assert_eq!(json["ready_for_submit"], false);
}
