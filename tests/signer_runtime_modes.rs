mod support;

use std::fs;
use std::path::Path;

use radroots_events::kinds::KIND_LISTING;
use radroots_identity::RadrootsIdentityPublic;
use serde_json::{Value, json};
use support::{
    RadrootsCliSandbox, assert_contains, assert_hex_len, create_listing_draft, identity_public,
    make_listing_publishable, shell_single_quoted, toml_string, write_public_identity_profile,
};

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
fn myc_signer_status_reports_unavailable_for_missing_executable() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    configure_myc_mode(&sandbox, &missing_myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
    assert_contains(&value["result"]["myc"]["reason"], "not found");
}

#[cfg(unix)]
#[test]
fn myc_signer_status_reports_unavailable_for_command_failure() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = sandbox.write_fake_myc(
        "myc-failure",
        myc_status_body("printf 'fake myc failed\\n' >&2\nexit 42").as_str(),
    );
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
    assert_contains(&value["result"]["myc"]["reason"], "status code 42");
    assert_contains(&value["result"]["myc"]["reason"], "fake myc failed");
}

#[cfg(unix)]
#[test]
fn myc_signer_status_reports_unavailable_for_invalid_json() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = sandbox.write_fake_myc(
        "myc-invalid-json",
        myc_status_body("printf 'not json\\n'").as_str(),
    );
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["myc"]["state"], "unavailable");
    assert_contains(&value["result"]["myc"]["reason"], "not valid JSON");
}

#[cfg(unix)]
#[test]
fn myc_signer_status_invokes_exact_status_view_argv() {
    let sandbox = RadrootsCliSandbox::new();
    let argv_log = sandbox.root().join("myc-argv.txt");
    let payload = ready_myc_payload(Vec::new());
    let raw = serde_json::to_string(&payload).expect("myc status payload");
    let body = format!(
        "printf '%s\\n' \"$*\" > '{}'\nprintf '%s\\n' '{}'",
        shell_single_quoted(argv_log.to_string_lossy().as_ref()),
        shell_single_quoted(raw.as_str())
    );
    let myc = sandbox.write_fake_myc(
        "myc-exact-status-argv",
        myc_status_body(body.as_str()).as_str(),
    );
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(
        fs::read_to_string(argv_log).expect("myc argv log"),
        "status --view signer\n"
    );
}

#[cfg(unix)]
#[test]
fn myc_signer_status_reports_unconfigured_when_ready_without_binding() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = write_fake_myc_status(
        &sandbox,
        "myc-ready-no-binding",
        ready_myc_payload(Vec::new()),
    );
    configure_myc_mode(&sandbox, &myc);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["operation_id"], "signer.status.get");
    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["myc"]["state"], "ready");
    assert_eq!(value["result"]["myc"]["ready"], true);
    assert_eq!(value["result"]["myc"]["remote_session_count"], 0);
    assert_eq!(value["result"]["binding"]["state"], "unconfigured");
    assert_contains(&value["result"]["binding"]["reason"], "signer.remote_nip46");
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_unsupported_target_kind() {
    let sandbox = RadrootsCliSandbox::new();
    let myc = write_fake_myc_status(
        &sandbox,
        "myc-ready-unsupported",
        ready_myc_payload(Vec::new()),
    );
    configure_myc_mode_with_binding(&sandbox, &myc, "explicit_endpoint", "default", None, None);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["binding"]["state"], "unsupported");
    assert_contains(&value["result"]["binding"]["reason"], "target_kind");
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_no_authorized_sessions() {
    let sandbox = RadrootsCliSandbox::new();
    let signer = identity_public(2);
    let user = identity_public(3);
    let payload = ready_myc_payload(vec![remote_session("session-ping", &signer, &user, "ping")]);
    let myc = write_fake_myc_status(&sandbox, "myc-no-authorized", payload);
    configure_myc_mode_with_binding(&sandbox, &myc, "managed_instance", "default", None, None);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unavailable");
    assert_eq!(value["result"]["binding"]["state"], "unavailable");
    assert_eq!(value["result"]["binding"]["matched_session_count"], 0);
    assert_contains(
        &value["result"]["binding"]["reason"],
        "no authorized remote signer session",
    );
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_ambiguous_authorized_sessions() {
    let sandbox = RadrootsCliSandbox::new();
    let signer = identity_public(4);
    let user_one = identity_public(5);
    let user_two = identity_public(6);
    let payload = ready_myc_payload(vec![
        remote_session("session-one", &signer, &user_one, "sign_event"),
        remote_session("session-two", &signer, &user_two, "sign_event"),
    ]);
    let myc = write_fake_myc_status(&sandbox, "myc-ambiguous", payload);
    configure_myc_mode_with_binding(&sandbox, &myc, "managed_instance", "default", None, None);

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "unconfigured");
    assert_eq!(value["result"]["binding"]["state"], "ambiguous");
    assert_eq!(value["result"]["binding"]["matched_session_count"], 2);
    assert_contains(
        &value["result"]["binding"]["reason"],
        "multiple authorized remote signer sessions",
    );
}

