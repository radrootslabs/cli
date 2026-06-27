mod support;

use std::fs;
use std::path::Path;

use serde_json::Value;
use support::{
    RadrootsCliSandbox, assert_contains, assert_no_daemon_runtime_reference,
    assert_no_removed_command_reference, create_listing_draft, identity_public, identity_secret,
    json_from_stdout, make_listing_publishable, seed_orderable_listing, shell_single_quoted,
    toml_string, write_public_identity_profile, write_secret_identity_profile,
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
    assert_eq!(created["result"]["account"]["custody"], "secret_backed");
    assert_eq!(created["result"]["account"]["write_capable"], true);
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
    assert_eq!(value["result"]["account"]["custody"], "watch_only");
    assert_eq!(value["result"]["account"]["write_capable"], false);
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
fn account_attach_secret_dry_run_validates_without_mutating_store() {
    let sandbox = RadrootsCliSandbox::new();
    let default_account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let default_account_id = default_account["result"]["account"]["id"]
        .as_str()
        .expect("default account id");
    let identity = identity_secret(31);
    let public_identity = identity.to_public();
    let public_identity_file =
        write_public_identity_profile(&sandbox, "attach-dry-public", &public_identity);
    let secret_identity_file =
        write_secret_identity_profile(&sandbox, "attach-dry-secret", &identity);
    let imported = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        public_identity_file.to_string_lossy().as_ref(),
    ]);
    let watch_account_id = imported["result"]["account"]["id"]
        .as_str()
        .expect("watch account id");
    assert_eq!(imported["result"]["account"]["signer"], "watch_only");
    assert_eq!(imported["result"]["account"]["custody"], "watch_only");
    assert_eq!(imported["result"]["account"]["write_capable"], false);
    assert_eq!(imported["result"]["account"]["is_default"], false);

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "attach-secret",
        watch_account_id,
        secret_identity_file.to_string_lossy().as_ref(),
        "--default",
    ]);

    assert_eq!(value["operation_id"], "account.attach_secret");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(value["result"]["default"], true);
    assert_eq!(value["result"]["account"]["id"], watch_account_id);
    assert_eq!(value["result"]["account"]["signer"], "local");
    assert_eq!(value["result"]["account"]["custody"], "secret_backed");
    assert_eq!(value["result"]["account"]["write_capable"], true);
    assert_eq!(value["result"]["account"]["is_default"], true);

    let watch_get = sandbox.json_success(&["--format", "json", "account", "get", watch_account_id]);
    assert_eq!(
        watch_get["result"]["account_resolution"]["resolved_account"]["signer"],
        "watch_only"
    );
    assert_eq!(
        watch_get["result"]["account_resolution"]["resolved_account"]["custody"],
        "watch_only"
    );
    assert_eq!(
        watch_get["result"]["account_resolution"]["resolved_account"]["write_capable"],
        false
    );
    let selected = sandbox.json_success(&["--format", "json", "account", "selection", "get"]);
    assert_eq!(
        selected["result"]["account_resolution"]["resolved_account"]["id"],
        default_account_id
    );
}

#[test]
fn account_attach_secret_attaches_matching_secret_and_can_make_default() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let identity = identity_secret(32);
    let public_identity = identity.to_public();
    let public_identity_file =
        write_public_identity_profile(&sandbox, "attach-public", &public_identity);
    let secret_identity_file = write_secret_identity_profile(&sandbox, "attach-secret", &identity);
    let imported = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        public_identity_file.to_string_lossy().as_ref(),
    ]);
    let watch_account_id = imported["result"]["account"]["id"]
        .as_str()
        .expect("watch account id");

    let attached = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "attach-secret",
        watch_account_id,
        secret_identity_file.to_string_lossy().as_ref(),
        "--default",
    ]);

    assert_eq!(attached["operation_id"], "account.attach_secret");
    assert_eq!(attached["result"]["state"], "secret_attached");
    assert_eq!(attached["result"]["account"]["id"], watch_account_id);
    assert_eq!(attached["result"]["account"]["signer"], "local");
    assert_eq!(attached["result"]["account"]["custody"], "secret_backed");
    assert_eq!(attached["result"]["account"]["write_capable"], true);
    assert_eq!(attached["result"]["account"]["is_default"], true);

    let status = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(status["result"]["state"], "ready");
    assert_eq!(status["result"]["signer_account_id"], watch_account_id);
    assert_eq!(status["result"]["local"]["availability"], "secret_backed");
}

#[test]
fn account_attach_secret_requires_approval_before_writing_secret() {
    let sandbox = RadrootsCliSandbox::new();
    let identity = identity_secret(33);
    let public_identity = identity.to_public();
    let public_identity_file =
        write_public_identity_profile(&sandbox, "attach-approval-public", &public_identity);
    let secret_identity_file =
        write_secret_identity_profile(&sandbox, "attach-approval-secret", &identity);
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
    let account_id = imported["result"]["account"]["id"]
        .as_str()
        .expect("account id");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "account",
        "attach-secret",
        account_id,
        secret_identity_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "account.attach_secret");
    assert_eq!(value["errors"][0]["code"], "approval_required");
    assert_eq!(value["errors"][0]["exit_code"], 6);
    let get = sandbox.json_success(&["--format", "json", "account", "get", account_id]);
    assert_eq!(
        get["result"]["account_resolution"]["resolved_account"]["signer"],
        "watch_only"
    );
    assert_eq!(
        get["result"]["account_resolution"]["resolved_account"]["custody"],
        "watch_only"
    );
    assert_eq!(
        get["result"]["account_resolution"]["resolved_account"]["write_capable"],
        false
    );
}

