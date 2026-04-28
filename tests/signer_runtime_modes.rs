mod support;

use std::path::Path;

use support::{
    RadrootsCliSandbox, assert_contains, assert_no_daemon_runtime_reference,
    assert_no_removed_command_reference, create_listing_draft, identity_public,
    make_listing_publishable, seed_orderable_listing, shell_single_quoted, toml_string,
    write_public_identity_profile,
};

const LISTING_ADDR: &str =
    "30402:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";

#[test]
fn local_signer_status_reports_unconfigured_without_account() {
    let sandbox = RadrootsCliSandbox::new();

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["schema_version"], "radroots.cli.output.v1");
    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["kind"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "local");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(
        value["result"]["signer_account_id"],
        serde_json::Value::Null
    );
    assert_eq!(value["result"]["binding"]["state"], "disabled");
    assert_eq!(value["result"]["local"], serde_json::Value::Null);
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn local_signer_status_reports_ready_after_account_create() {
    let sandbox = RadrootsCliSandbox::new();

    let created = sandbox.json_success(&["--format", "json", "account", "create"]);
    assert_eq!(created["operation_id"], "account.create");
    assert_eq!(created["result"]["state"], "created");
    assert_eq!(created["result"]["account"]["signer"], "local");
    assert_eq!(created["result"]["account"]["is_default"], true);
    let account_id = created["result"]["account"]["id"]
        .as_str()
        .expect("created account id");

    let status = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(status["operation_id"], "signer.status.get");
    assert_eq!(status["result"]["mode"], "local");
    assert_eq!(status["result"]["state"], "ready");
    assert_eq!(status["result"]["signer_account_id"], account_id);
    assert_eq!(status["result"]["local"]["account_id"], account_id);
    assert_eq!(status["result"]["local"]["availability"], "secret_backed");
    assert_eq!(status["result"]["local"]["secret_backed"], true);
    assert_eq!(status["result"]["local"]["backend"], "encrypted_file");
    assert_eq!(status["result"]["local"]["used_fallback"], false);
    assert_eq!(status["result"]["binding"]["state"], "disabled");
}

#[test]
fn local_account_selection_and_invocation_override_resolve_signer_actor() {
    let sandbox = RadrootsCliSandbox::new();

    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");
    assert_eq!(first["result"]["account"]["is_default"], true);
    assert_eq!(second["result"]["account"]["is_default"], false);

    let default_status = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(default_status["result"]["state"], "ready");
    assert_eq!(
        default_status["result"]["signer_account_id"],
        first_account_id
    );
    assert_eq!(
        default_status["result"]["account_resolution"]["source"],
        "default_account"
    );

    let override_status = sandbox.json_success(&[
        "--format",
        "json",
        "--account-id",
        second_account_id,
        "signer",
        "status",
        "get",
    ]);
    assert_eq!(override_status["actor"]["account_id"], second_account_id);
    assert_eq!(override_status["actor"]["role"], "account");
    assert_eq!(
        override_status["result"]["signer_account_id"],
        second_account_id
    );
    assert_eq!(
        override_status["result"]["account_resolution"]["source"],
        "invocation_override"
    );
    assert_eq!(
        override_status["result"]["account_resolution"]["default_account"]["id"],
        first_account_id
    );

    let selected = sandbox.json_success(&[
        "--format",
        "json",
        "account",
        "selection",
        "update",
        second_account_id,
    ]);
    assert_eq!(selected["operation_id"], "account.selection.update");
    assert_eq!(selected["result"]["state"], "default");
    assert_eq!(selected["result"]["account"]["id"], second_account_id);

    let selected_status = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(
        selected_status["result"]["signer_account_id"],
        second_account_id
    );
    assert_eq!(
        selected_status["result"]["account_resolution"]["source"],
        "default_account"
    );

    let selected_get =
        sandbox.json_success(&["--format", "json", "account", "get", first_account_id]);
    assert_eq!(selected_get["operation_id"], "account.get");
    assert_eq!(
        selected_get["result"]["account_resolution"]["source"],
        "invocation_override"
    );
    assert_eq!(
        selected_get["result"]["account_resolution"]["resolved_account"]["id"],
        first_account_id
    );
}

