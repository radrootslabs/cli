mod support;

use std::fs;
use std::path::Path;

use serde_json::Value;

use support::{
    RadrootsCliSandbox, assert_no_daemon_runtime_reference, assert_no_removed_command_reference,
    create_listing_draft, identity_public, make_listing_publishable, ndjson_from_stdout, radroots,
    write_public_identity_profile,
};

const LISTING_ADDR: &str =
    "30402:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";

#[test]
fn root_help_exposes_only_target_namespaces() {
    let output = radroots().arg("--help").output().expect("run root help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    for namespace in [
        "workspace",
        "health",
        "config",
        "account",
        "signer",
        "relay",
        "store",
        "sync",
        "farm",
        "listing",
        "market",
        "basket",
        "order",
    ] {
        assert!(
            help_lists(&stdout, namespace),
            "root help should contain `{namespace}`"
        );
    }

    for removed in [
        "setup", "status", "doctor", "sell", "find", "local", "net", "myc", "rpc", "product",
        "runtime", "job", "message", "approval", "agent",
    ] {
        assert!(
            !help_lists(&stdout, removed),
            "root help should not contain `{removed}`"
        );
    }
}

fn help_lists(stdout: &str, command: &str) -> bool {
    stdout.lines().any(|line| {
        let line = line.trim_start();
        line == command || line.starts_with(&format!("{command} "))
    })
}

#[test]
fn removed_global_flags_are_rejected_publicly() {
    for args in [
        ["--output", "json", "workspace", "get"].as_slice(),
        ["--json", "workspace", "get"].as_slice(),
        ["--ndjson", "workspace", "get"].as_slice(),
        ["--yes", "workspace", "get"].as_slice(),
        ["--non-interactive", "workspace", "get"].as_slice(),
        ["--signer", "myc", "workspace", "get"].as_slice(),
        ["--farm-id", "farm_test", "workspace", "get"].as_slice(),
        ["--profile", "repo_local", "workspace", "get"].as_slice(),
        ["--signer-session-id", "session_test", "workspace", "get"].as_slice(),
    ] {
        let output = radroots().args(args).output().expect("run removed flag");

        assert!(!output.status.success(), "`{args:?}` should be rejected");
        let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
        assert!(stderr.contains("unexpected argument") || stderr.contains("unrecognized"));
    }
}

#[test]
fn removed_order_submit_watch_flag_is_rejected_publicly() {
    let output = radroots()
        .args(["order", "submit", "--watch"])
        .output()
        .expect("run removed order submit watch flag");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("unexpected argument") || stderr.contains("unrecognized"));
}

#[test]
fn removed_command_families_are_rejected_publicly() {
    for command in [
        "setup", "status", "doctor", "sell", "find", "local", "net", "myc", "rpc", "product",
        "runtime", "job", "message", "approval", "agent",
    ] {
        let output = radroots()
            .arg(command)
            .output()
            .expect("run removed command");

        assert!(!output.status.success(), "`{command}` should be rejected");
        let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
        assert!(stderr.contains("unrecognized subcommand"));
    }
}

#[test]
fn target_outputs_do_not_suggest_removed_command_families() {
    let sandbox = RadrootsCliSandbox::new();

    for args in [
        ["--format", "json", "market", "product", "search", "eggs"].as_slice(),
        ["--format", "json", "market", "listing", "get", "eggs"].as_slice(),
        ["--format", "json", "listing", "get", "eggs"].as_slice(),
        ["--format", "json", "listing", "list"].as_slice(),
        ["--format", "json", "sync", "status", "get"].as_slice(),
        [
            "--format",
            "json",
            "order",
            "get",
            "ord_AAAAAAAAAAAAAAAAAAAAAA",
        ]
        .as_slice(),
    ] {
        let value = sandbox.json_success(args);
        assert_no_removed_command_reference(&value, args);
    }
}