#[test]
fn account_attach_secret_reports_structured_validation_failures() {
    let sandbox = RadrootsCliSandbox::new();
    let matching_identity = identity_secret(34);
    let public_identity = matching_identity.to_public();
    let public_identity_file =
        write_public_identity_profile(&sandbox, "attach-fail-public", &public_identity);
    let secret_identity_file =
        write_secret_identity_profile(&sandbox, "attach-fail-secret", &matching_identity);
    let imported = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        public_identity_file.to_string_lossy().as_ref(),
    ]);
    let account_id = imported["result"]["account"]["id"]
        .as_str()
        .expect("account id");

    let (missing_input_output, missing_input) =
        sandbox.json_output(&["--format", "json", "--dry-run", "account", "attach-secret"]);
    assert!(!missing_input_output.status.success());
    assert_eq!(missing_input["operation_id"], "account.attach_secret");
    assert_eq!(missing_input["errors"][0]["code"], "invalid_input");

    let (missing_account_output, missing_account) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "attach-secret",
        "missing-account",
        secret_identity_file.to_string_lossy().as_ref(),
    ]);
    assert!(!missing_account_output.status.success());
    assert_eq!(missing_account["errors"][0]["code"], "account_unresolved");
    assert_eq!(missing_account["errors"][0]["exit_code"], 5);

    let mismatched_identity = identity_secret(35);
    let mismatched_identity_file =
        write_secret_identity_profile(&sandbox, "attach-mismatch-secret", &mismatched_identity);
    let (mismatch_output, mismatch) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "attach-secret",
        account_id,
        mismatched_identity_file.to_string_lossy().as_ref(),
    ]);
    assert!(!mismatch_output.status.success());
    assert_eq!(mismatch["errors"][0]["code"], "account_mismatch");
    assert_eq!(mismatch["errors"][0]["exit_code"], 5);

    let invalid_identity_file = sandbox.root().join("attach-invalid-secret.json");
    fs::write(&invalid_identity_file, "{ invalid json").expect("write invalid identity");
    let (invalid_output, invalid) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "attach-secret",
        account_id,
        invalid_identity_file.to_string_lossy().as_ref(),
    ]);
    assert!(!invalid_output.status.success());
    assert_eq!(invalid["errors"][0]["code"], "validation_failed");
    assert_eq!(invalid["errors"][0]["exit_code"], 10);

    let mut unavailable_command = sandbox.command();
    unavailable_command
        .env("RADROOTS_CLI_ACCOUNT_SECRET_BACKEND", "host_vault")
        .env("RADROOTS_CLI_ACCOUNT_SECRET_FALLBACK", "none")
        .env("RADROOTS_CLI_ACCOUNT_HOST_VAULT_AVAILABLE", "false")
        .args([
            "--format",
            "json",
            "--dry-run",
            "account",
            "attach-secret",
            account_id,
            secret_identity_file.to_string_lossy().as_ref(),
        ]);
    let unavailable_output = unavailable_command
        .output()
        .expect("run unavailable backend");
    let unavailable = json_from_stdout(&unavailable_output);
    assert!(!unavailable_output.status.success());
    assert_eq!(unavailable["errors"][0]["code"], "operation_unavailable");
    assert_eq!(unavailable["errors"][0]["exit_code"], 3);
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
fn account_remove_warns_when_farm_bound_seller_is_orphaned() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let farm = create_test_farm(&sandbox);
    let farm_path = farm["result"]["config"]["path"]
        .as_str()
        .expect("farm path");
    let farm_before_remove = fs::read_to_string(farm_path).expect("farm before account remove");
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");

    let non_bound_removed = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        second_account_id,
    ]);

    assert_eq!(non_bound_removed["operation_id"], "account.remove");
    assert_eq!(non_bound_removed["result"]["state"], "removed");
    assert_eq!(
        non_bound_removed["result"]["removed_account"]["id"],
        second_account_id
    );
    assert!(non_bound_removed["result"].get("warnings").is_none());
    assert!(non_bound_removed["result"].get("actions").is_none());
    assert!(
        non_bound_removed["warnings"]
            .as_array()
            .expect("top-level warnings")
            .is_empty()
    );
    assert!(
        non_bound_removed["next_actions"]
            .as_array()
            .expect("next actions")
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(farm_path).expect("farm after non-bound remove"),
        farm_before_remove
    );

    let orphaned = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        first_account_id,
    ]);

    assert_eq!(orphaned["operation_id"], "account.remove");
    assert_eq!(orphaned["result"]["state"], "removed");
    assert_eq!(
        orphaned["result"]["removed_account"]["id"],
        first_account_id
    );
    assert_eq!(
        orphaned["result"]["warnings"][0]["code"],
        "farm_bound_seller_orphaned"
    );
    assert_eq!(
        orphaned["result"]["warnings"][0]["subject_account_id"],
        first_account_id
    );
    assert_eq!(
        orphaned["result"]["warnings"][0]["farm_config"]["scope"],
        "workspace"
    );
    assert_eq!(
        orphaned["result"]["warnings"][0]["farm_config"]["path"],
        farm_path
    );
    assert_eq!(
        orphaned["warnings"][0]["code"],
        "farm_bound_seller_orphaned"
    );
    assert_action_present(&orphaned, "radroots account import <path>");
    assert_action_present(&orphaned, "radroots --dry-run farm rebind <selector>");
    assert_action_present(
        &orphaned,
        "radroots --approval-token approve farm rebind <selector>",
    );
    assert_next_action_present(&orphaned, "radroots account import <path>");
    assert_next_action_present(&orphaned, "radroots --dry-run farm rebind <selector>");
    assert_next_action_present(
        &orphaned,
        "radroots --approval-token approve farm rebind <selector>",
    );
    assert_eq!(
        fs::read_to_string(farm_path).expect("farm after farm-bound remove"),
        farm_before_remove
    );
}

