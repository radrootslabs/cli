#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

use assert_cmd::prelude::*;
use radroots_events::RadrootsNostrEvent;
use radroots_events::ids::RadrootsListingAddress;
use radroots_events::kinds::{KIND_FARM, KIND_LISTING};
use radroots_identity::{RadrootsIdentity, RadrootsIdentityPublic};
use radroots_local_events::{
    LocalEventRecord, LocalEventRecordInput, LocalEventsStore, LocalRecordFamily,
    LocalRecordStatus, PublishOutboxStatus, RelayDeliveryEvidence, SourceRuntime,
    canonical_relay_set_fingerprint,
};
use radroots_protected_store::RadrootsProtectedFileSecretVault;
use radroots_replica_sync::{RadrootsReplicaIngestOutcome, radroots_replica_ingest_event};
use radroots_secret_vault::RadrootsSecretVault;
use radroots_sql_core::{SqlExecutor, SqliteExecutor};
use serde_json::{Value, json};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static COMMAND_LOCK: Mutex<()> = Mutex::new(());
pub const ORDERABLE_LISTING_RELAY: &str = "ws://127.0.0.1:9";

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

    pub fn replica_db_path(&self) -> PathBuf {
        self.root
            .path()
            .join("data/apps/cli/replica/replica.sqlite")
    }

    pub fn local_events_db_path(&self) -> PathBuf {
        self.root
            .path()
            .join("data/shared/local_events/local_events.sqlite")
    }

    pub fn local_event_records(&self) -> Vec<LocalEventRecord> {
        let path = self.local_events_db_path();
        if !path.exists() {
            return Vec::new();
        }
        let executor = SqliteExecutor::open(path).expect("open local events db");
        let store = LocalEventsStore::new(executor);
        store.migrate_up().expect("migrate local events db");
        store
            .list_records_after_seq(0, 200)
            .expect("list local event records")
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
        command.env("RADROOTS_CLI_ACCOUNT_SECRET_BACKEND", "encrypted_file");
        command.env("RADROOTS_CLI_ACCOUNT_SECRET_FALLBACK", "none");
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
    let (seller_pubkey, listing_id) = listing_addr_parts(listing_addr);
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
    seed_orderable_listing_signed_event(sandbox, &event, listing_addr);
    event_id
}

fn seed_orderable_listing_signed_event(
    sandbox: &RadrootsCliSandbox,
    event: &RadrootsNostrEvent,
    listing_addr: &str,
) {
    let database_path = sandbox.local_events_db_path();
    fs::create_dir_all(database_path.parent().expect("local events parent"))
        .expect("local events parent");
    let executor = SqliteExecutor::open(database_path).expect("open local events");
    let store = LocalEventsStore::new(executor);
    store.migrate_up().expect("migrate local events");
    let delivery = RelayDeliveryEvidence::acknowledged(
        [ORDERABLE_LISTING_RELAY],
        [ORDERABLE_LISTING_RELAY],
        [ORDERABLE_LISTING_RELAY],
        Vec::new(),
    )
    .expect("listing relay delivery evidence");
    store
        .append_record(&LocalEventRecordInput {
            record_id: format!("test:signed_listing:{}", event.id),
            family: LocalRecordFamily::SignedEvent,
            status: LocalRecordStatus::Published,
            source_runtime: SourceRuntime::Cli,
            created_at_ms: 1_779_000_001_000,
            inserted_at_ms: 1_779_000_001_000,
            owner_account_id: None,
            owner_pubkey: Some(event.author.clone()),
            farm_id: None,
            listing_addr: Some(listing_addr.to_owned()),
            local_work_json: None,
            event_id: Some(event.id.clone()),
            event_kind: Some(i64::from(event.kind)),
            event_pubkey: Some(event.author.clone()),
            event_created_at: Some(i64::try_from(event.created_at).expect("event created_at")),
            event_tags_json: Some(json!(event.tags)),
            event_content: Some(event.content.clone()),
            event_sig: Some(event.sig.clone()),
            raw_event_json: Some(json!(event)),
            outbox_status: PublishOutboxStatus::Acknowledged,
            relay_set_fingerprint: canonical_relay_set_fingerprint([ORDERABLE_LISTING_RELAY]),
            relay_delivery_json: Some(delivery.to_json_value().expect("delivery json")),
        })
        .expect("append listing signed event record");
}

pub fn remove_orderable_listing(sandbox: &RadrootsCliSandbox, listing_addr: &str) {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica db");
    let params = serde_json::to_string(&vec![listing_addr]).expect("delete listing params");
    executor
        .exec(
            "DELETE FROM trade_product WHERE listing_addr = ?;",
            params.as_str(),
        )
        .expect("delete listing row");
}

pub fn update_orderable_listing_available_amount(
    sandbox: &RadrootsCliSandbox,
    listing_addr: &str,
    available_amount: i64,
) {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica db");
    let params = serde_json::to_string(&serde_json::json!([available_amount, listing_addr]))
        .expect("update listing params");
    executor
        .exec(
            "UPDATE trade_product SET qty_avail = ? WHERE listing_addr = ?;",
            params.as_str(),
        )
        .expect("update listing available amount");
}