#[test]
fn account_import_dry_run_validates_profile_without_mutating_store() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(21);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "dry-run-import", &public_identity);

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "import",
        "--default",
        public_identity_file.to_string_lossy().as_ref(),
    ]);

    assert_eq!(value["operation_id"], "account.import");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(
        value["result"]["account"]["id"],
        public_identity.id.as_str()
    );
    assert_eq!(value["result"]["account"]["signer"], "watch_only");
    assert_eq!(value["result"]["account"]["is_default"], true);

    let list = sandbox.json_success(&["--format", "json", "account", "list"]);
    assert_eq!(list["result"]["count"], 0);
}

#[test]
fn account_import_dry_run_validates_missing_profile_file() {
    let sandbox = RadrootsCliSandbox::new();
    let missing = sandbox.root().join("missing-account.json");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "import",
        missing.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "account.import");
    assert_eq!(value["errors"][0]["code"], "not_found");
    assert_eq!(value["errors"][0]["exit_code"], 4);
}

#[test]
fn account_remove_dry_run_validates_selector_without_mutating_store() {
    let sandbox = RadrootsCliSandbox::new();
    let created = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = created["result"]["account"]["id"]
        .as_str()
        .expect("account id");

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "remove",
        account_id,
    ]);

    assert_eq!(value["operation_id"], "account.remove");
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(value["result"]["removed_account"]["id"], account_id);
    assert_eq!(value["result"]["default_would_clear"], true);
    assert_eq!(value["result"]["remaining_account_count"], 0);

    let get = sandbox.json_success(&["--format", "json", "account", "get", account_id]);
    assert_eq!(get["result"]["state"], "ready");
    assert_eq!(
        get["result"]["account_resolution"]["resolved_account"]["id"],
        account_id
    );
}

#[test]
fn account_selection_update_dry_run_validates_selector_without_mutating_selection() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "selection",
        "update",
        second_account_id,
    ]);

    assert_eq!(value["operation_id"], "account.selection.update");
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(value["result"]["account"]["id"], second_account_id);

    let selected = sandbox.json_success(&["--format", "json", "account", "selection", "get"]);
    assert_eq!(
        selected["result"]["account_resolution"]["resolved_account"]["id"],
        first_account_id
    );
}

#[test]
fn account_selection_update_dry_run_rejects_missing_selector() {
    let sandbox = RadrootsCliSandbox::new();
    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "selection",
        "update",
        "missing-account",
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "account.selection.update");
    assert_eq!(value["errors"][0]["code"], "account_unresolved");
    assert_eq!(value["errors"][0]["exit_code"], 5);
}

#[test]
fn unresolved_account_override_returns_account_failure() {
    let sandbox = RadrootsCliSandbox::new();
    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--account-id",
        "missing-account",
        "account",
        "get",
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "account.get");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_unresolved");
    assert_eq!(value["errors"][0]["exit_code"], 5);
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_contains(&value["errors"][0]["message"], "account selector");
}

#[test]
fn watch_only_import_reports_unconfigured_local_signer() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(11);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "watch-only", &public_identity);

    let imported = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        "--default",
        public_identity_file.to_string_lossy().as_ref(),
    ]);

    assert_eq!(imported["operation_id"], "account.import");
    assert_eq!(imported["result"]["state"], "imported");
    assert_eq!(
        imported["result"]["account"]["id"],
        public_identity.id.as_str()
    );
    assert_eq!(imported["result"]["account"]["signer"], "watch_only");
    assert_eq!(imported["result"]["account"]["is_default"], true);

    let status = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(status["result"]["mode"], "local");
    assert_eq!(status["result"]["state"], "unconfigured");
    assert_eq!(
        status["result"]["signer_account_id"],
        public_identity.id.as_str()
    );
    assert_eq!(
        status["result"]["account_resolution"]["source"],
        "default_account"
    );
    assert_eq!(
        status["result"]["account_resolution"]["resolved_account"]["signer"],
        "watch_only"
    );
    assert_eq!(
        status["result"]["local"]["account_id"],
        public_identity.id.as_str()
    );
    assert_eq!(status["result"]["local"]["availability"], "public_only");
    assert_eq!(status["result"]["local"]["secret_backed"], false);
    assert_contains(&status["result"]["reason"], "not secret-backed");
    assert!(
        status["result"]["write_kinds"]
            .as_array()
            .expect("write kinds")
            .iter()
            .all(|kind| kind["ready"] == false)
    );
}