#[test]
fn account_remove_farm_orphan_warning_renders_terminal() {
    let sandbox = RadrootsCliSandbox::new();
    let created = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = created["result"]["account"]["id"]
        .as_str()
        .expect("account id")
        .to_owned();
    create_test_farm(&sandbox);

    let output = sandbox
        .command()
        .args([
            "--approval-token",
            "approve",
            "account",
            "remove",
            account_id.as_str(),
        ])
        .output()
        .expect("terminal account remove");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("terminal stdout");
    assert!(stdout.contains("Account removed"), "{stdout}");
    assert!(stdout.contains(account_id.as_str()), "{stdout}");
    assert!(
        stdout.contains("Warnings\n  farm_bound_seller_orphaned:"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Next\n  radroots account import <path>"),
        "{stdout}"
    );
    assert!(
        stdout.contains("radroots --dry-run farm rebind <selector>"),
        "{stdout}"
    );
    assert!(
        stdout.contains("radroots --approval-token approve farm rebind <selector>"),
        "{stdout}"
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
    assert_eq!(imported["result"]["account"]["custody"], "watch_only");
    assert_eq!(imported["result"]["account"]["write_capable"], false);
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
        status["result"]["account_resolution"]["resolved_account"]["custody"],
        "watch_only"
    );
    assert_eq!(
        status["result"]["account_resolution"]["resolved_account"]["write_capable"],
        false
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
fn myc_signer_status_reports_missing_binding() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    configure_myc_mode(&sandbox, &missing_myc);

    let (output, value) = sandbox.json_output(&["--format", "json", "signer", "status", "get"]);

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "signer.status.get");
    assert!(value["errors"].as_array().expect("errors").is_empty());
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["binding"]["state"], "unconfigured");
    assert_eq!(value["result"]["myc"]["state"], "unconfigured");
    assert_eq!(value["result"]["myc"]["ready"], false);
    assert_contains(
        &value["result"]["reason"],
        "signer.remote_nip46 binding is missing",
    );
    assert_no_removed_command_reference(&value, &["signer", "status", "get"]);
}

#[test]
fn myc_signer_status_fails_closed_when_managed_account_is_unresolved() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    let remote_signer = identity_public(91);
    sandbox.write_app_config(&format!(
        r#"[signer]
backend = "myc"

[myc]
executable = "{}"

[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "bunker://{}?relay=wss%3A%2F%2Frelay.example"
managed_account_ref = "acct_missing"
signer_session_ref = "session_missing"
"#,
        toml_string(missing_myc.display().to_string().as_str()),
        remote_signer.public_key_hex,
    ));

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["binding"]["state"], "unconfigured");
    assert_eq!(value["result"]["myc"]["ready"], false);
    assert_contains(
        &value["result"]["reason"],
        "managed_account_ref `acct_missing` cannot be evaluated",
    );
    assert!(
        value["result"]["write_kinds"]
            .as_array()
            .expect("write kinds")
            .iter()
            .all(|kind| kind["ready"] == false)
    );
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

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "signer.status.get");
    assert!(value["errors"].as_array().expect("errors").is_empty());
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["myc"]["ready"], false);
    assert!(!invoked.exists(), "target CLI must not execute MYC");
}

#[test]
fn myc_mode_allows_read_inspection_commands() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    configure_myc_mode(&sandbox, &missing_myc);

    for args in [
        &["--format", "json", "workspace", "get"][..],
        &["--format", "json", "config", "get"][..],
        &["--format", "json", "account", "list"][..],
        &["--format", "json", "relay", "list"][..],
    ] {
        let (output, value) = sandbox.json_output(args);

        assert!(
            output.status.success(),
            "`{args:?}` should remain observable under MYC mode: {value:?}"
        );
        assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
    }
}

#[test]
fn local_listing_publish_fails_without_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    let listing_file = create_listing_draft(&sandbox, "local-no-account");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        account_id,
    ]);

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
        "listing-bound seller account",
    );
}

#[test]
fn local_listing_publish_dry_run_validates_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    let listing_file = create_listing_draft(&sandbox, "local-dry-run-no-account");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        account_id,
    ]);

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
fn local_listing_update_dry_run_validates_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    let listing_file = create_listing_draft(&sandbox, "local-update-dry-run-no-account");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        account_id,
    ]);

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.update");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_unresolved");
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_no_removed_command_reference(&value, &["listing", "update", "--dry-run"]);
}

#[test]
fn local_listing_update_dry_run_rejects_mismatched_local_account() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "local-update-dry-run-mismatch");
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
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert_ne!(
        first["result"]["account"]["id"],
        second["result"]["account"]["id"]
    );
    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.update");
    assert_eq!(value["errors"][0]["code"], "account_mismatch");
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
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
    assert_eq!(value["errors"][0]["code"], "empty_target_relays");
    assert_eq!(value["errors"][0]["detail"]["class"], "configuration");
    assert_contains(&value["errors"][0]["message"], "sdk empty target relays");
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
    assert_eq!(
        value["result"]["event_id"]
            .as_str()
            .expect("dry-run event id")
            .len(),
        64
    );
    assert!(
        !sandbox.root().join("data/apps/cli/replica/sdk").exists(),
        "dry-run must not materialize durable SDK storage"
    );
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
        "listing draft is bound to seller account",
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "farm",
        "publish",
    ]);

    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["result"]["state"], "dry_run");
    assert_eq!(value["result"]["dry_run"], true);
    assert_no_daemon_runtime_reference(&value, &["farm", "publish", "--dry-run"]);
}

#[test]
fn local_farm_publish_dry_run_fails_without_configured_relay() {
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);

    let (output, value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "farm", "publish"]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_eq!(value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &value["errors"][0]["message"],
        "requires at least one configured relay",
    );
    assert_no_removed_command_reference(&value, &["farm", "publish", "--dry-run"]);
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
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
fn farm_setup_actions_offer_publish_only_when_relay_publish_executable() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let unconfigured = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);

    assert_action_present(&unconfigured, "radroots farm readiness check");
    assert_action_absent(&unconfigured, "radroots farm publish");

    let configured = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "farm",
        "profile",
        "update",
        "--field",
        "name",
        "--value",
        "Green Farm Updated",
    ]);

    assert_action_present(&configured, "radroots farm readiness check");
    assert_action_present(&configured, "radroots farm publish");
}

