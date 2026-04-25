use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

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
        "RADROOTS_CLI_PATHS_PROFILE",
        "RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT",
        "RADROOTS_LOG_FILTER",
        "RADROOTS_LOG_DIR",
        "RADROOTS_LOG_STDOUT",
        "RADROOTS_ACCOUNT",
        "RADROOTS_ACCOUNT_SECRET_BACKEND",
        "RADROOTS_ACCOUNT_SECRET_FALLBACK",
        "RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE",
        "RADROOTS_HYF_ENABLED",
        "RADROOTS_HYF_EXECUTABLE",
        "RADROOTS_IDENTITY_PATH",
        "RADROOTS_SIGNER",
        "RADROOTS_RELAYS",
        "RADROOTS_MYC_EXECUTABLE",
        "RADROOTS_MYC_STATUS_TIMEOUT_MS",
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command.env("RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE", "false");
    command
}

fn sell_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn bootstrap_seller(workdir: &Path) {
    let account_output = cli_command_in(workdir)
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let farm_output = cli_command_in(workdir)
        .args([
            "--json",
            "farm",
            "setup",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
            "--city",
            "San Francisco",
            "--region",
            "CA",
            "--country",
            "US",
            "--delivery-method",
            "pickup",
        ])
        .output()
        .expect("run farm setup");
    assert!(farm_output.status.success());
}

#[test]
fn sell_add_creates_named_local_draft_from_human_flags() {
    let _guard = sell_test_guard();
    let dir = tempdir().expect("tempdir");
    bootstrap_seller(dir.path());

    let output = cli_command_in(dir.path())
        .args([
            "--json",
            "sell",
            "add",
            "tomatoes",
            "--pack",
            "1 kg",
            "--price",
            "10 USD/kg",
            "--stock",
            "25",
        ])
        .output()
        .expect("run sell add");
    assert!(output.status.success());

    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "draft_saved");
    assert_eq!(json["product_key"], "tomatoes");
    assert_eq!(json["title"], "Tomatoes");
    assert_eq!(json["offer"], "1 kg");
    assert_eq!(json["price"], "10 USD/kg");
    assert_eq!(json["stock"], "25 available");
    assert_eq!(json["farm_name"], "La Huerta");
    assert_eq!(json["delivery_method"], "pickup");
    assert_eq!(json["location_primary"], "San Francisco, CA");
    assert_eq!(
        json["actions"][0],
        "radroots sell check listing-tomatoes.toml"
    );
    assert_eq!(
        json["actions"][1],
        "radroots sell publish listing-tomatoes.toml"
    );

    let draft_path = dir.path().join("listing-tomatoes.toml");
    let contents = fs::read_to_string(&draft_path).expect("draft contents");
    assert!(contents.contains("key = \"tomatoes\""));
    assert!(contents.contains("title = \"Tomatoes\""));
    assert!(contents.contains("category = \"Tomatoes\""));
    assert!(contents.contains("quantity_amount = \"1\""));
    assert!(contents.contains("quantity_unit = \"kg\""));
    assert!(contents.contains("price_amount = \"10\""));
    assert!(contents.contains("price_currency = \"USD\""));
    assert!(contents.contains("price_per_amount = \"1\""));
    assert!(contents.contains("price_per_unit = \"kg\""));
    assert!(contents.contains("available = \"25\""));

    let human_output = cli_command_in(dir.path())
        .args([
            "sell", "add", "potatoes", "--pack", "2 kg", "--price", "6 USD/kg", "--stock", "10",
        ])
        .output()
        .expect("run human sell add");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Listing draft saved"));
    assert!(stdout.contains("The draft is local until you publish it."));
    assert!(stdout.contains("Draft"));
    assert!(stdout.contains("Defaults"));
    assert!(stdout.contains("radroots sell check listing-potatoes.toml"));
}