#[cfg(unix)]
#[test]
fn myc_binding_reports_ready_for_one_authorized_session() {
    let sandbox = RadrootsCliSandbox::new();
    let signer = identity_public(7);
    let user = identity_public(8);
    let payload = ready_myc_payload(vec![remote_session(
        "session-ready",
        &signer,
        &user,
        "sign_event",
    )]);
    let myc = write_fake_myc_status(&sandbox, "myc-ready-bound", payload);
    configure_myc_mode_with_binding(
        &sandbox,
        &myc,
        "managed_instance",
        "default",
        Some(user.id.as_str()),
        None,
    );

    let value = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);

    assert_eq!(value["result"]["mode"], "myc");
    assert_eq!(value["result"]["state"], "ready");
    assert_eq!(value["result"]["signer_account_id"], user.id.as_str());
    assert_eq!(value["result"]["binding"]["state"], "ready");
    assert_eq!(
        value["result"]["binding"]["resolved_signer_session_id"],
        "session-ready"
    );
    assert_eq!(value["result"]["binding"]["matched_session_count"], 1);
    assert_eq!(
        value["result"]["write_kinds"]
            .as_array()
            .expect("write kinds")
            .iter()
            .filter(|kind| kind["ready"] == true)
            .count(),
        4
    );
}

#[test]
fn local_listing_publish_fails_without_local_account_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let listing_file = create_listing_draft(&sandbox, "local-no-account");

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
    assert_eq!(value["errors"][0]["code"], "account_unresolved");
    assert_eq!(value["errors"][0]["exit_code"], 5);
    assert_eq!(value["errors"][0]["detail"]["class"], "account");
    assert_contains(
        &value["errors"][0]["message"],
        "no local account is selected",
    );
}

#[test]
fn local_listing_publish_signs_with_selected_account_without_remote_fallback() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "local-signed");
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

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"]["state"], "signed");
    assert_eq!(value["result"]["signer_mode"], "local");
    assert_eq!(
        value["result"]["signer_session_id"],
        serde_json::Value::Null
    );
    assert_eq!(value["result"]["job_id"], serde_json::Value::Null);
    assert_eq!(value["result"]["event"]["kind"], KIND_LISTING);
    assert_eq!(
        value["result"]["event"]["author"],
        value["result"]["seller_pubkey"]
    );
    assert_eq!(
        value["result"]["event"]["event_id"],
        value["result"]["event_id"]
    );
    assert_hex_len(&value["result"]["event_id"], 64);
    assert_hex_len(&value["result"]["event"]["signature"], 128);
    assert_contains(
        &value["result"]["reason"],
        "relay delivery was not attempted",
    );
    assert!(
        value["result"]["event"]["tags"]
            .as_array()
            .expect("event tags")
            .iter()
            .any(|tag| tag
                .as_array()
                .is_some_and(|items| items.first() == Some(&json!("d"))
                    && items.get(1) == Some(&value["result"]["listing_id"])))
    );
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
    let myc = write_fake_myc_status(
        &sandbox,
        "myc-ready-no-write-binding",
        ready_myc_payload(Vec::new()),
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
    assert_contains(&value["errors"][0]["message"], "signer.remote_nip46");
}

fn configure_myc_mode(sandbox: &RadrootsCliSandbox, executable: &Path) {
    sandbox.write_app_config(&format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
        toml_string(executable.display().to_string().as_str())
    ));
}

fn configure_myc_mode_with_binding(
    sandbox: &RadrootsCliSandbox,
    executable: &Path,
    target_kind: &str,
    target: &str,
    managed_account_ref: Option<&str>,
    signer_session_ref: Option<&str>,
) {
    let mut raw = format!(
        "[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n\n[[capability_binding]]\ncapability = \"signer.remote_nip46\"\nprovider = \"myc\"\ntarget_kind = \"{}\"\ntarget = \"{}\"\n",
        toml_string(executable.display().to_string().as_str()),
        toml_string(target_kind),
        toml_string(target)
    );
    if let Some(value) = managed_account_ref {
        raw.push_str(&format!(
            "managed_account_ref = \"{}\"\n",
            toml_string(value)
        ));
    }
    if let Some(value) = signer_session_ref {
        raw.push_str(&format!(
            "signer_session_ref = \"{}\"\n",
            toml_string(value)
        ));
    }
    sandbox.write_app_config(raw.as_str());
}

#[cfg(unix)]
fn write_fake_myc_status(
    sandbox: &RadrootsCliSandbox,
    name: &str,
    payload: Value,
) -> std::path::PathBuf {
    let raw = serde_json::to_string(&payload).expect("myc status payload");
    let body = format!("printf '%s\\n' '{}'", shell_single_quoted(raw.as_str()));
    sandbox.write_fake_myc(name, myc_status_body(body.as_str()).as_str())
}

#[cfg(unix)]
fn myc_status_body(body: &str) -> String {
    format!(
        "if [ \"$#\" -ne 3 ] || [ \"$1\" != 'status' ] || [ \"$2\" != '--view' ] || [ \"$3\" != 'signer' ]; then\nprintf '%s\\n' \"unexpected myc argv: $*\" >&2\nexit 64\nfi\n{body}"
    )
}

fn ready_myc_payload(remote_sessions: Vec<Value>) -> Value {
    json!({
        "status_contract_version": 1,
        "status": "ready",
        "ready": true,
        "signer_backend": {
            "remote_session_count": remote_sessions.len(),
            "remote_sessions": remote_sessions
        }
    })
}

fn remote_session(
    connection_id: &str,
    signer_identity: &RadrootsIdentityPublic,
    user_identity: &RadrootsIdentityPublic,
    permissions: &str,
) -> Value {
    json!({
        "connection_id": connection_id,
        "signer_identity": signer_identity,
        "user_identity": user_identity,
        "relays": ["wss://relay.example.test"],
        "permissions": permissions
    })
}