#[test]
fn farm_setup_actions_withhold_publish_for_watch_only_account() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(51);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "farm-watch-only", &public_identity);
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

    let created = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "farm",
        "create",
        "--name",
        "Watch Farm",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);

    assert_action_present(&created, "radroots farm readiness check");
    assert_action_absent(&created, "radroots farm publish");

    let updated = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "farm",
        "profile",
        "update",
        "--field",
        "name",
        "--value",
        "Watch Farm Updated",
    ]);

    assert_action_present(&updated, "radroots farm readiness check");
    assert_action_absent(&updated, "radroots farm publish");
}

#[test]
fn local_farm_publish_reports_sdk_push_failure_without_profile_publish() {
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);
    let relay_url = "ws://127.0.0.1:9";

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay_url,
        "--approval-token",
        "approve",
        "--idempotency-key",
        "farm_partial",
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
        "SDK relay publish did not reach accepted quorum",
    );
    let detail = &value["errors"][0]["detail"];
    assert_eq!(detail["source"], "SDK farm publish · configured signer");
    assert_eq!(detail["state"], "unavailable");
    assert_eq!(detail["profile"]["state"], "not_submitted");
    assert_eq!(detail["farm"]["state"], "unavailable");
    assert_eq!(detail["profile"]["event_id"], serde_json::Value::Null);
    assert_eq!(
        detail["farm"]["event_id"]
            .as_str()
            .expect("sdk farm event id")
            .len(),
        64
    );
    assert_eq!(detail["profile"]["idempotency_key"], "farm_partial:profile");
    assert_eq!(detail["farm"]["idempotency_key"], "farm_partial:farm");
    assert_eq!(detail["actions"][0], "radroots sync push");
    assert_eq!(detail["farm"]["target_relays"][0], relay_url);
    assert_relay_url(&detail["farm"]["failed_relays"][0]["relay"], relay_url);
    assert_no_removed_command_reference(&value, &["farm", "publish"]);
    assert_no_daemon_runtime_reference(&value, &["farm", "publish"]);

    let persisted = sandbox.json_success(&["--format", "json", "farm", "get"]);
    assert_eq!(
        persisted["result"]["document"]["publication"]["profile_state"],
        "not_published"
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["farm_state"],
        "not_published"
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["profile_event_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["farm_event_id"],
        serde_json::Value::Null
    );
}

#[test]
fn local_farm_publish_does_not_persist_publication_until_sdk_push_publishes() {
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);
    let relay_url = "ws://127.0.0.1:9";

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay_url,
        "--approval-token",
        "approve",
        "--idempotency-key",
        "farm_success",
        "farm",
        "publish",
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_eq!(value["errors"][0]["detail"]["class"], "network");
    let detail = &value["errors"][0]["detail"];
    assert_eq!(detail["source"], "SDK farm publish · configured signer");
    assert_eq!(detail["profile"]["state"], "not_submitted");
    assert_eq!(detail["farm"]["state"], "unavailable");
    assert_eq!(detail["profile"]["event_id"], serde_json::Value::Null);
    assert_eq!(
        detail["farm"]["event_id"]
            .as_str()
            .expect("sdk farm event id")
            .len(),
        64
    );
    assert_eq!(detail["profile"]["idempotency_key"], "farm_success:profile");
    assert_eq!(detail["farm"]["idempotency_key"], "farm_success:farm");
    assert_no_removed_command_reference(&value, &["farm", "publish"]);
    assert_no_daemon_runtime_reference(&value, &["farm", "publish"]);

    let persisted = sandbox.json_success(&["--format", "json", "farm", "get"]);
    assert_eq!(
        persisted["result"]["document"]["publication"]["profile_state"],
        "not_published"
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["farm_state"],
        "not_published"
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["profile_event_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["farm_event_id"],
        serde_json::Value::Null
    );
}