#[test]
fn listing_list_reports_empty_local_draft_state_truthfully() {
    let sandbox = RadrootsCliSandbox::new();
    let value = sandbox.json_success(&["--format", "json", "listing", "list"]);

    assert_eq!(value["operation_id"], "listing.list");
    assert_eq!(value["result"]["state"], "empty");
    assert_eq!(value["result"]["count"], 0);
    assert_eq!(
        value["result"]["listings"]
            .as_array()
            .expect("listings")
            .len(),
        0
    );
    assert!(
        value["result"]["draft_dir"]
            .as_str()
            .expect("draft dir")
            .ends_with("listings/drafts")
    );
    assert_no_removed_command_reference(&value, &["listing", "list"]);
}

#[test]
fn listing_list_reports_default_local_drafts() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let create = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "create",
        "--key",
        "eggs",
        "--title",
        "Eggs",
        "--category",
        "eggs",
        "--summary",
        "Fresh eggs",
        "--bin-id",
        "bin-1",
        "--quantity-amount",
        "1",
        "--quantity-unit",
        "each",
        "--price-amount",
        "6",
        "--price-currency",
        "USD",
        "--price-per-amount",
        "1",
        "--price-per-unit",
        "each",
        "--available",
        "10",
    ]);
    let listing_file = create["result"]["file"].as_str().expect("listing file");
    assert!(Path::new(listing_file).exists());

    let value = sandbox.json_success(&["--format", "json", "listing", "list"]);
    let listing = &value["result"]["listings"][0];

    assert_eq!(value["operation_id"], "listing.list");
    assert_eq!(value["result"]["state"], "ready");
    assert_eq!(value["result"]["count"], 1);
    assert_eq!(listing["id"], create["result"]["listing_id"]);
    assert_eq!(listing["state"], "ready");
    assert_eq!(listing["file"], listing_file);
    assert_eq!(listing["product_key"], "eggs");
    assert_eq!(listing["title"], "Eggs");
    assert_eq!(listing["category"], "eggs");
    assert_eq!(listing["location_primary"], "farmstand");
    assert!(listing["seller_pubkey"].is_string());
    assert!(listing["farm_d_tag"].is_string());
    assert_no_removed_command_reference(&value, &["listing", "list"]);
}

#[test]
fn account_id_global_populates_envelope_actor() {
    let output = radroots()
        .args([
            "--format",
            "json",
            "--account-id",
            "acct_test",
            "workspace",
            "get",
        ])
        .output()
        .expect("run workspace get");

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["actor"]["account_id"], "acct_test");
    assert_eq!(value["actor"]["role"], "account");
}