#[test]
fn myc_signer_status_returns_deferred_signer_error() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    configure_myc_mode(&sandbox, &missing_myc);

    let (output, value) = sandbox.json_output(&["--format", "json", "signer", "status", "get"]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "signer_mode_deferred");
    assert_eq!(value["errors"][0]["exit_code"], 7);
    assert_eq!(value["errors"][0]["detail"]["class"], "signer");
    assert_contains(&value["errors"][0]["message"], "signer mode `myc`");
    assert_no_removed_command_reference(&value, &["signer", "status", "get"]);
}

#[cfg(unix)]
#[test]
fn myc_signer_status_does_not_invoke_configured_executable() {
    let sandbox = RadrootsCliSandbox::new();
    let invoked = sandbox.root().join("myc-invoked.txt");
    let myc = sandbox.write_fake_myc(
        "myc-deferred",
        format!(
            "printf invoked > '{}'",
            shell_single_quoted(invoked.to_string_lossy().as_ref())
        )
        .as_str(),
    );
    configure_myc_mode(&sandbox, &myc);

    let (output, value) = sandbox.json_output(&["--format", "json", "signer", "status", "get"]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["errors"][0]["code"], "signer_mode_deferred");
    assert!(!invoked.exists(), "target CLI must not execute MYC");
}

#[test]
fn local_listing_publish_fails_without_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let listing_file = create_listing_draft(&sandbox, "local-no-account");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_unresolved");
    assert_eq!(value["errors"][0]["exit_code"], 5);
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_contains(
        &value["errors"][0]["message"],
        "no local account is selected",
    );
}

#[test]
fn local_listing_publish_dry_run_validates_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let listing_file = create_listing_draft(&sandbox, "local-dry-run-no-account");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_unresolved");
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_no_removed_command_reference(&value, &["listing", "publish", "--dry-run"]);
}

#[test]
fn local_listing_publish_fails_without_configured_relay() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "local-unavailable");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_eq!(value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &value["errors"][0]["message"],
        "requires at least one configured relay",
    );
    assert_no_removed_command_reference(&value, &["listing", "publish"]);
    assert_no_daemon_runtime_reference(&value, &["listing", "publish"]);
}

#[test]
fn local_listing_publish_dry_run_does_not_sign_matching_listing() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "local-dry-run");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(value["result"]["dry_run"], true);
    assert_eq!(value["result"]["event_id"], serde_json::Value::Null);
    assert_no_removed_command_reference(&value, &["listing", "publish", "--dry-run"]);
    assert_no_daemon_runtime_reference(&value, &["listing", "publish", "--dry-run"]);
}

#[test]
fn local_listing_archive_dry_run_validates_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "local-archive-mismatch");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--account-id",
        second_account_id,
        "--dry-run",
        "listing",
        "archive",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.archive");
    assert_eq!(value["errors"][0]["code"], "account_mismatch");
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
}

#[test]
fn local_listing_publish_fails_when_selected_account_does_not_match_seller() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let listing_file = create_listing_draft(&sandbox, "local-mismatch");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");
    assert_ne!(first_account_id, second_account_id);

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--account-id",
        second_account_id,
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_mismatch");
    assert_eq!(value["errors"][0]["exit_code"], 5);
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_contains(
        &value["errors"][0]["message"],
        "cannot sign listing seller_pubkey",
    );
    assert_no_removed_command_reference(&value, &["listing", "publish", "account mismatch"]);
}

#[test]
fn local_farm_publish_dry_run_validates_secret_backed_account() {
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

    let value = sandbox.json_success(&["--format", "json", "--dry-run", "farm", "publish"]);

    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(value["result"]["dry_run"], true);
    assert_no_daemon_runtime_reference(&value, &["farm", "publish", "--dry-run"]);
}

#[test]
fn local_farm_publish_fails_without_configured_relay() {
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

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "farm",
        "publish",
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_eq!(value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &value["errors"][0]["message"],
        "requires at least one configured relay",
    );
    assert_no_removed_command_reference(&value, &["farm", "publish"]);
    assert_no_daemon_runtime_reference(&value, &["farm", "publish"]);
}