#[test]
fn farm_rebind_is_explicit_and_publish_defaults_ignore_ambient_selection() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);
    let farm_path = farm["result"]["config"]["path"]
        .as_str()
        .expect("farm path");
    let farm_d_tag = farm["result"]["config"]["farm_d_tag"]
        .as_str()
        .expect("farm d tag");
    let first_pubkey = farm["result"]["config"]["seller_pubkey"]
        .as_str()
        .expect("first pubkey");
    assert_eq!(
        farm["result"]["config"]["seller_account_id"],
        first_account_id
    );
    assert_eq!(farm["result"]["config"]["seller_pubkey"], first_pubkey);
    assert_eq!(
        farm["result"]["config"]["seller_actor_source"],
        "farm_config"
    );
    assert!(
        farm["result"]["config"]
            .get("selected_account_id")
            .is_none()
    );

    let published = sandbox.json_success(&["--format", "json", "farm", "get"]);
    assert_eq!(
        published["result"]["document"]["publication"]["profile_state"],
        "not_published"
    );
    assert_eq!(
        published["result"]["document"]["publication"]["farm_state"],
        "not_published"
    );

    let same_seller_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "rebind",
        first_account_id,
    ]);
    assert_eq!(same_seller_dry_run["operation_id"], "farm.rebind");
    assert_eq!(
        same_seller_dry_run["result"]["publication_state_action"],
        "preserved"
    );

    let same_seller_live = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "farm",
        "rebind",
        first_account_id,
    ]);
    assert_eq!(same_seller_live["operation_id"], "farm.rebind");
    assert_eq!(
        same_seller_live["result"]["publication_state_action"],
        "preserved"
    );
    let same_seller_get = sandbox.json_success(&["--format", "json", "farm", "get"]);
    assert_eq!(
        same_seller_get["result"]["document"]["publication"]["profile_state"],
        "not_published"
    );
    assert_eq!(
        same_seller_get["result"]["document"]["publication"]["farm_state"],
        "not_published"
    );

    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");
    assert_ne!(first_account_id, second_account_id);
    sandbox.json_success(&[
        "--format",
        "json",
        "account",
        "selection",
        "update",
        second_account_id,
    ]);

    let (retarget_output, retarget) = sandbox.json_output(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm Retarget",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);
    assert!(!retarget_output.status.success());
    assert_eq!(retarget["operation_id"], "farm.create");
    assert_eq!(retarget["errors"][0]["code"], "account_mismatch");
    assert_contains(&retarget["errors"][0]["message"], "farm-bound seller");
    assert_eq!(
        retarget["errors"][0]["detail"]["seller_actor_source"],
        "farm_config"
    );
    assert_eq!(
        retarget["errors"][0]["detail"]["farm_bound_seller_account_id"],
        first_account_id
    );
    assert_eq!(
        retarget["errors"][0]["detail"]["attempted_seller_account_id"],
        second_account_id
    );
    assert_next_action_present(
        &retarget,
        format!("radroots --dry-run farm rebind {second_account_id}").as_str(),
    );
    assert_next_action_present(
        &retarget,
        format!("radroots --approval-token approve farm rebind {second_account_id}").as_str(),
    );

    let terminal_retarget_output = sandbox
        .command()
        .args([
            "farm",
            "create",
            "--name",
            "Green Farm Retarget",
            "--location",
            "farmstand",
            "--city",
            "San Francisco",
            "--country",
            "US",
            "--geohash",
            "9q8yy",
            "--delivery-method",
            "pickup",
        ])
        .output()
        .expect("terminal retarget");
    assert!(!terminal_retarget_output.status.success());
    assert!(terminal_retarget_output.stdout.is_empty());
    let terminal_stderr =
        String::from_utf8(terminal_retarget_output.stderr).expect("terminal stderr");
    assert!(
        terminal_stderr.contains("Next\n  radroots --dry-run farm rebind"),
        "{terminal_stderr}"
    );
    assert!(
        terminal_stderr.contains(
            format!("radroots --approval-token approve farm rebind {second_account_id}").as_str(),
        ),
        "{terminal_stderr}"
    );

    let (missing_rebind_output, missing_rebind) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "rebind",
        "acct_missing",
    ]);
    assert!(!missing_rebind_output.status.success());
    assert_eq!(missing_rebind["operation_id"], "farm.rebind");
    assert_eq!(missing_rebind["errors"][0]["code"], "account_unresolved");
    assert_eq!(
        missing_rebind["errors"][0]["detail"]["seller_actor_source"],
        "farm_config"
    );
    assert_eq!(
        missing_rebind["errors"][0]["detail"]["selector"],
        "acct_missing"
    );
    assert_next_action_present(&missing_rebind, "radroots account import <path>");
    assert_next_action_present(&missing_rebind, "radroots account create");

    let publish_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "farm",
        "publish",
    ]);
    assert_eq!(publish_dry_run["operation_id"], "farm.publish");
    assert_eq!(publish_dry_run["result"]["state"], "dry_run");
    assert_eq!(
        publish_dry_run["result"]["seller_account_id"],
        first_account_id
    );
    assert_eq!(publish_dry_run["result"]["seller_pubkey"], first_pubkey);
    assert!(
        publish_dry_run["result"]
            .get("selected_account_id")
            .is_none()
    );

    let listing_path = sandbox.root().join("drift-listing.toml");
    let listing = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "create",
        "--output",
        listing_path.to_string_lossy().as_ref(),
        "--key",
        "drift-eggs",
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
    assert_eq!(listing["operation_id"], "listing.create");
    assert_eq!(listing["result"]["seller_pubkey"], first_pubkey);
    assert_eq!(listing["result"]["farm_d_tag"], farm_d_tag);

    let farm_before_dry_run = fs::read_to_string(farm_path).expect("farm before dry-run rebind");
    let dry_rebind = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "rebind",
        second_account_id,
    ]);
    assert_eq!(dry_rebind["operation_id"], "farm.rebind");
    assert_eq!(dry_rebind["result"]["state"], "dry_run");
    assert_eq!(
        dry_rebind["result"]["from_seller_account_id"],
        first_account_id
    );
    assert_eq!(dry_rebind["result"]["from_seller_pubkey"], first_pubkey);
    assert_eq!(
        dry_rebind["result"]["to_seller_account_id"],
        second_account_id
    );
    let second_pubkey = dry_rebind["result"]["to_seller_pubkey"]
        .as_str()
        .expect("second pubkey");
    assert_eq!(dry_rebind["result"]["to_seller_pubkey"], second_pubkey);
    assert_eq!(dry_rebind["result"]["seller_pubkey_changed"], true);
    assert_eq!(dry_rebind["result"]["publication_state_action"], "cleared");
    assert_eq!(
        fs::read_to_string(farm_path).expect("farm after dry-run rebind"),
        farm_before_dry_run
    );

    let (unapproved_output, unapproved) =
        sandbox.json_output(&["--format", "json", "farm", "rebind", second_account_id]);
    assert!(!unapproved_output.status.success());
    assert_eq!(unapproved["operation_id"], "farm.rebind");
    assert_eq!(unapproved["errors"][0]["code"], "approval_required");

    let rebound = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "farm",
        "rebind",
        second_account_id,
    ]);
    assert_eq!(rebound["operation_id"], "farm.rebind");
    assert_eq!(rebound["result"]["state"], "rebound");
    assert_eq!(
        rebound["result"]["config"]["seller_account_id"],
        second_account_id
    );
    assert_eq!(rebound["result"]["config"]["seller_pubkey"], second_pubkey);
    assert_eq!(rebound["result"]["config"]["farm_d_tag"], farm_d_tag);
    assert_eq!(rebound["result"]["config"]["name"], "Green Farm");
    assert_eq!(rebound["result"]["config"]["location_primary"], "farmstand");
    assert_eq!(rebound["result"]["config"]["delivery_method"], "pickup");
    assert_eq!(rebound["result"]["publication_state_action"], "cleared");

    let rebound_get = sandbox.json_success(&["--format", "json", "farm", "get"]);
    assert_eq!(
        rebound_get["result"]["document"]["selection"]["seller_account_id"],
        second_account_id
    );
    assert_eq!(
        rebound_get["result"]["document"]["publication"]["profile_state"],
        "not_published"
    );
    assert_eq!(
        rebound_get["result"]["document"]["publication"]["farm_state"],
        "not_published"
    );
}