#[test]
fn target_command_outputs_standard_json_envelope() {
    let output = radroots()
        .args(["--format", "json", "workspace", "get"])
        .output()
        .expect("run workspace get");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["schema_version"], "radroots.cli.output.v1");
    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["kind"], "workspace.get");
    assert_eq!(value["dry_run"], false);
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn default_human_output_is_concise_and_not_json() {
    let output = radroots()
        .args(["workspace", "get"])
        .output()
        .expect("run workspace get");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("workspace.get: ok\n"));
    assert!(stdout.contains("request_id: req_workspace_get_"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn human_failure_output_preserves_error_code_and_message() {
    let output = radroots()
        .args(["--format", "human", "order", "submit"])
        .output()
        .expect("run order submit");

    assert_eq!(output.status.code(), Some(6));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("order.submit: error\n"));
    assert!(stdout.contains("request_id: req_order_submit_"));
    assert!(stdout.contains("error: approval_required"));
    assert!(stdout.contains("message: missing required `approval_token` input"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn request_ids_are_invocation_unique_and_preserve_caller_fields() {
    let first = radroots()
        .args([
            "--format",
            "json",
            "--correlation-id",
            "corr_test",
            "--idempotency-key",
            "idem_test",
            "workspace",
            "get",
        ])
        .output()
        .expect("run first workspace get");
    let second = radroots()
        .args([
            "--format",
            "json",
            "--correlation-id",
            "corr_test",
            "--idempotency-key",
            "idem_test",
            "workspace",
            "get",
        ])
        .output()
        .expect("run second workspace get");

    assert!(first.status.success());
    assert!(second.status.success());
    let first: Value = serde_json::from_slice(&first.stdout).expect("first json envelope");
    let second: Value = serde_json::from_slice(&second.stdout).expect("second json envelope");

    assert_eq!(first["correlation_id"], "corr_test");
    assert_eq!(first["idempotency_key"], "idem_test");
    assert_eq!(second["correlation_id"], "corr_test");
    assert_eq!(second["idempotency_key"], "idem_test");
    assert!(
        first["request_id"]
            .as_str()
            .expect("first request id")
            .starts_with("req_workspace_get_")
    );
    assert_ne!(first["request_id"], second["request_id"]);
}

#[test]
fn supported_ndjson_outputs_started_and_completed_frames() {
    let sandbox = RadrootsCliSandbox::new();
    let output = sandbox
        .command()
        .args(["--format", "ndjson", "account", "list"])
        .output()
        .expect("run account list ndjson");

    assert!(output.status.success());
    let frames = ndjson_from_stdout(&output);

    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["schema_version"], "radroots.cli.output.v1");
    assert_eq!(frames[0]["operation_id"], "account.list");
    assert_eq!(frames[0]["frame_type"], "started");
    assert_eq!(frames[0]["sequence"], 0);
    assert_eq!(frames[1]["operation_id"], "account.list");
    assert_eq!(frames[1]["frame_type"], "completed");
    assert_eq!(frames[1]["sequence"], 1);
    assert_eq!(frames[1]["errors"].as_array().expect("errors").len(), 0);
    assert_eq!(frames[0]["request_id"], frames[1]["request_id"]);
}

#[test]
fn unsupported_ndjson_returns_structured_invalid_input() {
    let output = radroots()
        .args(["--format", "ndjson", "workspace", "get"])
        .output()
        .expect("run workspace get ndjson");

    assert_eq!(output.status.code(), Some(2));
    let frames = ndjson_from_stdout(&output);

    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["operation_id"], "workspace.get");
    assert_eq!(frames[0]["frame_type"], "started");
    assert_eq!(frames[1]["operation_id"], "workspace.get");
    assert_eq!(frames[1]["frame_type"], "error");
    assert_eq!(frames[1]["errors"][0]["code"], "invalid_input");
    assert_eq!(frames[1]["errors"][0]["exit_code"], 2);
}

#[test]
fn offline_forbids_external_network_operations() {
    let output = radroots()
        .args(["--format", "json", "--offline", "sync", "pull"])
        .output()
        .expect("run offline sync pull");

    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "sync.pull");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "offline_forbidden");
    assert_eq!(value["errors"][0]["exit_code"], 8);
}

#[test]
fn offline_allows_supported_external_dry_run() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "offline-dry-run");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");

    let publish = sandbox.json_success(&[
        "--format",
        "json",
        "--offline",
        "--dry-run",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");
}

#[test]
fn listing_publish_dry_run_validates_missing_file() {
    let sandbox = RadrootsCliSandbox::new();
    let missing = sandbox.root().join("missing-listing.toml");
    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        missing.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "not_found");
    assert_eq!(value["errors"][0]["exit_code"], 4);
    assert_no_removed_command_reference(
        &value,
        &["listing", "publish", "--dry-run", "missing-listing.toml"],
    );
}

#[test]
fn listing_publish_invalid_draft_returns_validation_failure() {
    let sandbox = RadrootsCliSandbox::new();
    let invalid = sandbox.root().join("invalid-listing.toml");
    fs::write(&invalid, "listing = [").expect("write invalid listing");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        invalid.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "validation_failed");
    assert_eq!(value["errors"][0]["exit_code"], 10);
}

#[test]
fn online_requires_relay_for_external_network_operations() {
    let output = radroots()
        .args(["--format", "json", "--online", "market", "refresh"])
        .output()
        .expect("run online market refresh");

    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "market.refresh");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_eq!(value["errors"][0]["exit_code"], 8);
}