#[test]
fn sell_show_reads_local_draft_only() {
    let _guard = sell_test_guard();
    let dir = tempdir().expect("tempdir");
    bootstrap_seller(dir.path());

    let add = cli_command_in(dir.path())
        .args([
            "sell",
            "add",
            "tomatoes",
            "--pack",
            "1 kg",
            "--price",
            "10 USD/kg",
            "--stock",
            "25",
        ])
        .output()
        .expect("run sell add");
    assert!(add.status.success());

    let output = cli_command_in(dir.path())
        .args(["--json", "sell", "show", "listing-tomatoes.toml"])
        .output()
        .expect("run sell show");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["product_key"], "tomatoes");
    assert_eq!(json["title"], "Tomatoes");
    assert_eq!(json["category"], "Tomatoes");
    assert_eq!(json["offer"], "1 kg");
    assert_eq!(json["price"], "10 USD/kg");
    assert_eq!(json["stock"], "25 available");
    assert_eq!(json["delivery_method"], "pickup");
    assert_eq!(json["location_primary"], "San Francisco, CA");
    assert_eq!(
        json["actions"][0],
        "radroots sell check listing-tomatoes.toml"
    );

    let human_output = cli_command_in(dir.path())
        .args(["sell", "show", "listing-tomatoes.toml"])
        .output()
        .expect("run human sell show");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Listing draft"));
    assert!(stdout.contains("tomatoes"));
    assert!(stdout.contains("San Francisco, CA"));
    assert!(!stdout.contains("listing ·"));
}

