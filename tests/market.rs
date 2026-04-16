use std::fs;
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
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command.env("RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE", "false");
    command
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
                "1 kg tomato lot",
                qty_avail,
                10.0,
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

fn write_fake_hyfd(
    workdir: &Path,
    status_response: &str,
    rewrite_response: &str,
) -> std::path::PathBuf {
    let path = workdir.join("fake-hyfd");
    let script = format!(
        "#!/bin/sh\nread -r request || exit 64\ncase \"$request\" in\n  *'\"capability\":\"sys.status\"'*)\n    cat <<'JSON'\n{status_response}\nJSON\n    ;;\n  *'\"capability\":\"query_rewrite\"'*)\n    cat <<'JSON'\n{rewrite_response}\nJSON\n    ;;\n  *)\n    cat <<'JSON'\n{{\"version\":1,\"request_id\":\"unexpected\",\"ok\":false,\"error\":{{\"code\":\"unsupported_capability\",\"message\":\"unexpected request\"}}}}\nJSON\n    ;;\nesac\n"
    );
    fs::write(&path, script).expect("write fake hyfd");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod fake hyfd");
    }
    path
}

#[test]
fn market_update_reports_missing_local_data_and_relay_setup() {
    let dir = tempdir().expect("tempdir");
    let output = cli_command_in(dir.path())
        .args(["market", "update"])
        .output()
        .expect("run market update");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Not ready yet"));
    assert!(stdout.contains("Missing"));
    assert!(stdout.contains("Local market data"));
    assert!(stdout.contains("Relay configuration"));
    assert!(stdout.contains("radroots local init"));
    assert!(stdout.contains("radroots relay list --relay wss://relay.example.com"));
}

#[test]
fn market_update_stays_honest_about_unavailable_ingest() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    let config_dir = dir.path().join(".radroots");
    fs::create_dir_all(&config_dir).expect("workspace config dir");
    fs::write(
        config_dir.join("config.toml"),
        "[relay]\nurls = [\"wss://relay.one\"]\npublish_policy = \"any\"\n",
    )
    .expect("write workspace config");

    let json_output = cli_command_in(dir.path())
        .args(["--json", "market", "update"])
        .output()
        .expect("run market update json");
    assert_eq!(json_output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("json");
    assert_eq!(json["direction"], "pull");
    assert_eq!(json["state"], "unavailable");
    assert_eq!(json["relay_count"], 1);
    assert_eq!(json["actions"][0], "radroots rpc status");
    assert_eq!(json["actions"][1], "radroots runtime status radrootsd");
    assert_eq!(json["actions"][2], "radroots sync status");
    assert!(
        json["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("relay ingest"))
    );

    let human_output = cli_command_in(dir.path())
        .args(["market", "update"])
        .output()
        .expect("run market update human");
    assert_eq!(human_output.status.code(), Some(4));
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Unavailable right now"));
    assert!(stdout.contains("relay ingest is not wired into `radroots sync pull` yet"));
    assert!(stdout.contains("Next"));
    assert!(stdout.contains("radroots rpc status"));
}

#[test]
fn market_search_preserves_machine_shape_and_renders_card_list() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000401",
        "sf-tomatoes",
        "produce",
        "San Francisco Early Girl Tomatoes",
        "Fresh local tomatoes packed for pickup from the farm.",
        18,
        12,
        Some("San Francisco, CA"),
    );

    let json_output = cli_command_in(dir.path())
        .args(["--json", "market", "search", "tomatoes"])
        .output()
        .expect("run market search json");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["count"], 1);
    assert_eq!(json["results"][0]["product_key"], "sf-tomatoes");
    assert_eq!(
        json["results"][0]["title"],
        "San Francisco Early Girl Tomatoes"
    );
    assert_eq!(json["results"][0]["location_primary"], "San Francisco, CA");
    assert_eq!(json["actions"][0], "radroots market view sf-tomatoes");
    assert_eq!(
        json["actions"][1],
        "radroots order create --listing sf-tomatoes"
    );

    let ndjson_output = cli_command_in(dir.path())
        .args(["--ndjson", "market", "search", "tomatoes"])
        .output()
        .expect("run market search ndjson");
    assert!(ndjson_output.status.success());
    let stdout = String::from_utf8(ndjson_output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("\"product_key\":\"sf-tomatoes\""));
    assert!(lines[0].contains("\"title\":\"San Francisco Early Girl Tomatoes\""));

    let human_output = cli_command_in(dir.path())
        .args(["market", "search", "tomatoes"])
        .output()
        .expect("run market search human");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("1 listing for tomatoes"));
    assert!(stdout.contains("San Francisco Early Girl Tomatoes"));
    assert!(stdout.contains("Key"));
    assert!(stdout.contains("Place"));
    assert!(stdout.contains("Offer"));
    assert!(stdout.contains("Next"));
    assert!(stdout.contains("radroots market view sf-tomatoes"));
    assert!(stdout.contains("radroots order create --listing sf-tomatoes"));
    assert!(!stdout.contains("market · local first"));
}