#[test]
fn online_allows_local_diagnostics() {
    let value = RadrootsCliSandbox::new().json_success(&[
        "--format",
        "json",
        "--online",
        "workspace",
        "get",
    ]);

    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn store_export_dry_run_is_structured_unsupported() {
    let sandbox = RadrootsCliSandbox::new();
    let (output, value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "store", "export"]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(value["operation_id"], "store.export");
    assert_eq!(value["errors"][0]["code"], "invalid_input");
    assert_eq!(value["errors"][0]["exit_code"], 2);
}

#[test]
fn store_backup_dry_run_preflights_initialized_store_without_writing_file() {
    let sandbox = RadrootsCliSandbox::new();
    let (missing_output, missing_value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "store", "backup", "create"]);

    assert!(!missing_output.status.success());
    assert_eq!(missing_value["operation_id"], "store.backup.create");
    assert_eq!(missing_value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(missing_value["errors"][0]["exit_code"], 3);

    let init = sandbox.json_success(&["--format", "json", "store", "init"]);
    assert_eq!(init["operation_id"], "store.init");

    let backup =
        sandbox.json_success(&["--format", "json", "--dry-run", "store", "backup", "create"]);
    let file = backup["result"]["file"].as_str().expect("backup file");

    assert_eq!(backup["operation_id"], "store.backup.create");
    assert_eq!(backup["dry_run"], true);
    assert_eq!(backup["result"]["state"], "dry_run");
    assert_eq!(backup["result"]["size_bytes"], 0);
    assert!(!Path::new(file).exists());
}

#[test]
fn core_account_store_dry_runs_preflight_without_mutating_local_state() {
    let sandbox = RadrootsCliSandbox::new();

    let workspace = sandbox.json_success(&["--format", "json", "--dry-run", "workspace", "init"]);
    let workspace_db = workspace["result"]["local"]["path"]
        .as_str()
        .expect("workspace db path");
    assert_eq!(workspace["operation_id"], "workspace.init");
    assert_eq!(workspace["dry_run"], true);
    assert_eq!(workspace["result"]["state"], "dry_run");
    assert_eq!(workspace["result"]["local"]["replica_db"], "missing");
    assert!(!Path::new(workspace_db).exists());

    let store = sandbox.json_success(&["--format", "json", "--dry-run", "store", "init"]);
    let store_db = store["result"]["path"].as_str().expect("store db path");
    assert_eq!(store["operation_id"], "store.init");
    assert_eq!(store["dry_run"], true);
    assert_eq!(store["result"]["state"], "dry_run");
    assert_eq!(store["result"]["replica_db"], "missing");
    assert!(!Path::new(store_db).exists());

    let account_create =
        sandbox.json_success(&["--format", "json", "--dry-run", "account", "create"]);
    assert_eq!(account_create["operation_id"], "account.create");
    assert_eq!(account_create["dry_run"], true);
    assert_eq!(account_create["result"]["state"], "dry_run");
    assert_eq!(account_create["result"]["secret_backend"]["state"], "ready");

    let account_list = sandbox.json_success(&["--format", "json", "account", "list"]);
    assert_eq!(account_list["result"]["count"], 0);

    let created = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = created["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    let clear = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "selection",
        "clear",
    ]);
    assert_eq!(clear["operation_id"], "account.selection.clear");
    assert_eq!(clear["result"]["state"], "dry_run");
    assert_eq!(clear["result"]["cleared_account"]["id"], account_id);
    assert_eq!(clear["result"]["remaining_account_count"], 1);

    let selection = sandbox.json_success(&["--format", "json", "account", "selection", "get"]);
    assert_eq!(
        selection["result"]["account_resolution"]["default_account"]["id"],
        account_id
    );
}