#[test]
fn local_seller_publish_commands_attempt_configured_direct_relay() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
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
    let farm_d_tag = farm["result"]["config"]["farm_d_tag"]
        .as_str()
        .expect("farm d tag");
    let relay = "ws://127.0.0.1:9";

    let (farm_output, farm_value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay,
        "--approval-token",
        "approve",
        "farm",
        "publish",
    ]);
    assert!(!farm_output.status.success());
    assert_direct_relay_connection_failure(&farm_value, "farm.publish", &["farm", "publish"]);

    let listing_file = create_listing_draft(&sandbox, "direct-relay-attempt");
    make_listing_publishable(&listing_file, farm_d_tag);
    let listing_file_arg = listing_file.to_string_lossy();

    let (publish_output, publish_value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay,
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file_arg.as_ref(),
    ]);
    assert!(!publish_output.status.success());
    assert_direct_relay_connection_failure(
        &publish_value,
        "listing.publish",
        &["listing", "publish"],
    );

    let (archive_output, archive_value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay,
        "--approval-token",
        "approve",
        "listing",
        "archive",
        listing_file_arg.as_ref(),
    ]);
    assert!(!archive_output.status.success());
    assert_direct_relay_connection_failure(
        &archive_value,
        "listing.archive",
        &["listing", "archive"],
    );

    seed_orderable_listing(&sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", "direct_order"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "direct_order",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "1",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "direct_order",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");
    let (order_output, order_value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay,
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id,
    ]);
    assert!(!order_output.status.success());
    assert_direct_relay_connection_failure(&order_value, "order.submit", &["order", "submit"]);
}

#[test]
fn local_order_event_list_attempts_configured_direct_relay() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let relay = "ws://127.0.0.1:9";

    let (output, value) = sandbox.json_output(&[
        "--format", "json", "--relay", relay, "order", "event", "list",
    ]);

    assert!(!output.status.success());
    assert_direct_relay_connection_failure(&value, "order.event.list", &["order", "event", "list"]);
}

#[test]
fn watch_only_farm_publish_dry_run_fails_as_account_watch_only() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(13);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "watch-only-farm", &public_identity);
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        "--default",
        public_identity_file.to_string_lossy().as_ref(),
    ]);
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

    let (output, value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "farm", "publish"]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["errors"][0]["code"], "account_watch_only");
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
}

#[test]
fn watch_only_listing_publish_fails_as_account_watch_only() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(12);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "watch-only-publish", &public_identity);
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        "--default",
        public_identity_file.to_string_lossy().as_ref(),
    ]);
    let listing_file = create_listing_draft(&sandbox, "watch-only-publish");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_watch_only");
    assert_eq!(value["errors"][0]["exit_code"], 7);
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_contains(&value["errors"][0]["message"], "watch_only account");
}

#[cfg(unix)]
#[test]
fn myc_listing_publish_does_not_fallback_to_local_account() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "myc-no-binding");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    let invoked = sandbox.root().join("myc-listing-invoked.txt");
    let myc = sandbox.write_fake_myc(
        "myc-listing-deferred",
        format!(
            "printf invoked > '{}'",
            shell_single_quoted(invoked.to_string_lossy().as_ref())
        )
        .as_str(),
    );
    configure_myc_mode(&sandbox, &myc);

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "signer_mode_deferred");
    assert_eq!(value["errors"][0]["exit_code"], 7);
    assert_eq!(value["errors"][0]["detail"]["class"], "signer");
    assert_contains(&value["errors"][0]["message"], "signer mode `myc`");
    assert!(!invoked.exists(), "target CLI must not execute MYC");
}

fn configure_myc_mode(sandbox: &RadrootsCliSandbox, executable: &Path) {
    sandbox.write_app_config(&format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
        toml_string(executable.display().to_string().as_str())
    ));
}

fn assert_direct_relay_connection_failure(
    value: &serde_json::Value,
    operation_id: &str,
    args: &[&str],
) {
    assert_eq!(value["operation_id"], operation_id);
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_ne!(value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &value["errors"][0]["message"],
        "direct relay connection failed",
    );
    assert_no_removed_command_reference(value, args);
    assert_no_daemon_runtime_reference(value, args);
}
