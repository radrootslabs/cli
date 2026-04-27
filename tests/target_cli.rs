mod support;

use serde_json::Value;

use support::{
    RadrootsCliSandbox, assert_no_removed_command_reference, create_listing_draft,
    make_listing_publishable, ndjson_from_stdout, radroots,
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
        "runtime",
        "job",
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
        "message", "approval", "agent",
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
        "message", "approval", "agent",
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
    assert_eq!(value["errors"][0]["code"], "runtime_error");
    assert_no_removed_command_reference(
        &value,
        &["listing", "publish", "--dry-run", "missing-listing.toml"],
    );
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
fn required_approval_missing_token_returns_structured_error() {
    let output = radroots()
        .args(["--format", "json", "order", "submit"])
        .output()
        .expect("run order submit");

    assert_eq!(output.status.code(), Some(6));
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "approval_required");
    assert_eq!(value["errors"][0]["exit_code"], 6);
}

#[test]
fn buyer_target_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();

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

    let orders = sandbox.json_success(&["--format", "json", "order", "list"]);
    assert_eq!(orders["operation_id"], "order.list");
    assert_eq!(orders["result"]["state"], "ready");
    assert_eq!(orders["result"]["count"], 1);
    assert_eq!(orders["result"]["orders"][0]["id"], order_id);
    assert_eq!(orders["result"]["orders"][0]["ready_for_submit"], false);
    assert_eq!(
        orders["result"]["orders"][0]["issues"][0]["field"],
        "buyer_account_id"
    );
    assert_no_removed_command_reference(&orders, &["order", "list"]);

    let submit =
        sandbox.json_success(&["--format", "json", "--dry-run", "order", "submit", order_id]);
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["dry_run"], true);
    assert_eq!(submit["result"]["state"], "unconfigured");
    assert_eq!(submit["result"]["dry_run"], true);
    assert_eq!(submit["result"]["order_id"], order_id);
    assert!(
        submit["result"]["reason"]
            .as_str()
            .expect("submit reason")
            .contains("not ready for durable submit")
    );
    assert_eq!(submit["errors"].as_array().expect("errors").len(), 0);
    assert_no_removed_command_reference(&submit, &["order", "submit", "--dry-run"]);
}

#[test]
fn seller_target_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();
    let listing_file = sandbox.root().join("listing.toml");
    let listing_file = listing_file.to_string_lossy().into_owned();

    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    assert_eq!(account["operation_id"], "account.create");
    assert_eq!(account["result"]["account"]["signer"], "local");
    assert_no_removed_command_reference(&account, &["account", "create"]);

    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
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
        "--output",
        listing_file.as_str(),
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
    assert_eq!(create["operation_id"], "listing.create");
    assert_eq!(create["result"]["file"], listing_file);
    assert_no_removed_command_reference(&create, &["listing", "create"]);

    let validate = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "validate",
        listing_file.as_str(),
    ]);
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
        listing_file.as_str(),
    ]);
    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");
    assert_no_removed_command_reference(&publish, &["listing", "publish", "--dry-run"]);

    let signed = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file.as_str(),
    ]);
    assert_eq!(signed["operation_id"], "listing.publish");
    assert_eq!(signed["result"]["state"], "signed");
    assert_eq!(signed["result"]["signer_mode"], "local");
    assert_eq!(
        signed["result"]["event"]["author"],
        signed["result"]["seller_pubkey"]
    );
    assert!(signed["result"]["event"]["signature"].is_string());
    assert_no_removed_command_reference(&signed, &["listing", "publish"]);
}