#[test]
fn seller_dry_runs_preflight_without_mutating_farm_or_listing_files() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let farm_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let farm_path = farm_dry_run["result"]["config"]["path"]
        .as_str()
        .expect("farm path");
    assert_eq!(farm_dry_run["operation_id"], "farm.create");
    assert_eq!(farm_dry_run["result"]["state"], "dry_run");
    assert!(!Path::new(farm_path).exists());

    let missing_update = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "profile",
        "update",
        "--value",
        "Dry Name",
    ]);
    assert_eq!(missing_update["operation_id"], "farm.profile.update");
    assert_eq!(missing_update["result"]["state"], "unconfigured");
    assert!(!Path::new(farm_path).exists());

    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let farm_path = farm["result"]["config"]["path"]
        .as_str()
        .expect("farm path");
    let farm_before = fs::read_to_string(farm_path).expect("farm before");
    let farm_update = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "profile",
        "update",
        "--value",
        "Dry Name",
    ]);
    assert_eq!(farm_update["operation_id"], "farm.profile.update");
    assert_eq!(farm_update["result"]["state"], "dry_run");
    assert_eq!(farm_update["result"]["config"]["name"], "Dry Name");
    assert_eq!(
        fs::read_to_string(farm_path).expect("farm after dry-run"),
        farm_before
    );

    let listing_path = sandbox.root().join("dry-listing.toml");
    let listing_path_arg = listing_path.to_string_lossy();
    let listing_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "create",
        "--output",
        listing_path_arg.as_ref(),
        "--key",
        "eggs",
        "--title",
        "Eggs",
        "--category",
        "eggs",
        "--summary",
        "Fresh eggs",
        "--bin-id",
        "bin-1",
        "--quantity-amount",
        "1",
        "--quantity-unit",
        "each",
        "--price-amount",
        "6",
        "--price-currency",
        "USD",
        "--price-per-amount",
        "1",
        "--price-per-unit",
        "each",
        "--available",
        "10",
    ]);
    assert_eq!(listing_dry_run["operation_id"], "listing.create");
    assert_eq!(listing_dry_run["result"]["state"], "dry_run");
    assert_eq!(listing_dry_run["result"]["file"], listing_path_arg.as_ref());
    assert!(!listing_path.exists());

    fs::write(&listing_path, "existing").expect("existing listing path");
    let (collision_output, collision) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "create",
        "--output",
        listing_path_arg.as_ref(),
        "--key",
        "eggs",
    ]);
    assert!(!collision_output.status.success());
    assert_eq!(collision["operation_id"], "listing.create");
    assert_eq!(collision["errors"][0]["code"], "validation_failed");

    let listing_file = create_listing_draft(&sandbox, "seller-dry-run");
    make_listing_publishable(
        &listing_file,
        farm["result"]["config"]["farm_d_tag"]
            .as_str()
            .expect("farm d tag"),
    );
    let listing_before = fs::read_to_string(&listing_file).expect("listing before");
    let listing_update = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);
    assert_eq!(listing_update["operation_id"], "listing.update");
    assert_eq!(listing_update["result"]["state"], "dry_run");
    assert_eq!(
        fs::read_to_string(&listing_file).expect("listing after dry-run"),
        listing_before
    );
}

#[test]
fn buyer_market_sync_basket_dry_runs_preflight_without_mutating_local_state() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let market = sandbox.json_success(&["--format", "json", "--dry-run", "market", "refresh"]);
    assert_eq!(market["operation_id"], "market.refresh");
    assert_eq!(market["dry_run"], true);
    assert_eq!(market["result"]["state"], "unconfigured");
    assert_eq!(market["result"]["replica_db"], "missing");

    let sync_pull = sandbox.json_success(&["--format", "json", "--dry-run", "sync", "pull"]);
    assert_eq!(sync_pull["operation_id"], "sync.pull");
    assert_eq!(sync_pull["dry_run"], true);
    assert_eq!(sync_pull["result"]["state"], "unconfigured");
    assert_eq!(sync_pull["result"]["replica_db"], "missing");

    let sync_push = sandbox.json_success(&["--format", "json", "--dry-run", "sync", "push"]);
    assert_eq!(sync_push["operation_id"], "sync.push");
    assert_eq!(sync_push["dry_run"], true);
    assert_eq!(sync_push["result"]["state"], "unconfigured");
    assert_eq!(sync_push["result"]["replica_db"], "missing");

    let create_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "create",
        "basket_probe",
    ]);
    let basket_file = create_dry_run["result"]["file"]
        .as_str()
        .expect("basket file");
    assert_eq!(create_dry_run["operation_id"], "basket.create");
    assert_eq!(create_dry_run["result"]["state"], "dry_run");
    assert!(!Path::new(basket_file).exists());

    sandbox.json_success(&["--format", "json", "basket", "create", "basket_probe"]);
    let (collision_output, collision) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "create",
        "basket_probe",
    ]);
    assert!(!collision_output.status.success());
    assert_eq!(collision["operation_id"], "basket.create");
    assert_eq!(collision["errors"][0]["code"], "invalid_input");

    let before_add = sandbox.json_success(&["--format", "json", "basket", "get", "basket_probe"]);
    let add = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "item",
        "add",
        "basket_probe",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    assert_eq!(add["operation_id"], "basket.item.add");
    assert_eq!(add["result"]["state"], "dry_run");
    let after_add = sandbox.json_success(&["--format", "json", "basket", "get", "basket_probe"]);
    assert_eq!(after_add["result"], before_add["result"]);

    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "basket_probe",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "quote",
        "create",
        "basket_probe",
    ]);
    let order_file = quote["result"]["order"]["file"]
        .as_str()
        .expect("order file");
    assert_eq!(quote["operation_id"], "basket.quote.create");
    assert_eq!(quote["result"]["state"], "dry_run");
    assert_eq!(quote["result"]["order"]["state"], "dry_run");
    assert!(!Path::new(order_file).exists());

    let basket_after_quote =
        sandbox.json_success(&["--format", "json", "basket", "get", "basket_probe"]);
    assert_eq!(basket_after_quote["result"]["quote"], Value::Null);
}