#[test]
fn missing_farm_bound_seller_blocks_listing_create_and_guides_setup_repair() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Missing Seller Farm",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");
    sandbox.json_success(&[
        "--format",
        "json",
        "account",
        "selection",
        "update",
        second_account_id,
    ]);
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        first_account_id,
    ]);

    let updated = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "profile",
        "update",
        "--field",
        "name",
        "--value",
        "Missing Seller Farm Updated",
    ]);
    assert_eq!(updated["operation_id"], "farm.profile.update");
    assert_contains(&updated["result"]["reason"], "farm-bound seller account");
    assert_action_present(&updated, "radroots account import <path>");
    assert_action_present(&updated, "radroots --dry-run farm rebind <selector>");
    assert_action_present(
        &updated,
        "radroots --approval-token approve farm rebind <selector>",
    );

    let listing_path = sandbox.root().join("missing-seller-listing.toml");
    let (listing_output, listing) = sandbox.json_output(&[
        "--format",
        "json",
        "listing",
        "create",
        "--output",
        listing_path.to_string_lossy().as_ref(),
        "--key",
        "missing-seller-eggs",
        "--title",
        "Missing Seller Eggs",
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
    assert!(!listing_output.status.success());
    assert_eq!(listing["operation_id"], "listing.create");
    assert_eq!(listing["errors"][0]["code"], "account_unresolved");
    assert_contains(
        &listing["errors"][0]["message"],
        "farm-bound seller account",
    );
    assert_eq!(
        listing["errors"][0]["detail"]["seller_actor_source"],
        "farm_config"
    );
    assert_eq!(
        listing["errors"][0]["detail"]["farm_bound_seller_account_id"],
        first_account_id
    );
    assert_next_action_present(&listing, "radroots account import <path>");
    assert_next_action_present(&listing, "radroots --dry-run farm rebind <selector>");
    assert_next_action_present(
        &listing,
        "radroots --approval-token approve farm rebind <selector>",
    );
    assert!(!listing_path.exists());
}

#[test]
fn farm_rebind_allows_watch_only_target_and_attach_secret_recovers_publish() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Watch Rebind Farm",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);
    let watch_identity = identity_secret(56);
    let watch_public = watch_identity.to_public();
    let public_identity_file =
        write_public_identity_profile(&sandbox, "watch-rebind-public", &watch_public);
    let secret_identity_file =
        write_secret_identity_profile(&sandbox, "watch-rebind-secret", &watch_identity);
    let imported = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        public_identity_file.to_string_lossy().as_ref(),
    ]);
    let watch_account_id = imported["result"]["account"]["id"]
        .as_str()
        .expect("watch account id");
    assert_eq!(imported["result"]["account"]["custody"], "watch_only");

    let rebound = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "farm",
        "rebind",
        watch_account_id,
    ]);
    assert_eq!(rebound["operation_id"], "farm.rebind");
    assert_eq!(
        rebound["result"]["config"]["seller_account_id"],
        watch_account_id
    );

    let readiness = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "farm",
        "readiness",
        "check",
    ]);
    assert_eq!(readiness["operation_id"], "farm.readiness.check");
    assert_eq!(readiness["result"]["publish_state"], "unconfigured");
    assert_eq!(
        readiness["result"]["missing"][0],
        "Write-capable farm-bound seller account"
    );
    assert_action_present(
        &readiness,
        format!("radroots account attach-secret {watch_account_id} <path>").as_str(),
    );

    let (publish_output, publish) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "farm",
        "publish",
    ]);
    assert!(!publish_output.status.success());
    assert_eq!(publish["operation_id"], "farm.publish");
    assert_eq!(publish["errors"][0]["code"], "account_watch_only");

    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "attach-secret",
        watch_account_id,
        secret_identity_file.to_string_lossy().as_ref(),
    ]);
    let recovered = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "farm",
        "publish",
    ]);
    assert_eq!(recovered["operation_id"], "farm.publish");
    assert_eq!(recovered["result"]["state"], "dry_run");
    assert_eq!(recovered["result"]["seller_account_id"], watch_account_id);
    assert_eq!(
        recovered["result"]["seller_pubkey"],
        watch_public.public_key_hex
    );
}

