#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

use assert_cmd::prelude::*;
use radroots_events::RadrootsNostrEvent;
use radroots_events::kinds::{KIND_FARM, KIND_LISTING};
use radroots_events_codec::trade::RadrootsTradeListingAddress;
use radroots_identity::{RadrootsIdentity, RadrootsIdentityPublic};
use radroots_replica_sync::{RadrootsReplicaIngestOutcome, radroots_replica_ingest_event};
use radroots_sql_core::SqliteExecutor;
use serde_json::Value;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static COMMAND_LOCK: Mutex<()> = Mutex::new(());

pub fn radroots() -> Command {
    Command::cargo_bin("radroots").expect("binary")
}

pub fn json_from_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not json: {error}; stderr `{}`; stdout `{}`",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

pub fn ndjson_from_stdout(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let frames = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap_or_else(|error| {
                panic!(
                    "stdout line was not json: {error}; stderr `{}`; line `{line}`; stdout `{stdout}`",
                    String::from_utf8_lossy(&output.stderr)
                )
            })
        })
        .collect::<Vec<_>>();
    assert!(!frames.is_empty(), "stdout should contain ndjson frames");
    frames
}

pub struct RadrootsCliSandbox {
    root: TempDir,
}

impl RadrootsCliSandbox {
    pub fn new() -> Self {
        Self {
            root: TempDir::new().expect("tempdir"),
        }
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn command(&self) -> Command {
        let mut command = radroots();
        self.apply_base_env(&mut command);
        command
    }

    pub fn json_success(&self, args: &[&str]) -> Value {
        let _guard = COMMAND_LOCK.lock().expect("cli command lock");
        let output = self.command().args(args).output().expect("run command");
        assert!(
            output.status.success(),
            "`{args:?}` failed with stderr `{}` and stdout `{}`",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        json_from_stdout(&output)
    }

    pub fn json_output(&self, args: &[&str]) -> (Output, Value) {
        let _guard = COMMAND_LOCK.lock().expect("cli command lock");
        let output = self.command().args(args).output().expect("run command");
        let value = json_from_stdout(&output);
        (output, value)
    }

    pub fn write_workspace_config(&self, raw: &str) -> PathBuf {
        let path = self.root.path().join("config.toml");
        fs::write(&path, raw).expect("write workspace config");
        path
    }

    pub fn write_app_config(&self, raw: &str) -> PathBuf {
        let path = self.root.path().join("config/apps/cli/config.toml");
        fs::create_dir_all(path.parent().expect("app config parent")).expect("app config dir");
        fs::write(&path, raw).expect("write app config");
        path
    }

    #[cfg(unix)]
    pub fn write_fake_myc(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.path().join("bin").join(name);
        fs::create_dir_all(path.parent().expect("fake myc parent")).expect("fake myc dir");
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write fake myc");
        let mut permissions = fs::metadata(&path)
            .expect("fake myc metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("fake myc executable");
        path
    }

    fn apply_base_env(&self, command: &mut Command) {
        command.env("RADROOTS_CLI_PATHS_PROFILE", "repo_local");
        command.env("RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT", self.root.path());
        command.env("RADROOTS_ACCOUNT_SECRET_BACKEND", "encrypted_file");
        command.env("RADROOTS_ACCOUNT_SECRET_FALLBACK", "none");
    }
}

pub fn assert_no_removed_command_reference(value: &Value, args: &[&str]) {
    let raw = serde_json::to_string(value).expect("json value");
    for removed in [
        "radroots setup",
        "radroots status",
        "radroots doctor",
        "radroots sell",
        "radroots find",
        "radroots local",
        "radroots net",
        "radroots myc",
        "radroots rpc",
        "radroots account new",
        "radroots config show",
        "radroots runtime status get",
        "radroots runtime start",
        "radroots runtime stop",
        "radroots runtime restart",
        "radroots runtime log watch",
        "radroots runtime config get",
        "radroots runtime config show",
        "radroots runtime install",
        "radroots runtime uninstall",
        "radroots runtime config set",
        "radroots signer session",
        "myc status",
        "radroots job get",
        "radroots job list",
        "radroots job watch",
        "radroots job cancel",
        "radroots job retry",
        "radroots market search",
        "radroots market view",
        "radroots market update",
        "radroots order ls",
        "radroots order history",
        "radroots order watch",
        "radroots order new",
        "radroots order create",
        "radroots farm init",
        "radroots farm check",
        "radroots relay ls",
        "radroots product",
        "radroots message",
        "radroots approval",
        "radroots agent",
    ] {
        assert!(
            !raw.contains(removed),
            "`{args:?}` output should not contain removed command reference `{removed}`: {raw}"
        );
    }
}

pub fn assert_no_daemon_runtime_reference(value: &Value, args: &[&str]) {
    let raw = serde_json::to_string(value).expect("json value");
    for removed in ["radrootsd", "daemon", "bridge", "radroots job"] {
        assert!(
            !raw.contains(removed),
            "`{args:?}` output should not contain daemon runtime reference `{removed}`: {raw}"
        );
    }
}

pub fn assert_contains(value: &Value, needle: &str) {
    let value = value.as_str().expect("string value");
    assert!(
        value.contains(needle),
        "expected `{value}` to contain `{needle}`"
    );
}

pub fn assert_hex_len(value: &Value, expected_len: usize) {
    let value = value.as_str().expect("hex string");
    assert_eq!(value.len(), expected_len);
    assert!(value.chars().all(|ch| ch.is_ascii_hexdigit()));
}

pub fn seed_orderable_listing(sandbox: &RadrootsCliSandbox, listing_addr: &str) -> String {
    let store = sandbox.json_success(&["--format", "json", "store", "init"]);
    let db_path = store["result"]["path"]
        .as_str()
        .expect("replica db path from store init");
    let parsed = RadrootsTradeListingAddress::parse(listing_addr).expect("listing addr");
    let seller_pubkey = parsed.seller_pubkey.clone();
    let listing_id = parsed.listing_id.clone();
    let event_id = "2".repeat(64);
    let event = RadrootsNostrEvent {
        id: event_id.clone(),
        author: seller_pubkey.clone(),
        created_at: 1,
        kind: KIND_LISTING,
        tags: vec![
            vec!["d".to_owned(), listing_id],
            vec![
                "a".to_owned(),
                format!(
                    "{}:{}:{}",
                    KIND_FARM, seller_pubkey, "AAAAAAAAAAAAAAAAAAAAAA"
                ),
            ],
            vec!["p".to_owned(), seller_pubkey],
            vec!["key".to_owned(), "pasture-eggs".to_owned()],
            vec!["title".to_owned(), "Market Eggs".to_owned()],
            vec!["category".to_owned(), "eggs".to_owned()],
            vec!["summary".to_owned(), "Pasture-raised eggs".to_owned()],
            vec!["process".to_owned(), "washed".to_owned()],
            vec!["lot".to_owned(), "lot-a".to_owned()],
            vec!["profile".to_owned(), "dozen".to_owned()],
            vec!["year".to_owned(), "2026".to_owned()],
            vec!["radroots:primary_bin".to_owned(), "bin-1".to_owned()],
            vec![
                "radroots:bin".to_owned(),
                "bin-1".to_owned(),
                "12".to_owned(),
                "each".to_owned(),
                "12".to_owned(),
                "each".to_owned(),
                "dozen".to_owned(),
            ],
            vec![
                "radroots:price".to_owned(),
                "bin-1".to_owned(),
                "6".to_owned(),
                "USD".to_owned(),
                "1".to_owned(),
                "each".to_owned(),
                "6".to_owned(),
                "each".to_owned(),
            ],
            vec!["inventory".to_owned(), "5".to_owned()],
            vec!["status".to_owned(), "active".to_owned()],
        ],
        content: "# Market Eggs".to_owned(),
        sig: "f".repeat(128),
    };
    let executor = SqliteExecutor::open(Path::new(db_path)).expect("open replica db");
    assert_eq!(
        radroots_replica_ingest_event(&executor, &event).expect("ingest listing"),
        RadrootsReplicaIngestOutcome::Applied
    );
    event_id
}

pub fn create_listing_draft(sandbox: &RadrootsCliSandbox, key: &str) -> PathBuf {
    let listing_file = sandbox.root().join(format!("{key}.toml"));
    let listing_file_arg = listing_file.to_string_lossy();
    let value = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "create",
        "--output",
        listing_file_arg.as_ref(),
        "--key",
        key,
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
    assert_eq!(value["operation_id"], "listing.create");
    listing_file
}

pub fn identity_public(seed: u8) -> RadrootsIdentityPublic {
    let secret = [seed; 32];
    RadrootsIdentity::from_secret_key_bytes(&secret)
        .expect("fixture identity")
        .to_public()
}

pub fn make_listing_publishable(path: &Path, farm_d_tag: &str) {
    let raw = fs::read_to_string(path).expect("listing draft");
    let mut seller_pubkey_present = false;
    let patched = raw
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("seller_pubkey =") {
                seller_pubkey_present = !trimmed.ends_with("\"\"");
                line.to_owned()
            } else if trimmed.starts_with("farm_d_tag =") {
                format!("{}farm_d_tag = \"{}\"", line_indent(line), farm_d_tag)
            } else if trimmed.starts_with("method =") {
                format!("{}method = \"pickup\"", line_indent(line))
            } else if trimmed.starts_with("primary =") {
                format!("{}primary = \"farmstand\"", line_indent(line))
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(seller_pubkey_present, "listing draft seller pubkey");
    fs::write(path, format!("{patched}\n")).expect("write listing draft");
}

pub fn shell_single_quoted(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
}

pub fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn write_public_identity_profile(
    sandbox: &RadrootsCliSandbox,
    name: &str,
    identity: &RadrootsIdentityPublic,
) -> PathBuf {
    let path = sandbox.root().join(format!("{name}.json"));
    fs::write(
        &path,
        serde_json::to_string_pretty(identity).expect("public identity json"),
    )
    .expect("write public identity");
    path
}

fn line_indent(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}