#[test]
fn required_approval_token_rejects_absent_empty_and_whitespace_values() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(61);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "approval-import", &public_identity);
    let public_identity_path = public_identity_file.to_string_lossy();

    assert_required_approval_token_rejected(
        &sandbox,
        "account.import",
        &["account", "import", public_identity_path.as_ref()],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "account.remove",
        &["account", "remove", "acct_missing"],
    );
    assert_required_approval_token_rejected(&sandbox, "farm.publish", &["farm", "publish"]);
    assert_required_approval_token_rejected(
        &sandbox,
        "listing.publish",
        &["listing", "publish", "missing-listing.toml"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "listing.archive",
        &["listing", "archive", "missing-listing.toml"],
    );
    assert_required_approval_token_rejected(&sandbox, "order.submit", &["order", "submit"]);
}

fn assert_required_approval_token_rejected(
    sandbox: &RadrootsCliSandbox,
    operation_id: &str,
    command_args: &[&str],
) {
    for token in [None, Some(""), Some(" \t ")] {
        let mut args = vec!["--format", "json"];
        if let Some(token) = token {
            args.push("--approval-token");
            args.push(token);
        }
        args.extend_from_slice(command_args);

        let (output, value) = sandbox.json_output(&args);

        assert_eq!(output.status.code(), Some(6), "`{args:?}` should fail");
        assert_eq!(value["operation_id"], operation_id);
        assert_eq!(value["errors"][0]["code"], "approval_required");
        assert_eq!(value["errors"][0]["exit_code"], 6);
        assert_no_removed_command_reference(&value, &args);
    }
}

#[test]
fn order_submit_missing_order_returns_not_found_while_read_view_stays_successful() {
    let sandbox = RadrootsCliSandbox::new();

    let get = sandbox.json_success(&[
        "--format",
        "json",
        "order",
        "get",
        "ord_missing_submit_target",
    ]);
    assert_eq!(get["operation_id"], "order.get");
    assert_eq!(get["result"]["state"], "missing");
    assert_eq!(get["errors"].as_array().expect("errors").len(), 0);

    let (output, submit) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "order",
        "submit",
        "ord_missing_submit_target",
    ]);

    assert_eq!(output.status.code(), Some(4));
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["errors"][0]["code"], "not_found");
    assert_eq!(submit["errors"][0]["exit_code"], 4);
    assert_eq!(submit["errors"][0]["detail"]["class"], "resource");
    assert_no_removed_command_reference(&submit, &["order", "submit"]);
}

