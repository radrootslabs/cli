use std::path::Path;
use std::process::Command;

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

fn json_output(output: &std::process::Output) -> Value {
    serde_json::from_slice(output.stdout.as_slice()).expect("json output")
}

fn json_string_array(json: &Value, field: &str) -> Vec<String> {
    json[field]
        .as_array()
        .expect("array field")
        .iter()
        .map(|value| value.as_str().expect("string item").to_owned())
        .collect()
}

#[test]
fn farm_init_requires_a_selected_account() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["--json", "farm", "init"])
        .output()
        .expect("run farm init");

    assert_eq!(output.status.code(), Some(3));
    let json = json_output(&output);
    assert_eq!(json["state"], "unconfigured");
    assert_eq!(
        json["actions"],
        serde_json::json!(["radroots account create"])
    );
}

#[test]
fn farm_init_creates_a_minimal_draft_and_reports_missing_fields() {
    let dir = tempdir().expect("tempdir");

    let account = cli_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account.status.success());
    let account_json = json_output(&account);
    let account_id = account_json["account"]["id"]
        .as_str()
        .expect("account id")
        .to_owned();

    let init = cli_command_in(dir.path())
        .args(["--json", "farm", "init"])
        .output()
        .expect("run farm init");
    assert!(init.status.success());
    let init_json = json_output(&init);
    assert_eq!(init_json["state"], "saved");
    assert_eq!(init_json["config"]["selected_account_id"], account_id);
    assert_eq!(
        init_json["actions"],
        serde_json::json!(["radroots farm check"])
    );

    let check = cli_command_in(dir.path())
        .args(["farm", "check"])
        .output()
        .expect("run farm check");
    assert_eq!(check.status.code(), Some(3));
    let stdout = String::from_utf8(check.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Farm not ready yet"));
    assert!(stdout.contains("Missing"));
    assert!(stdout.contains("Location"));
    assert!(stdout.contains("Delivery method"));
    assert!(stdout.contains("radroots farm set location \"San Francisco, CA\""));
    assert!(stdout.contains("radroots farm set delivery pickup"));

    let publish = cli_command_in(dir.path())
        .args(["--json", "farm", "publish"])
        .output()
        .expect("run farm publish");
    assert_eq!(publish.status.code(), Some(3));
    let publish_json = json_output(&publish);
    assert_eq!(publish_json["state"], "unconfigured");
    let missing = json_string_array(&publish_json, "missing");
    assert!(missing.contains(&"Location".to_owned()));
    assert!(missing.contains(&"Delivery method".to_owned()));
}

#[test]
fn farm_set_updates_the_draft_and_farm_check_turns_ready() {
    let dir = tempdir().expect("tempdir");

    let account = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account.status.success());

    let init = cli_command_in(dir.path())
        .args([
            "farm",
            "init",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
            "--country",
            "US",
        ])
        .output()
        .expect("run farm init");
    assert!(init.status.success());

    let set = cli_command_in(dir.path())
        .args(["--json", "farm", "set", "delivery", "pickup"])
        .output()
        .expect("run farm set");
    assert!(set.status.success());
    let set_json = json_output(&set);
    assert_eq!(set_json["state"], "updated");
    assert_eq!(set_json["field"], "Delivery");
    assert_eq!(set_json["value"], "Pickup");

    let check = cli_command_in(dir.path())
        .args(["--json", "farm", "check"])
        .output()
        .expect("run farm check");
    assert!(check.status.success());
    let check_json = json_output(&check);
    assert_eq!(check_json["state"], "ready");
    assert_eq!(
        check_json["actions"],
        serde_json::json!(["radroots farm publish"])
    );
    assert!(check_json.get("missing").is_none());
}

#[test]
fn farm_show_reports_a_missing_draft() {
    let dir = tempdir().expect("tempdir");

    let output = cli_command_in(dir.path())
        .args(["farm", "show"])
        .output()
        .expect("run farm show");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Farm draft not found"));
    assert!(stdout.contains("radroots farm init"));
}

#[test]
fn farm_setup_compatibility_path_still_produces_a_publishable_draft() {
    let dir = tempdir().expect("tempdir");

    let account = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account.status.success());

    let setup = cli_command_in(dir.path())
        .args([
            "--json",
            "farm",
            "setup",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
            "--country",
            "US",
            "--delivery-method",
            "pickup",
        ])
        .output()
        .expect("run farm setup");
    assert!(setup.status.success());
    let setup_json = json_output(&setup);
    assert_eq!(setup_json["state"], "configured");
    assert_eq!(
        setup_json["actions"],
        serde_json::json!(["radroots farm check", "radroots farm publish"])
    );

    let check = cli_command_in(dir.path())
        .args(["--json", "farm", "check"])
        .output()
        .expect("run farm check");
    assert!(check.status.success());
    let check_json = json_output(&check);
    assert_eq!(check_json["state"], "ready");
}

#[test]
fn listing_new_points_back_to_farm_check_when_defaults_are_incomplete() {
    let dir = tempdir().expect("tempdir");

    let account = cli_command_in(dir.path())
        .args(["account", "new"])
        .output()
        .expect("run account new");
    assert!(account.status.success());

    let init = cli_command_in(dir.path())
        .args(["farm", "init", "--name", "La Huerta"])
        .output()
        .expect("run farm init");
    assert!(init.status.success());

    let listing = cli_command_in(dir.path())
        .args([
            "--json",
            "listing",
            "new",
            "--key",
            "eggs",
            "--title",
            "Pasture eggs",
            "--category",
            "protein",
            "--summary",
            "Fresh pasture-raised eggs.",
        ])
        .output()
        .expect("run listing new");
    assert!(listing.status.success());
    let listing_json = json_output(&listing);
    assert_eq!(
        listing_json["reason"],
        "selected farm draft is missing delivery or location defaults; those fields were left blank"
    );
    let actions = json_string_array(&listing_json, "actions");
    assert!(actions.iter().any(|action| action == "radroots farm check"));
}