#[test]
fn sell_check_reports_ready_and_invalid_drafts() {
    let _guard = sell_test_guard();
    let dir = tempdir().expect("tempdir");
    bootstrap_seller(dir.path());

    let add = cli_command_in(dir.path())
        .args([
            "sell",
            "add",
            "tomatoes",
            "--pack",
            "1 kg",
            "--price",
            "10 USD/kg",
            "--stock",
            "25",
        ])
        .output()
        .expect("run sell add");
    assert!(add.status.success());

    let ready_output = cli_command_in(dir.path())
        .args(["--json", "sell", "check", "listing-tomatoes.toml"])
        .output()
        .expect("run ready sell check");
    assert!(ready_output.status.success());
    let ready_json: Value =
        serde_json::from_slice(ready_output.stdout.as_slice()).expect("ready json");
    assert_eq!(ready_json["state"], "ready");
    assert_eq!(ready_json["valid"], true);
    assert_eq!(ready_json["product_key"], "tomatoes");
    assert_eq!(
        ready_json["actions"][0],
        "radroots sell publish listing-tomatoes.toml"
    );

    let draft_path = dir.path().join("listing-tomatoes.toml");
    let broken = fs::read_to_string(&draft_path)
        .expect("draft contents")
        .replace("price_amount = \"10\"", "price_amount = \"\"");
    fs::write(&draft_path, broken).expect("write broken draft");

    let invalid_output = cli_command_in(dir.path())
        .args(["sell", "check", "listing-tomatoes.toml"])
        .output()
        .expect("run invalid sell check");
    assert!(invalid_output.status.success());
    let stdout = String::from_utf8(invalid_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Draft needs changes"));
    assert!(stdout.contains("primary_bin.price_amount"));
    assert!(stdout.contains("radroots sell show listing-tomatoes.toml"));
    assert!(stdout.contains("Edit the draft file and run the command again"));
}

#[test]
fn sell_reprice_and_restock_mutate_draft_file() {
    let _guard = sell_test_guard();
    let dir = tempdir().expect("tempdir");
    bootstrap_seller(dir.path());

    let add = cli_command_in(dir.path())
        .args([
            "sell",
            "add",
            "tomatoes",
            "--pack",
            "1 kg",
            "--price",
            "10 USD/kg",
            "--stock",
            "25",
        ])
        .output()
        .expect("run sell add");
    assert!(add.status.success());

    let reprice_output = cli_command_in(dir.path())
        .args([
            "--json",
            "sell",
            "reprice",
            "listing-tomatoes.toml",
            "12 USD/kg",
        ])
        .output()
        .expect("run sell reprice");
    assert!(reprice_output.status.success());
    let reprice_json: Value =
        serde_json::from_slice(reprice_output.stdout.as_slice()).expect("reprice json");
    assert_eq!(reprice_json["state"], "updated");
    assert_eq!(reprice_json["operation"], "reprice");
    assert_eq!(reprice_json["changed_label"], "Price");
    assert_eq!(reprice_json["changed_value"], "12 USD/kg");

    let restock_output = cli_command_in(dir.path())
        .args(["--json", "sell", "restock", "listing-tomatoes.toml", "40"])
        .output()
        .expect("run sell restock");
    assert!(restock_output.status.success());
    let restock_json: Value =
        serde_json::from_slice(restock_output.stdout.as_slice()).expect("restock json");
    assert_eq!(restock_json["state"], "updated");
    assert_eq!(restock_json["operation"], "restock");
    assert_eq!(restock_json["changed_label"], "Stock");
    assert_eq!(restock_json["changed_value"], "40 available");

    let contents = fs::read_to_string(dir.path().join("listing-tomatoes.toml")).expect("draft");
    assert!(contents.contains("price_amount = \"12\""));
    assert!(contents.contains("available = \"40\""));
}

#[test]
fn sell_publish_update_and_pause_wrap_listing_dry_runs() {
    let _guard = sell_test_guard();
    let dir = tempdir().expect("tempdir");
    bootstrap_seller(dir.path());

    let add = cli_command_in(dir.path())
        .args([
            "sell",
            "add",
            "tomatoes",
            "--pack",
            "1 kg",
            "--price",
            "10 USD/kg",
            "--stock",
            "25",
        ])
        .output()
        .expect("run sell add");
    assert!(add.status.success());

    let publish_output = cli_command_in(dir.path())
        .args([
            "--dry-run",
            "--json",
            "sell",
            "publish",
            "listing-tomatoes.toml",
        ])
        .output()
        .expect("run sell publish dry run");
    assert!(publish_output.status.success());
    let publish_json: Value =
        serde_json::from_slice(publish_output.stdout.as_slice()).expect("publish json");
    assert_eq!(publish_json["state"], "dry_run");
    assert_eq!(publish_json["operation"], "publish");
    assert_eq!(publish_json["product_key"], "tomatoes");
    assert_eq!(
        publish_json["actions"][0],
        "radroots sell publish listing-tomatoes.toml"
    );

    let update_output = cli_command_in(dir.path())
        .args([
            "--dry-run",
            "--json",
            "sell",
            "update",
            "listing-tomatoes.toml",
        ])
        .output()
        .expect("run sell update dry run");
    assert!(update_output.status.success());
    let update_json: Value =
        serde_json::from_slice(update_output.stdout.as_slice()).expect("update json");
    assert_eq!(update_json["state"], "dry_run");
    assert_eq!(update_json["operation"], "update");
    assert_eq!(update_json["product_key"], "tomatoes");
    assert_eq!(
        update_json["actions"][0],
        "radroots sell update listing-tomatoes.toml"
    );

    let pause_output = cli_command_in(dir.path())
        .args([
            "--dry-run",
            "--json",
            "sell",
            "pause",
            "listing-tomatoes.toml",
        ])
        .output()
        .expect("run sell pause dry run");
    assert!(pause_output.status.success());
    let pause_json: Value =
        serde_json::from_slice(pause_output.stdout.as_slice()).expect("pause json");
    assert_eq!(pause_json["state"], "dry_run");
    assert_eq!(pause_json["operation"], "pause");
    assert_eq!(pause_json["product_key"], "tomatoes");
    assert_eq!(
        pause_json["actions"][0],
        "radroots sell pause listing-tomatoes.toml"
    );

    let human_output = cli_command_in(dir.path())
        .args(["--dry-run", "sell", "publish", "listing-tomatoes.toml"])
        .output()
        .expect("run human sell publish dry run");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Dry run only"));
    assert!(stdout.contains("Listing would be published."));
    assert!(stdout.contains("Nothing was written."));
}