#[test]
fn buyer_target_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();

    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    assert_eq!(account["operation_id"], "account.create");
    assert_eq!(account["result"]["account"]["signer"], "local");
    assert_no_removed_command_reference(&account, &["account", "create"]);

    let signer = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(signer["operation_id"], "signer.status.get");
    assert_eq!(signer["result"]["mode"], "local");
    assert_eq!(signer["result"]["state"], "ready");
    assert_eq!(signer["result"]["signer_account_id"], account_id);
    assert_no_removed_command_reference(&signer, &["signer", "status", "get"]);

    let search = sandbox.json_success(&["--format", "json", "market", "product", "search", "eggs"]);
    assert_eq!(search["operation_id"], "market.product.search");
    assert_eq!(search["errors"].as_array().expect("errors").len(), 0);
    assert_no_removed_command_reference(&search, &["market", "product", "search"]);

    let create = sandbox.json_success(&["--format", "json", "basket", "create", "basket_flow"]);
    assert_eq!(create["operation_id"], "basket.create");
    assert_eq!(create["result"]["basket_id"], "basket_flow");
    assert_no_removed_command_reference(&create, &["basket", "create"]);

    let add = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "basket_flow",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    assert_eq!(add["operation_id"], "basket.item.add");
    assert_eq!(add["result"]["ready_for_quote"], true);
    assert_no_removed_command_reference(&add, &["basket", "item", "add"]);

    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "basket_flow",
    ]);
    assert_eq!(quote["operation_id"], "basket.quote.create");
    assert_eq!(quote["result"]["state"], "quoted");
    assert_no_removed_command_reference(&quote, &["basket", "quote", "create"]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");
    assert_eq!(quote["result"]["quote"]["ready_for_submit"], true);
    assert_eq!(quote["result"]["order"]["buyer_account_id"], account_id);

    let orders = sandbox.json_success(&["--format", "json", "order", "list"]);
    assert_eq!(orders["operation_id"], "order.list");
    assert_eq!(orders["result"]["state"], "ready");
    assert_eq!(orders["result"]["count"], 1);
    assert_eq!(orders["result"]["orders"][0]["id"], order_id);
    assert_eq!(orders["result"]["orders"][0]["ready_for_submit"], true);
    assert_eq!(
        orders["result"]["orders"][0]["buyer_account_id"],
        account_id
    );
    assert_eq!(orders["result"]["orders"][0]["issues"], Value::Null);
    assert_no_removed_command_reference(&orders, &["order", "list"]);

    let submit =
        sandbox.json_success(&["--format", "json", "--dry-run", "order", "submit", order_id]);
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["dry_run"], true);
    assert_eq!(submit["result"]["state"], "dry_run");
    assert_eq!(submit["result"]["dry_run"], true);
    assert_eq!(submit["result"]["order_id"], order_id);
    assert_eq!(submit["result"]["buyer_account_id"], account_id);
    assert_eq!(submit["errors"].as_array().expect("errors").len(), 0);
    assert_no_removed_command_reference(&submit, &["order", "submit", "--dry-run"]);
}