pub fn update_orderable_listing_primary_bin_id(
    sandbox: &RadrootsCliSandbox,
    listing_addr: &str,
    primary_bin_id: Option<&str>,
) {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica db");
    let params = serde_json::to_string(&serde_json::json!([primary_bin_id, listing_addr]))
        .expect("update listing primary bin params");
    executor
        .exec(
            "UPDATE trade_product SET primary_bin_id = ? WHERE listing_addr = ?;",
            params.as_str(),
        )
        .expect("update listing primary bin");
}

pub fn duplicate_orderable_listing_row(sandbox: &RadrootsCliSandbox, listing_addr: &str) {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica db");
    let params = serde_json::to_string(&json!([
        "33333333-3333-3333-3333-333333333333",
        listing_addr
    ]))
    .expect("duplicate listing params");
    executor
        .exec(
            "INSERT INTO trade_product (id, created_at, updated_at, key, category, title, summary, process, lot, profile, year, qty_amt, qty_unit, qty_label, qty_avail, price_amt, price_currency, price_qty_amt, price_qty_unit, notes, listing_addr, primary_bin_id, qty_amt_exact, price_amt_exact, price_qty_amt_exact, verified_primary_bin_id) SELECT ?, created_at, updated_at, key, category, title, summary, process, lot, profile, year, qty_amt, qty_unit, qty_label, qty_avail, price_amt, price_currency, price_qty_amt, price_qty_unit, notes, listing_addr, primary_bin_id, qty_amt_exact, price_amt_exact, price_qty_amt_exact, verified_primary_bin_id FROM trade_product WHERE listing_addr = ?;",
            params.as_str(),
        )
        .expect("duplicate listing row");
}

pub fn replace_latest_listing_event_id(
    sandbox: &RadrootsCliSandbox,
    listing_addr: &str,
    event_id: &str,
) {
    let (seller_pubkey, listing_id) = listing_addr_parts(listing_addr);
    let key = format!("{KIND_LISTING}:{seller_pubkey}:{listing_id}");
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica db");
    let params = serde_json::to_string(&vec![event_id, key.as_str()]).expect("update params");
    executor
        .exec(
            "UPDATE nostr_event_head SET last_event_id = ? WHERE key = ?;",
            params.as_str(),
        )
        .expect("update latest listing event id");
}

fn listing_addr_parts(listing_addr: &str) -> (String, String) {
    let parsed = RadrootsListingAddress::parse(listing_addr).expect("listing addr");
    let (_, rest) = parsed.as_str().split_once(':').expect("listing addr kind");
    let (seller_pubkey, listing_id) = rest.split_once(':').expect("listing addr parts");
    (seller_pubkey.to_owned(), listing_id.to_owned())
}

pub fn create_listing_draft(sandbox: &RadrootsCliSandbox, key: &str) -> PathBuf {
    let accounts = sandbox.json_success(&["--format", "json", "account", "list"]);
    if accounts["result"]["count"].as_u64().unwrap_or_default() == 0 {
        sandbox.json_success(&["--format", "json", "account", "create"]);
    }
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
    identity_secret(seed).to_public()
}

pub fn identity_secret(seed: u8) -> RadrootsIdentity {
    let secret = [seed; 32];
    RadrootsIdentity::from_secret_key_bytes(&secret).expect("fixture identity")
}

pub fn store_test_session_secret(sandbox: &RadrootsCliSandbox, slot: &str, secret: &str) {
    let vault =
        RadrootsProtectedFileSecretVault::new(sandbox.root().join("secrets/shared/accounts"));
    vault
        .store_secret(slot, secret)
        .expect("store test session secret");
}

pub fn make_listing_publishable(path: &Path, farm_d_tag: &str) {
    let raw = fs::read_to_string(path).expect("listing draft");
    let mut seller_pubkey_present = false;
    let mut in_seller_actor = false;
    let patched = raw
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') {
                in_seller_actor = trimmed == "[seller_actor]";
            }
            if in_seller_actor && trimmed.starts_with("pubkey =") {
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

pub fn make_listing_publishable_with_seller(path: &Path, farm_d_tag: &str, seller_pubkey: &str) {
    let raw = fs::read_to_string(path).expect("listing draft");
    let mut seller_pubkey_field_present = false;
    let mut in_seller_actor = false;
    let patched = raw
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') {
                in_seller_actor = trimmed == "[seller_actor]";
            }
            if in_seller_actor && trimmed.starts_with("pubkey =") {
                seller_pubkey_field_present = true;
                format!("{}pubkey = \"{}\"", line_indent(line), seller_pubkey)
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
    assert!(
        seller_pubkey_field_present,
        "listing draft seller pubkey field"
    );
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

pub fn write_secret_identity_profile(
    sandbox: &RadrootsCliSandbox,
    name: &str,
    identity: &RadrootsIdentity,
) -> PathBuf {
    let path = sandbox.root().join(format!("{name}.json"));
    identity.save_json(&path).expect("write secret identity");
    path
}

fn line_indent(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}
