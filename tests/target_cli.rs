mod support;

use serde_json::Value;

use support::{RadrootsCliSandbox, radroots};

const LISTING_ADDR: &str =
    "30402:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";

fn json_success(sandbox: &RadrootsCliSandbox, args: &[&str]) -> Value {
    sandbox.json_success(args)
}

#[test]
fn root_help_exposes_only_mvp_namespaces() {
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
fn unsupported_ndjson_returns_structured_invalid_input() {
    let output = radroots()
        .args(["--format", "ndjson", "workspace", "get"])
        .output()
        .expect("run workspace get ndjson");

    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["errors"][0]["code"], "invalid_input");
    assert_eq!(value["errors"][0]["exit_code"], 2);
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
    let listing_file = sandbox.root().join("listing.toml");
    let listing_file = listing_file.to_string_lossy().into_owned();

    let publish = json_success(
        &sandbox,
        &[
            "--format",
            "json",
            "--offline",
            "--dry-run",
            "listing",
            "publish",
            listing_file.as_str(),
        ],
    );

    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");
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
    let value = json_success(
        &RadrootsCliSandbox::new(),
        &["--format", "json", "--online", "workspace", "get"],
    );

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
fn buyer_mvp_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();

    let search = json_success(
        &sandbox,
        &["--format", "json", "market", "product", "search", "eggs"],
    );
    assert_eq!(search["operation_id"], "market.product.search");
    assert_eq!(search["errors"].as_array().expect("errors").len(), 0);

    let create = json_success(
        &sandbox,
        &["--format", "json", "basket", "create", "basket_flow"],
    );
    assert_eq!(create["operation_id"], "basket.create");
    assert_eq!(create["result"]["basket_id"], "basket_flow");

    let add = json_success(
        &sandbox,
        &[
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
        ],
    );
    assert_eq!(add["operation_id"], "basket.item.add");
    assert_eq!(add["result"]["ready_for_quote"], true);

    let quote = json_success(
        &sandbox,
        &[
            "--format",
            "json",
            "basket",
            "quote",
            "create",
            "basket_flow",
        ],
    );
    assert_eq!(quote["operation_id"], "basket.quote.create");
    assert_eq!(quote["result"]["state"], "quoted");
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");

    let submit = json_success(
        &sandbox,
        &["--format", "json", "--dry-run", "order", "submit", order_id],
    );
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["dry_run"], true);
    assert_eq!(submit["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn seller_mvp_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();
    let listing_file = sandbox.root().join("listing.toml");
    let listing_file = listing_file.to_string_lossy().into_owned();

    let create = json_success(
        &sandbox,
        &[
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
            "--bin-id",
            "bin-1",
            "--quantity-amount",
            "1",
            "--quantity-unit",
            "dozen",
            "--price-amount",
            "6",
            "--price-currency",
            "USD",
            "--price-per-amount",
            "1",
            "--price-per-unit",
            "dozen",
            "--available",
            "10",
        ],
    );
    assert_eq!(create["operation_id"], "listing.create");
    assert_eq!(create["result"]["file"], listing_file);

    let validate = json_success(
        &sandbox,
        &[
            "--format",
            "json",
            "listing",
            "validate",
            listing_file.as_str(),
        ],
    );
    assert_eq!(validate["operation_id"], "listing.validate");
    assert!(validate["result"]["valid"].is_boolean());

    let publish = json_success(
        &sandbox,
        &[
            "--format",
            "json",
            "--dry-run",
            "listing",
            "publish",
            listing_file.as_str(),
        ],
    );
    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");

    let orders = json_success(&sandbox, &["--format", "json", "order", "list"]);
    assert_eq!(orders["operation_id"], "order.list");
    assert_eq!(orders["errors"].as_array().expect("errors").len(), 0);
}