#[test]
fn ready_order_submit_dry_run_validates_local_buyer_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    sandbox.json_success(&["--format", "json", "basket", "create", "ready_order"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "ready_order",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "ready_order",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");
    assert_eq!(quote["result"]["quote"]["ready_for_submit"], true);
    assert_eq!(
        quote["result"]["order"]["buyer_account_id"],
        first_account_id
    );

    let dry_run =
        sandbox.json_success(&["--format", "json", "--dry-run", "order", "submit", order_id]);

    assert_eq!(dry_run["operation_id"], "order.submit");
    assert_eq!(dry_run["dry_run"], true);
    assert_eq!(dry_run["result"]["state"], "dry_run");
    assert_eq!(dry_run["result"]["dry_run"], true);
    assert_eq!(dry_run["result"]["buyer_account_id"], first_account_id);

    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");
    let (output, mismatch) = sandbox.json_output(&[
        "--format",
        "json",
        "--account-id",
        second_account_id,
        "--dry-run",
        "order",
        "submit",
        order_id,
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(mismatch["operation_id"], "order.submit");
    assert_eq!(mismatch["errors"][0]["code"], "account_mismatch");
    assert_eq!(mismatch["errors"][0]["detail"]["class"], "account");
    assert_no_removed_command_reference(&mismatch, &["order", "submit", "--dry-run"]);
}

#[test]
fn seller_target_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();

    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    assert_eq!(account["operation_id"], "account.create");
    assert_eq!(account["result"]["account"]["signer"], "local");
    assert_no_removed_command_reference(&account, &["account", "create"]);

    let signer = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(signer["operation_id"], "signer.status.get");
    assert_eq!(signer["result"]["mode"], "local");
    assert_eq!(signer["result"]["state"], "ready");
    assert_eq!(signer["result"]["signer_account_id"], account_id);
    assert_no_removed_command_reference(&signer, &["signer", "status", "get"]);

    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    assert_eq!(farm["operation_id"], "farm.create");
    assert_eq!(farm["result"]["state"], "saved");
    assert_no_removed_command_reference(&farm, &["farm", "create"]);

    let create = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "create",
        "--key",
        "eggs",
        "--title",
        "Eggs",
        "--category",
        "eggs",
        "--summary",
        "Fresh eggs",
        "--bin-id",
        "bin-1",
        "--quantity-amount",
        "1",
        "--quantity-unit",
        "each",
        "--price-amount",
        "6",
        "--price-currency",
        "USD",
        "--price-per-amount",
        "1",
        "--price-per-unit",
        "each",
        "--available",
        "10",
    ]);
    let listing_file = create["result"]["file"].as_str().expect("listing file");
    assert_eq!(create["operation_id"], "listing.create");
    assert!(Path::new(listing_file).exists());
    assert_no_removed_command_reference(&create, &["listing", "create"]);

    let list = sandbox.json_success(&["--format", "json", "listing", "list"]);
    assert_eq!(list["operation_id"], "listing.list");
    assert_eq!(list["result"]["state"], "ready");
    assert_eq!(list["result"]["count"], 1);
    assert_eq!(
        list["result"]["listings"][0]["id"],
        create["result"]["listing_id"]
    );
    assert_eq!(list["result"]["listings"][0]["state"], "ready");
    assert_no_removed_command_reference(&list, &["listing", "list"]);

    let validate = sandbox.json_success(&["--format", "json", "listing", "validate", listing_file]);
    assert_eq!(validate["operation_id"], "listing.validate");
    assert_eq!(validate["result"]["valid"], true);
    assert_eq!(validate["result"]["issues"], Value::Null);
    assert_no_removed_command_reference(&validate, &["listing", "validate"]);

    let publish = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        listing_file,
    ]);
    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");
    assert_no_removed_command_reference(&publish, &["listing", "publish", "--dry-run"]);
    assert_no_daemon_runtime_reference(&publish, &["listing", "publish", "--dry-run"]);

    let archive = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "archive",
        listing_file,
    ]);
    assert_eq!(archive["operation_id"], "listing.archive");
    assert_eq!(archive["result"]["state"], "dry_run");
    assert_eq!(archive["result"]["operation"], "archive");
    assert_no_removed_command_reference(&archive, &["listing", "archive", "--dry-run"]);
    assert_no_daemon_runtime_reference(&archive, &["listing", "archive", "--dry-run"]);

    let (publish_output, unavailable_publish) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file,
    ]);
    assert!(!publish_output.status.success());
    assert_eq!(unavailable_publish["operation_id"], "listing.publish");
    assert_eq!(
        unavailable_publish["errors"][0]["code"],
        "operation_unavailable"
    );
    assert_eq!(
        unavailable_publish["errors"][0]["detail"]["class"],
        "operation"
    );
    assert_no_removed_command_reference(&unavailable_publish, &["listing", "publish"]);
    assert_no_daemon_runtime_reference(&unavailable_publish, &["listing", "publish"]);

    let (archive_output, unavailable_archive) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "archive",
        listing_file,
    ]);
    assert!(!archive_output.status.success());
    assert_eq!(unavailable_archive["operation_id"], "listing.archive");
    assert_eq!(
        unavailable_archive["errors"][0]["code"],
        "operation_unavailable"
    );
    assert_eq!(
        unavailable_archive["errors"][0]["detail"]["class"],
        "operation"
    );
    assert_no_removed_command_reference(&unavailable_archive, &["listing", "archive"]);
    assert_no_daemon_runtime_reference(&unavailable_archive, &["listing", "archive"]);
}