#[test]
fn local_seller_publish_commands_attempt_configured_relay() {
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
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
    assert_eq!(farm_value["operation_id"], "farm.publish");
    assert_eq!(farm_value["result"], serde_json::Value::Null);
    assert_eq!(farm_value["errors"][0]["code"], "network_unavailable");
    assert_eq!(farm_value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &farm_value["errors"][0]["message"],
        "SDK relay publish did not reach accepted quorum",
    );
    assert_eq!(
        farm_value["errors"][0]["detail"]["source"],
        "SDK farm publish · configured signer"
    );
    assert_eq!(
        farm_value["errors"][0]["detail"]["farm"]["target_relays"][0],
        relay
    );
    assert_eq!(
        farm_value["errors"][0]["detail"]["farm"]["failed_relays"][0]["relay"],
        relay
    );
    assert_no_removed_command_reference(&farm_value, &["farm", "publish"]);
    assert_no_daemon_runtime_reference(&farm_value, &["farm", "publish"]);

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
    assert_eq!(publish_value["operation_id"], "listing.publish");
    assert_eq!(publish_value["result"], serde_json::Value::Null);
    assert_eq!(publish_value["errors"][0]["code"], "network_unavailable");
    assert_eq!(publish_value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &publish_value["errors"][0]["message"],
        "SDK relay publish did not reach accepted quorum",
    );
    assert_no_removed_command_reference(&publish_value, &["listing", "publish"]);
    assert_no_daemon_runtime_reference(&publish_value, &["listing", "publish"]);
    assert_eq!(
        publish_value["errors"][0]["detail"]["target_relays"][0],
        relay
    );
    assert_eq!(
        publish_value["errors"][0]["detail"]["connected_relays"]
            .as_array()
            .expect("connected relays")
            .len(),
        1
    );
    assert_eq!(
        publish_value["errors"][0]["detail"]["failed_relays"]
            .as_array()
            .expect("failed relays")
            .len(),
        1
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
    assert_eq!(archive_value["operation_id"], "listing.archive");
    assert_eq!(archive_value["result"], serde_json::Value::Null);
    assert_eq!(archive_value["errors"][0]["code"], "network_unavailable");
    assert_eq!(archive_value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &archive_value["errors"][0]["message"],
        "SDK relay publish did not reach accepted quorum",
    );
    assert_no_removed_command_reference(&archive_value, &["listing", "archive"]);
    assert_no_daemon_runtime_reference(&archive_value, &["listing", "archive"]);
    assert_eq!(
        archive_value["errors"][0]["detail"]["target_relays"][0],
        relay
    );
    assert_eq!(
        archive_value["errors"][0]["detail"]["connected_relays"]
            .as_array()
            .expect("connected relays")
            .len(),
        1
    );
    assert_eq!(
        archive_value["errors"][0]["detail"]["failed_relays"]
            .as_array()
            .expect("failed relays")
            .len(),
        1
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
    let order_id = quote["result"]["quote"]["trade_id"]
        .as_str()
        .expect("order id");
    let (order_output, order_value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay,
        "--approval-token",
        "approve",
        "trade",
        "submit",
        order_id,
    ]);
    assert!(!order_output.status.success());
    assert_eq!(order_value["operation_id"], "trade.submit");
    assert_eq!(order_value["result"], serde_json::Value::Null);
    assert_eq!(order_value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(
        order_value["errors"][0]["detail"]["issues"][0]["field"],
        "trade.listing_addr"
    );
    assert_contains(
        &order_value["errors"][0]["detail"]["issues"][0]["message"],
        "local market freshness",
    );
    assert_no_removed_command_reference(&order_value, &["trade", "submit"]);
    assert_no_daemon_runtime_reference(&order_value, &["trade", "submit"]);
}

#[test]
fn local_order_event_list_attempts_configured_direct_relay() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let relay = "ws://127.0.0.1:9";

    let (output, value) = sandbox.json_output(&[
        "--format", "json", "--relay", relay, "trade", "event", "list",
    ]);

    assert!(!output.status.success());
    assert_direct_relay_connection_failure(&value, "trade.event.list", &["trade", "event", "list"]);
    assert_eq!(value["errors"][0]["detail"]["state"], "unavailable");
    assert_eq!(value["errors"][0]["detail"]["target_relays"][0], relay);
    assert_eq!(
        value["errors"][0]["detail"]["connected_relays"]
            .as_array()
            .expect("connected relays")
            .len(),
        0
    );
    assert_eq!(
        value["errors"][0]["detail"]["failed_relays"]
            .as_array()
            .expect("failed relays")
            .len(),
        1
    );
    assert_contains(
        &value["errors"][0]["detail"]["failed_relays"][0]["relay"],
        "127.0.0.1:9",
    );
    assert_eq!(value["errors"][0]["detail"]["fetched_count"], 0);
    assert_eq!(value["errors"][0]["detail"]["decoded_count"], 0);
    assert_eq!(value["errors"][0]["detail"]["skipped_count"], 0);
}

#[test]
fn local_order_failure_envelopes_are_structured_and_actionable() {
    let sandbox = RadrootsCliSandbox::new();
    let watch_args = ["--format", "json", "trade", "event", "watch", "ord_missing"];
    let (watch_output, watch) = sandbox.json_output(&watch_args);
    assert!(!watch_output.status.success());
    assert_eq!(watch["operation_id"], "trade.event.watch");
    assert_eq!(watch["result"], Value::Null);
    assert_eq!(watch["errors"][0]["code"], "not_implemented");
    assert_eq!(watch["errors"][0]["detail"]["state"], "not_implemented");
    assert_eq!(watch["errors"][0]["detail"]["trade_id"], "ord_missing");
    assert_eq!(
        watch["next_actions"][0]["command"],
        "radroots trade status get ord_missing"
    );
    assert_no_daemon_runtime_reference(&watch, &watch_args);

    let submit_args = [
        "--format",
        "json",
        "--publish-transport",
        "direct_nostr_relay",
        "--dry-run",
        "trade",
        "submit",
        "ord_missing",
    ];
    let (submit_output, submit) = sandbox.json_output(&submit_args);
    assert!(!submit_output.status.success());
    assert_eq!(submit["errors"][0]["code"], "not_found");
    assert_eq!(submit["errors"][0]["detail"]["state"], "missing");
    assert_eq!(submit["errors"][0]["detail"]["trade_id"], "ord_missing");
    assert_eq!(submit["next_actions"][0]["command"], "radroots trade list");
    assert_eq!(
        submit["next_actions"][1]["command"],
        "radroots basket create"
    );
    assert_no_daemon_runtime_reference(&submit, &submit_args);

    let status_args = ["--format", "json", "trade", "status", "get", "ord_missing"];
    let status = sandbox.json_success(&status_args);
    assert_eq!(status["operation_id"], "trade.status.get");
    assert_eq!(status["result"]["state"], "missing");
    assert_eq!(status["result"]["source"], "SDK local trade projection");
    assert_eq!(
        status["result"]["actor_context_source"],
        "sdk_local_projection"
    );
    assert_eq!(status["result"]["trade_id"], "ord_missing");
    assert_eq!(status["result"]["fetched_count"], 0);
    assert_eq!(status["result"]["decoded_count"], 0);
    assert_eq!(
        status["result"]["reason"],
        "no local SDK trade events matched `ord_missing`"
    );
    assert_no_daemon_runtime_reference(&status, &status_args);

    let event_list_no_relay_args = ["--format", "json", "trade", "event", "list"];
    let (event_list_no_relay_output, event_list_no_relay) =
        sandbox.json_output(&event_list_no_relay_args);
    assert!(!event_list_no_relay_output.status.success());
    assert_eq!(
        event_list_no_relay["errors"][0]["code"],
        "operation_unavailable"
    );
    assert_eq!(
        event_list_no_relay["errors"][0]["detail"]["state"],
        "unconfigured"
    );
    assert_eq!(
        event_list_no_relay["next_actions"][0]["command"],
        "radroots --relay wss://relay.example.com trade event list"
    );
    assert_no_daemon_runtime_reference(&event_list_no_relay, &event_list_no_relay_args);

    let event_list_no_account_args = [
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "trade",
        "event",
        "list",
    ];
    let (event_list_no_account_output, event_list_no_account) =
        sandbox.json_output(&event_list_no_account_args);
    assert!(!event_list_no_account_output.status.success());
    assert_eq!(
        event_list_no_account["errors"][0]["code"],
        "operation_unavailable"
    );
    assert_eq!(
        event_list_no_account["errors"][0]["detail"]["state"],
        "unconfigured"
    );
    assert_eq!(
        event_list_no_account["next_actions"][0]["command"],
        "radroots account create"
    );
    assert_no_daemon_runtime_reference(&event_list_no_account, &event_list_no_account_args);

    let accept_args = [
        "--format",
        "json",
        "--publish-transport",
        "direct_nostr_relay",
        "--dry-run",
        "trade",
        "accept",
        "ord_missing",
    ];
    let (accept_output, accept) = sandbox.json_output(&accept_args);
    assert!(!accept_output.status.success());
    assert_eq!(accept["errors"][0]["code"], "operation_unavailable");
    assert_eq!(accept["errors"][0]["detail"]["state"], "unconfigured");
    assert_eq!(accept["errors"][0]["detail"]["trade_id"], "ord_missing");
    assert_eq!(accept["errors"][0]["detail"]["decision"], "accepted");
    assert_no_daemon_runtime_reference(&accept, &accept_args);

    let decline_args = [
        "--format",
        "json",
        "--publish-transport",
        "direct_nostr_relay",
        "--dry-run",
        "trade",
        "decline",
        "ord_missing",
        "--reason",
        "not available",
    ];
    let (decline_output, decline) = sandbox.json_output(&decline_args);
    assert!(!decline_output.status.success());
    assert_eq!(decline["errors"][0]["code"], "operation_unavailable");
    assert_eq!(decline["errors"][0]["detail"]["state"], "unconfigured");
    assert_eq!(decline["errors"][0]["detail"]["trade_id"], "ord_missing");
    assert_eq!(decline["errors"][0]["detail"]["decision"], "declined");
    assert_no_daemon_runtime_reference(&decline, &decline_args);
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
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ]);

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "farm",
        "publish",
    ]);

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
    assert_contains(&value["errors"][0]["message"], "resolved account");
    assert_contains(&value["errors"][0]["message"], "watch_only");
}

