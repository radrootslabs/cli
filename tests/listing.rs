use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use radroots_sql_core::{SqlExecutor, SqliteExecutor};
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
        "RADROOTS_SIGNER",
        "RADROOTS_RELAYS",
        "RADROOTS_MYC_EXECUTABLE",
    ] {
        command.env_remove(key);
    }
    command
}

#[test]
fn listing_new_scaffolds_a_toml_draft_with_account_and_farm_defaults() {
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
    let account_id = account_json["account"]["id"]
        .as_str()
        .expect("account id");
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
    let farm_d_tag = "AAAAAAAAAAAAAAAAAAAAAw";
    seed_farm(dir.path(), seller_pubkey, farm_d_tag, "La Huerta");

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

fn seed_farm(workdir: &Path, pubkey: &str, d_tag: &str, name: &str) {
    let replica_db = workdir
        .join("home")
        .join(".local/share/radroots/replica/replica.sqlite");
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
    let replica_db = workdir
        .join("home")
        .join(".local/share/radroots/replica/replica.sqlite");
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
