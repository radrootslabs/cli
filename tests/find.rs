use std::path::Path;
use std::process::Command;

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

#[test]
fn find_reports_unconfigured_when_local_replica_is_missing() {
    let dir = tempdir().expect("tempdir");
    let output = cli_command_in(dir.path())
        .args(["--json", "find", "eggs"])
        .output()
        .expect("run find");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(json["actions"][0], "radroots local init");
}

#[test]
fn find_returns_json_and_ndjson_from_local_market_rows() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000101",
        "heirloom-tomato",
        "produce",
        "Heirloom Tomato",
        "Bright red slicing tomatoes",
        18,
        12,
        Some("Asheville"),
    );
    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000102",
        "tomato-sauce",
        "prepared",
        "Tomato Sauce",
        "Slow cooked tomato sauce",
        8,
        6,
        Some("Black Mountain"),
    );

    let json_output = cli_command_in(dir.path())
        .args(["--json", "find", "tomato"])
        .output()
        .expect("run json find");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["count"], 2);
    assert_eq!(
        json["results"][0]["provenance"]["origin"],
        "local_replica.trade_product"
    );
    assert_eq!(json["results"][0]["location_primary"], "Asheville");

    let ndjson_output = cli_command_in(dir.path())
        .args(["--ndjson", "find", "tomato"])
        .output()
        .expect("run ndjson find");
    assert!(ndjson_output.status.success());
    let stdout = String::from_utf8(ndjson_output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"title\":\"Heirloom Tomato\""));
    assert!(lines[1].contains("\"title\":\"Tomato Sauce\""));
}

#[test]
fn find_human_output_uses_market_table_and_provenance_footer() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000103",
        "fresh-eggs",
        "protein",
        "Fresh Eggs",
        "Pasture-raised eggs",
        36,
        24,
        Some("Marshall"),
    );

    let output = cli_command_in(dir.path())
        .args(["find", "eggs"])
        .output()
        .expect("run human find");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("market · local first · 1 result"));
    assert!(stdout.contains("product"));
    assert!(stdout.contains("Fresh Eggs"));
    assert!(stdout.contains("provenance: local replica"));
}

#[test]
fn find_reports_empty_results_without_failing() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let output = cli_command_in(dir.path())
        .args(["--json", "find", "saffron"])
        .output()
        .expect("run empty find");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "empty");
    assert_eq!(json["count"], 0);
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("no local market results matched"))
    );
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
                "kg",
                "kg",
                qty_avail,
                12.5,
                "USD",
                1,
                "kg",
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