#[test]
fn watch_only_listing_update_dry_run_fails_as_account_watch_only() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(13);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "watch-only-update", &public_identity);
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
    let listing_file = create_listing_draft(&sandbox, "watch-only-update");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.update");
    assert_eq!(value["result"], serde_json::Value::Null);
    assert_eq!(value["errors"][0]["code"], "account_watch_only");
    assert_eq!(value["errors"][0]["exit_code"], 7);
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_contains(&value["errors"][0]["message"], "watch_only");
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
    assert_eq!(value["errors"][0]["code"], "signer_unconfigured");
    assert_eq!(value["errors"][0]["exit_code"], 7);
    assert_eq!(value["errors"][0]["detail"]["class"], "signer");
    assert_contains(
        &value["errors"][0]["message"],
        "signer.remote_nip46 binding is missing",
    );
    assert!(!invoked.exists(), "target CLI must not execute MYC");
}

fn configure_myc_mode(sandbox: &RadrootsCliSandbox, executable: &Path) {
    sandbox.write_app_config(&format!(
        "[signer]\nbackend = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
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

fn assert_relay_url(value: &Value, relay_url: &str) {
    let actual = value.as_str().expect("relay url");
    assert!(
        actual == relay_url || actual == format!("{relay_url}/"),
        "expected relay url `{actual}` to match `{relay_url}`"
    );
}

fn assert_action_present(value: &Value, action: &str) {
    assert!(
        action_list(value).iter().any(|entry| *entry == action),
        "expected action `{action}` in `{}`",
        value["result"]["actions"]
    );
}

fn assert_next_action_present(value: &Value, action: &str) {
    assert!(
        next_action_commands(value)
            .iter()
            .any(|entry| *entry == action),
        "expected next action `{action}` in `{}`",
        value["next_actions"]
    );
}

fn assert_action_absent(value: &Value, action: &str) {
    assert!(
        action_list(value).iter().all(|entry| *entry != action),
        "did not expect action `{action}` in `{}`",
        value["result"]["actions"]
    );
}

fn action_list(value: &Value) -> Vec<&str> {
    value["result"]["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .map(|entry| entry.as_str().expect("action"))
        .collect()
}

fn next_action_commands(value: &Value) -> Vec<&str> {
    value["next_actions"]
        .as_array()
        .expect("next actions")
        .iter()
        .filter_map(|entry| entry["command"].as_str())
        .collect()
}

fn create_test_farm(sandbox: &RadrootsCliSandbox) -> Value {
    sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Green Farm",
        "--location",
        "farmstand",
        "--city",
        "San Francisco",
        "--country",
        "US",
        "--geohash",
        "9q8yy",
        "--delivery-method",
        "pickup",
    ])
}