#[test]
fn market_search_uses_also_searched_for_when_hyf_rewrites_query() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000402",
        "fresh-eggs",
        "protein",
        "Fresh Eggs",
        "Pasture-raised eggs",
        36,
        24,
        Some("Marshall"),
    );

    let hyfd = write_fake_hyfd(
        dir.path(),
        r#"{"version":1,"request_id":"cli-doctor-hyf-status","trace_id":"cli-doctor-hyf-status","ok":true,"output":{"build_identity":{"protocol_version":1},"enabled_execution_modes":{"deterministic":true}}}"#,
        r#"{"version":1,"request_id":"cli-find-query-rewrite","trace_id":"cli-find-query-rewrite","ok":true,"output":{"original_text":"henhouse","normalized_text":"henhouse","rewritten_text":"eggs","query_terms":["eggs"],"normalization_signals":["query_rewrite"],"ranking_hints":["local_first"],"extracted_filters":{"local_intent":false,"fulfillment":"any","time_window":"any"}}}"#,
    );

    let json_output = cli_command_in(dir.path())
        .env("RADROOTS_HYF_ENABLED", "true")
        .env("RADROOTS_HYF_EXECUTABLE", &hyfd)
        .args(["--json", "market", "search", "henhouse"])
        .output()
        .expect("run market search json");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["hyf"]["state"], "query_rewrite_applied");
    assert_eq!(json["hyf"]["rewritten_query"], "eggs");

    let human_output = cli_command_in(dir.path())
        .env("RADROOTS_HYF_ENABLED", "true")
        .env("RADROOTS_HYF_EXECUTABLE", &hyfd)
        .args(["market", "search", "henhouse"])
        .output()
        .expect("run market search human");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("1 listing for eggs"));
    assert!(stdout.contains("Also searched for"));
    assert!(stdout.contains("henhouse"));
    assert!(!stdout.contains("hyf: query rewritten"));
}

#[test]
fn market_view_wraps_listing_reads_and_guides_to_order_create() {
    let dir = tempdir().expect("tempdir");
    let init = cli_command_in(dir.path())
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());

    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000403",
        "pasture-eggs",
        "protein",
        "Pasture Eggs",
        "Fresh pasture-raised eggs collected daily.",
        36,
        18,
        Some("Marshall"),
    );

    let json_output = cli_command_in(dir.path())
        .args(["--json", "market", "view", "pasture-eggs"])
        .output()
        .expect("run market view json");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["product_key"], "pasture-eggs");
    assert_eq!(json["title"], "Pasture Eggs");
    assert_eq!(json["location_primary"], "Marshall");
    assert_eq!(
        json["actions"][0],
        "radroots order create --listing pasture-eggs"
    );

    let human_output = cli_command_in(dir.path())
        .args(["market", "view", "pasture-eggs"])
        .output()
        .expect("run market view human");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Pasture Eggs"));
    assert!(stdout.contains("Listing"));
    assert!(stdout.contains("Key"));
    assert!(stdout.contains("Place"));
    assert!(stdout.contains("About"));
    assert!(stdout.contains("radroots order create --listing pasture-eggs"));
    assert!(!stdout.contains("listing ·"));

    let missing_output = cli_command_in(dir.path())
        .args(["--json", "market", "view", "missing-listing"])
        .output()
        .expect("run missing market view");
    assert!(missing_output.status.success());
    let missing_json: Value =
        serde_json::from_slice(missing_output.stdout.as_slice()).expect("json");
    assert_eq!(missing_json["state"], "missing");
    assert_eq!(
        missing_json["actions"][0],
        "radroots market search tomatoes"
    );
    assert_eq!(missing_json["actions"][1], "radroots market update");
}
