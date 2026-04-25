use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use assert_cmd::prelude::*;
use radroots_sql_core::{SqlExecutor, SqliteExecutor};
use serde_json::{Value, json};
use tempfile::tempdir;

const ORDER_SELLER_PUBKEY: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";
const ORDER_LISTING_ADDR: &str =
    "30402:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";
const ORDER_DRAFT_LISTING_ADDR: &str =
    "30403:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";

fn data_root(workdir: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        workdir.join("local").join("Radroots").join("data")
    } else {
        workdir.join("home").join(".radroots").join("data")
    }
}

fn order_command_in(workdir: &Path) -> Command {
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
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command.env("RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE", "false");
    command
}

fn write_user_config(workdir: &Path, contents: &str) {
    let config_dir = workdir.join("home/.radroots/config/apps/cli");
    fs::create_dir_all(&config_dir).expect("user config dir");
    fs::write(config_dir.join("config.toml"), contents).expect("write user config");
}

fn init_local_replica(workdir: &Path) {
    let init = order_command_in(workdir)
        .args(["local", "init"])
        .output()
        .expect("run local init");
    assert!(init.status.success());
}

fn seed_trade_product(workdir: &Path, product_id: &str, key: &str, listing_addr: Option<&str>) {
    let replica_db = data_root(workdir).join("apps/cli/replica/replica.sqlite");
    let executor = SqliteExecutor::open(&replica_db).expect("open replica db");
    let now = "2026-04-07T00:00:00.000Z";
    executor
        .exec(
            "INSERT INTO trade_product (id, created_at, updated_at, key, listing_addr, category, title, summary, process, lot, profile, year, qty_amt, qty_unit, qty_label, qty_avail, price_amt, price_currency, price_qty_amt, price_qty_unit, notes) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);",
            json!([
                product_id,
                now,
                now,
                key,
                listing_addr,
                "produce",
                "Pasture Eggs",
                "Fresh pasture-raised eggs",
                "fresh",
                "lot-a",
                "standard",
                2026,
                36,
                "each",
                "dozen",
                18,
                4.5,
                "USD",
                1,
                "each",
                Value::Null
            ])
            .to_string()
            .as_str(),
        )
        .expect("insert trade product");
}

fn assert_no_order_drafts(workdir: &Path) {
    let drafts_dir = data_root(workdir).join("apps/cli/orders/drafts");
    if !drafts_dir.exists() {
        return;
    }
    assert!(
        fs::read_dir(&drafts_dir)
            .expect("read drafts dir")
            .next()
            .is_none()
    );
}

fn run_order_lookup_failure(seed: impl FnOnce(&Path), expected_stderr: &str) {
    let dir = tempdir().expect("tempdir");
    seed(dir.path());

    let output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--bin",
            "bin-1",
            "--qty",
            "1",
        ])
        .output()
        .expect("run order new failure");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains(expected_stderr),
        "stderr did not contain `{expected_stderr}`: {stderr}"
    );
    assert_no_order_drafts(dir.path());
}

fn config_with_write_plane(extra: &str, url: &str) -> String {
    let mut rendered = String::new();
    if !extra.trim().is_empty() {
        rendered.push_str(extra.trim());
        rendered.push_str("\n\n");
    }
    rendered.push_str(
        format!(
            r#"[[capability_binding]]
capability = "write_plane.trade_jsonrpc"
provider = "radrootsd"
target_kind = "explicit_endpoint"
target = "{url}"
"#
        )
        .as_str(),
    );
    rendered
}

fn write_fake_myc(dir: &Path, script: &str) -> std::path::PathBuf {
    let path = dir.join("fake-myc");
    fs::write(&path, script).expect("write fake myc");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod fake myc");
    path
}

fn successful_status_script(payload_json: String) -> String {
    format!(
        "#!/bin/sh\nif [ \"$1\" != \"status\" ] || [ \"$2\" != \"--view\" ] || [ \"$3\" != \"full\" ]; then\n  echo \"unexpected args: $*\" >&2\n  exit 64\nfi\ncat <<'JSON'\n{payload_json}\nJSON\n"
    )
}

fn sample_myc_status_payload(
    account_id: &str,
    public_identity: &Value,
    connection_id: &str,
) -> Value {
    json!({
        "status": "healthy",
        "ready": true,
        "reasons": [],
        "signer_backend": {
            "local_signer": {
                "account_id": account_id,
                "public_identity": public_identity,
                "availability": "SecretBacked"
            },
            "remote_session_count": 1,
            "remote_sessions": [
                {
                    "connection_id": connection_id,
                    "signer_identity": public_identity,
                    "user_identity": public_identity,
                    "relays": ["wss://relay.one"],
                    "permissions": "sign_event"
                }
            ]
        }
    })
}

fn order_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("order test lock")
}

#[derive(Debug, Clone)]
struct MockRpcRequest {
    body: Value,
    method: String,
    auth_header: Option<String>,
}

#[derive(Debug, Clone)]
struct MockRpcResponse {
    body: Value,
}

impl MockRpcResponse {
    fn success(result: Value) -> Self {
        Self {
            body: serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result,
            }),
        }
    }
}

struct MockRpcServer {
    address: String,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockRpcServer {
    fn start<F>(handler: F) -> Self
    where
        F: Fn(Value, Option<String>) -> MockRpcResponse + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock rpc listener");
        listener
            .set_nonblocking(true)
            .expect("set mock rpc listener nonblocking");
        let address = listener
            .local_addr()
            .expect("mock rpc local addr")
            .to_string();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let handler: Arc<dyn Fn(Value, Option<String>) -> MockRpcResponse + Send + Sync> =
            Arc::new(handler);
        let handle = thread::spawn(move || {
            while !shutdown_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Ok(request) = read_request(&mut stream) {
                            let response =
                                handler(request.body.clone(), request.auth_header.clone());
                            let _ = write_response(&mut stream, &response);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            address,
            shutdown,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for MockRpcServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.address);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("join mock rpc thread");
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<MockRpcRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("set mock rpc read timeout: {error}"))?;

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut header_end = None;
    let mut content_length = 0_usize;

    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read mock rpc request: {error}"))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if header_end.is_none() {
            header_end = find_subslice(&buffer, b"\r\n\r\n").map(|index| index + 4);
            if let Some(end) = header_end {
                content_length = parse_content_length(&buffer[..end])?;
                if buffer.len() >= end + content_length {
                    break;
                }
            }
        } else if let Some(end) = header_end {
            if buffer.len() >= end + content_length {
                break;
            }
        }
    }

    let end = header_end.ok_or_else(|| "mock rpc request did not include headers".to_owned())?;
    let headers = std::str::from_utf8(&buffer[..end])
        .map_err(|error| format!("mock rpc headers were not utf-8: {error}"))?;
    let auth_header = parse_header(headers, "authorization");
    let body = std::str::from_utf8(&buffer[end..end + content_length])
        .map_err(|error| format!("mock rpc body was not utf-8: {error}"))?;
    let envelope: Value =
        serde_json::from_str(body).map_err(|error| format!("parse mock rpc body: {error}"))?;
    let method = envelope["method"]
        .as_str()
        .ok_or_else(|| "mock rpc body did not include method".to_owned())?
        .to_owned();

    Ok(MockRpcRequest {
        body: envelope,
        method,
        auth_header,
    })
}

fn parse_content_length(headers: &[u8]) -> Result<usize, String> {
    let text =
        std::str::from_utf8(headers).map_err(|error| format!("parse mock rpc headers: {error}"))?;
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| format!("parse content-length: {error}"));
            }
        }
    }
    Ok(0)
}

fn parse_header(headers: &str, wanted: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case(wanted) {
            Some(value.trim().to_owned())
        } else {
            None
        }
    })
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_response(stream: &mut TcpStream, response: &MockRpcResponse) -> Result<(), String> {
    let body = response.body.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .map_err(|error| format!("write mock rpc response: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("flush mock rpc response: {error}"))
}

fn sample_bridge_job(job_id: &str, state: &str, terminal: bool, signer_session_id: &str) -> Value {
    serde_json::json!({
        "job_id": job_id,
        "command": "bridge.order.request",
        "idempotency_key": "order-submit-1",
        "status": state,
        "terminal": terminal,
        "recovered_after_restart": false,
        "requested_at_unix": 1_712_720_000,
        "completed_at_unix": terminal.then_some(1_712_720_030),
        "signer_mode": "nip46_session",
        "signer_session_id": signer_session_id,
        "event_kind": 30420,
        "event_id": "evt_order_01",
        "event_addr": "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
        "delivery_policy": "best_effort",
        "delivery_quorum": 2,
        "relay_count": 2,
        "acknowledged_relay_count": if terminal { 2 } else { 1 },
        "required_acknowledged_relay_count": 2,
        "attempt_count": if terminal { 2 } else { 1 },
        "relay_outcome_summary": if terminal { "submitted to 2 relays" } else { "awaiting relay quorum" },
        "attempt_summaries": if terminal {
            serde_json::json!(["attempt 1: relay.one accepted", "attempt 2: relay.two accepted"])
        } else {
            serde_json::json!(["attempt 1: relay.one accepted"])
        }
    })
}

#[test]
fn order_new_creates_a_local_draft_with_selected_account_defaults() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let account_id = account_json["account"]["id"]
        .as_str()
        .expect("account id")
        .to_owned();
    let buyer_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("buyer pubkey");

    init_local_replica(dir.path());
    seed_trade_product(
        dir.path(),
        "00000000-0000-0000-0000-000000000901",
        "pasture-eggs",
        Some(ORDER_LISTING_ADDR),
    );

    let output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--bin",
            "bin-1",
            "--qty",
            "2",
        ])
        .output()
        .expect("run order new");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("order json");
    assert_eq!(json["state"], "draft_created");
    assert_eq!(json["buyer_account_id"], account_id);
    assert_eq!(json["buyer_pubkey"], buyer_pubkey);
    assert_eq!(json["listing_addr"], ORDER_LISTING_ADDR);
    assert_eq!(json["seller_pubkey"], ORDER_SELLER_PUBKEY);
    assert_eq!(json["ready_for_submit"], true);
    assert_eq!(json["items"][0]["bin_id"], "bin-1");
    assert_eq!(json["items"][0]["bin_count"], 2);

    let file = json["file"].as_str().expect("draft file");
    assert!(file.contains("/data/apps/cli/orders/drafts/ord_"));
    let contents = fs::read_to_string(file).expect("read order draft");
    assert!(contents.contains("kind = \"order_draft_v1\""));
    assert!(contents.contains("listing_lookup = \"pasture-eggs\""));
    assert!(contents.contains(&format!("listing_addr = \"{ORDER_LISTING_ADDR}\"")));
    assert!(contents.contains(&format!("seller_pubkey = \"{ORDER_SELLER_PUBKEY}\"")));
    assert!(contents.contains(&format!("buyer_account_id = \"{account_id}\"")));
}

#[test]
fn order_new_listing_lookup_failures_do_not_create_drafts() {
    let _guard = order_test_guard();

    run_order_lookup_failure(|_| {}, "requires local market data");
    run_order_lookup_failure(
        |workdir| {
            init_local_replica(workdir);
        },
        "is not available in the local replica",
    );
    run_order_lookup_failure(
        |workdir| {
            init_local_replica(workdir);
            seed_trade_product(
                workdir,
                "00000000-0000-0000-0000-000000000902",
                "pasture-eggs",
                None,
            );
        },
        "is missing a canonical listing address",
    );
    run_order_lookup_failure(
        |workdir| {
            init_local_replica(workdir);
            seed_trade_product(
                workdir,
                "00000000-0000-0000-0000-000000000903",
                "pasture-eggs",
                Some(ORDER_DRAFT_LISTING_ADDR),
            );
        },
        "must reference a public NIP-99 listing",
    );
    run_order_lookup_failure(
        |workdir| {
            init_local_replica(workdir);
            seed_trade_product(
                workdir,
                "00000000-0000-0000-0000-000000000904",
                "pasture-eggs",
                Some(ORDER_LISTING_ADDR),
            );
            seed_trade_product(
                workdir,
                "00000000-0000-0000-0000-000000000905",
                "pasture-eggs",
                Some(ORDER_LISTING_ADDR),
            );
        },
        "matched 2 local listings",
    );
}

#[test]
fn order_get_and_ls_read_local_drafts_and_report_missing() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let first = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run first order new");
    assert!(first.status.success());
    let first_json: Value = serde_json::from_slice(first.stdout.as_slice()).expect("first json");
    let first_order_id = first_json["order_id"].as_str().expect("first order id");

    let second = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "carrots",
            "--listing-addr",
            ORDER_LISTING_ADDR,
        ])
        .output()
        .expect("run second order new");
    assert!(second.status.success());
    let second_json: Value = serde_json::from_slice(second.stdout.as_slice()).expect("second json");
    let second_order_id = second_json["order_id"].as_str().expect("second order id");

    let get_output = order_command_in(dir.path())
        .args(["--json", "order", "get", first_order_id])
        .output()
        .expect("run order get");
    assert!(get_output.status.success());
    let get_json: Value = serde_json::from_slice(get_output.stdout.as_slice()).expect("get json");
    assert_eq!(get_json["state"], "ready");
    assert_eq!(get_json["order_id"], first_order_id);
    assert_eq!(get_json["listing_lookup"], "pasture-eggs");

    let missing_output = order_command_in(dir.path())
        .args(["--json", "order", "get", "ord_missing"])
        .output()
        .expect("run missing order get");
    assert!(missing_output.status.success());
    let missing_json: Value =
        serde_json::from_slice(missing_output.stdout.as_slice()).expect("missing json");
    assert_eq!(missing_json["state"], "missing");

    let human_list = order_command_in(dir.path())
        .args(["order", "ls"])
        .output()
        .expect("run human order ls");
    assert!(human_list.status.success());
    let human_text = String::from_utf8(human_list.stdout).expect("human text");
    assert!(human_text.contains("orders · 2 local drafts"));
    assert!(human_text.contains(first_order_id));
    assert!(human_text.contains(second_order_id));

    let ndjson_output = order_command_in(dir.path())
        .args(["--ndjson", "order", "ls"])
        .output()
        .expect("run ndjson order ls");
    assert!(ndjson_output.status.success());
    let ndjson = String::from_utf8(ndjson_output.stdout).expect("ndjson text");
    let lines = ndjson.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|line| line.contains(first_order_id)));
    assert!(lines.iter().any(|line| line.contains(second_order_id)));
}

#[test]
fn order_create_view_and_list_aliases_wrap_the_existing_draft_surfaces() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let create_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "create",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
            "--qty",
            "2",
        ])
        .output()
        .expect("run order create");
    assert!(create_output.status.success());
    let create_json: Value =
        serde_json::from_slice(create_output.stdout.as_slice()).expect("create json");
    let order_id = create_json["order_id"].as_str().expect("order id");
    assert_eq!(create_json["state"], "draft_created");
    assert_eq!(
        create_json["actions"][0],
        format!("radroots order view {order_id}")
    );

    let view_output = order_command_in(dir.path())
        .args(["--json", "order", "view", order_id])
        .output()
        .expect("run order view");
    assert!(view_output.status.success());
    let view_json: Value =
        serde_json::from_slice(view_output.stdout.as_slice()).expect("view json");
    assert_eq!(view_json["state"], "ready");
    assert_eq!(view_json["order_id"], order_id);

    let list_output = order_command_in(dir.path())
        .args(["--json", "order", "list"])
        .output()
        .expect("run order list");
    assert!(list_output.status.success());
    let list_json: Value =
        serde_json::from_slice(list_output.stdout.as_slice()).expect("list json");
    assert_eq!(list_json["count"], 1);
    assert_eq!(list_json["orders"][0]["id"], order_id);
}

#[test]
fn order_list_empty_prefers_the_create_follow_up() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let output = order_command_in(dir.path())
        .args(["--json", "order", "list"])
        .output()
        .expect("run empty order list");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("list json");
    assert_eq!(json["state"], "empty");
    assert_eq!(json["actions"][0], "radroots order create");
}

#[test]
fn order_get_surfaces_recorded_job_metadata_from_the_local_draft_store() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let drafts_dir = data_root(dir.path()).join("apps/cli/orders/drafts");
    fs::create_dir_all(&drafts_dir).expect("create drafts dir");
    let draft_path = drafts_dir.join("ord_AAAAAAAAAAAAAAAAAAAAAg.toml");
    fs::write(
        &draft_path,
        r#"version = 1
kind = "order_draft_v1"
listing_lookup = "fresh-eggs"
buyer_account_id = "acct_demo"

[order]
order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg"
listing_addr = "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg"
buyer_pubkey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
seller_pubkey = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"

[[order.items]]
bin_id = "bin-1"
bin_count = 2

[submission]
job_id = "job_order_01"
"#,
    )
    .expect("write order draft");

    let output = order_command_in(dir.path())
        .args(["--json", "order", "get", "ord_AAAAAAAAAAAAAAAAAAAAAg"])
        .output()
        .expect("run order get");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json");
    assert_eq!(json["state"], "submitted");
    assert_eq!(json["job"]["job_id"], "job_order_01");
    assert_eq!(json["job"]["state"], "recorded");
    assert_eq!(json["ready_for_submit"], false);
}

#[test]
fn order_submit_persists_submission_metadata_and_reports_job() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let new_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run order new");
    assert!(new_output.status.success());
    let new_json: Value = serde_json::from_slice(new_output.stdout.as_slice()).expect("new json");
    let order_id = new_json["order_id"].as_str().expect("order id");
    let file = new_json["file"].as_str().expect("file");
    let buyer_pubkey = new_json["buyer_pubkey"]
        .as_str()
        .expect("buyer pubkey")
        .to_owned();

    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, auth_header| {
        recorded
            .lock()
            .expect("recorded requests lock")
            .push(MockRpcRequest {
                body: body.clone(),
                method: body["method"].as_str().unwrap_or_default().to_owned(),
                auth_header,
            });
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_order_01",
                buyer_pubkey.as_str(),
                &["sign_event"],
                true
            )])),
            "bridge.order.request" => MockRpcResponse::success(serde_json::json!({
                "deduplicated": false,
                "job": sample_bridge_job("job_order_01", "accepted", false, "sess_order_01"),
            })),
            "bridge.job.status" => MockRpcResponse::success(sample_bridge_job(
                "job_order_01",
                "accepted",
                false,
                "sess_order_01",
            )),
            other => panic!("unexpected mock rpc method {other}"),
        }
    });
    write_user_config(
        dir.path(),
        config_with_write_plane("", server.url().as_str()).as_str(),
    );

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "test-token")
        .args([
            "--json",
            "order",
            "submit",
            order_id,
            "--idempotency-key",
            "order-submit-1",
            "--signer-session-id",
            "sess_order_01",
        ])
        .output()
        .expect("run order submit");
    assert!(submit_output.status.success());
    let submit_json: Value =
        serde_json::from_slice(submit_output.stdout.as_slice()).expect("submit json");
    assert_eq!(submit_json["state"], "accepted");
    assert_eq!(submit_json["signer_mode"], "nip46_session");
    assert_eq!(submit_json["signer_session_id"], "sess_order_01");
    assert_eq!(submit_json["job"]["job_id"], "job_order_01");
    assert_eq!(submit_json["job"]["command"], "order.submit");
    assert_eq!(submit_json["job"]["signer_mode"], "nip46_session");
    assert_eq!(submit_json["job"]["signer_session_id"], "sess_order_01");
    assert_eq!(submit_json["requested_signer_session_id"], "sess_order_01");
    assert_eq!(
        submit_json["job"]["requested_signer_session_id"],
        "sess_order_01"
    );

    let contents = fs::read_to_string(file).expect("read updated order draft");
    assert!(contents.contains("job_id = \"job_order_01\""));
    assert!(contents.contains("state = \"accepted\""));
    assert!(contents.contains("command = \"order.submit\""));

    let recorded_requests = requests.lock().expect("requests lock");
    assert!(
        recorded_requests
            .iter()
            .any(|request| request.method == "bridge.order.request")
    );
    assert!(
        recorded_requests
            .iter()
            .any(|request| request.method == "nip46.session.list")
    );
    let request = recorded_requests
        .iter()
        .find(|request| request.method == "bridge.order.request")
        .expect("bridge order request");
    assert_eq!(request.body["params"]["signer_session_id"], "sess_order_01");
    assert!(
        recorded_requests
            .iter()
            .any(|request| { request.auth_header.as_deref() == Some("Bearer test-token") })
    );
}

#[test]
fn order_submit_quiet_reports_submitted_order_id() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let new_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run order new");
    assert!(new_output.status.success());
    let new_json: Value = serde_json::from_slice(new_output.stdout.as_slice()).expect("new json");
    let order_id = new_json["order_id"].as_str().expect("order id");
    let buyer_pubkey = new_json["buyer_pubkey"]
        .as_str()
        .expect("buyer pubkey")
        .to_owned();

    let server = MockRpcServer::start(move |body, _auth_header| {
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_order_quiet_01",
                buyer_pubkey.as_str(),
                &["sign_event"],
                true
            )])),
            "bridge.order.request" => MockRpcResponse::success(serde_json::json!({
                "deduplicated": false,
                "job": sample_bridge_job("job_order_quiet_01", "accepted", false, "sess_order_quiet_01"),
            })),
            "bridge.job.status" => MockRpcResponse::success(sample_bridge_job(
                "job_order_quiet_01",
                "accepted",
                false,
                "sess_order_quiet_01",
            )),
            other => panic!("unexpected mock rpc method {other}"),
        }
    });
    write_user_config(
        dir.path(),
        config_with_write_plane("", server.url().as_str()).as_str(),
    );

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "quiet-token")
        .args([
            "--quiet",
            "order",
            "submit",
            order_id,
            "--signer-session-id",
            "sess_order_quiet_01",
        ])
        .output()
        .expect("run quiet order submit");
    assert!(submit_output.status.success());
    let stdout = String::from_utf8(submit_output.stdout).expect("utf8 stdout");
    assert_eq!(stdout.trim(), format!("Order submitted: {order_id}"));
}

#[test]
fn order_submit_watch_rejects_json_output() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let output = order_command_in(dir.path())
        .args(["--json", "order", "submit", "ord_demo", "--watch"])
        .output()
        .expect("run order submit watch json");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("`order submit --watch` only supports human output"));
}

#[test]
fn order_submit_watch_appends_human_watch_snapshots() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let buyer_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("buyer pubkey")
        .to_owned();

    let create_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "create",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run order create");
    assert!(create_output.status.success());
    let create_json: Value =
        serde_json::from_slice(create_output.stdout.as_slice()).expect("create json");
    let order_id = create_json["order_id"].as_str().expect("order id");

    let server = MockRpcServer::start(move |body, _auth_header| {
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_order_watch_01",
                buyer_pubkey.as_str(),
                &["sign_event"],
                true
            )])),
            "bridge.order.request" => MockRpcResponse::success(serde_json::json!({
                "deduplicated": false,
                "job": sample_bridge_job(
                    "job_order_watch_01",
                    "accepted",
                    false,
                    "sess_order_watch_01"
                ),
            })),
            "bridge.job.status" => MockRpcResponse::success(sample_bridge_job(
                "job_order_watch_01",
                "completed",
                true,
                "sess_order_watch_01",
            )),
            other => panic!("unexpected mock rpc method {other}"),
        }
    });
    write_user_config(
        dir.path(),
        config_with_write_plane("", server.url().as_str()).as_str(),
    );

    let output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "watch-token")
        .args([
            "order",
            "submit",
            order_id,
            "--watch",
            "--signer-session-id",
            "sess_order_watch_01",
        ])
        .output()
        .expect("run order submit watch");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        output.status.success(),
        "status: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    assert!(stdout.contains("Order submitted"));
    assert!(stdout.contains("Watching order"));
    assert!(stdout.contains(order_id));
    assert!(stdout.contains("Completed"));
    assert!(stdout.contains("submitted to 2 relays"));
    assert!(!stdout.contains("order ·"));
}

#[test]
fn order_watch_reports_job_frames_for_submitted_order() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let polls = Arc::new(Mutex::new(0usize));
    let watch_polls = Arc::clone(&polls);
    let server = MockRpcServer::start(move |body, _auth_header| {
        match body["method"].as_str().unwrap_or_default() {
            "bridge.job.status" => {
                let mut count = watch_polls.lock().expect("watch polls lock");
                *count += 1;
                if *count == 1 {
                    MockRpcResponse::success(sample_bridge_job(
                        "job_watch_01",
                        "accepted",
                        false,
                        "sess_order_01",
                    ))
                } else {
                    MockRpcResponse::success(sample_bridge_job(
                        "job_watch_01",
                        "completed",
                        true,
                        "sess_order_01",
                    ))
                }
            }
            other => panic!("unexpected mock rpc method {other}"),
        }
    });

    let drafts_dir = data_root(dir.path()).join("apps/cli/orders/drafts");
    fs::create_dir_all(&drafts_dir).expect("create drafts dir");
    fs::write(
        drafts_dir.join("ord_AAAAAAAAAAAAAAAAAAAAAg.toml"),
        r#"version = 1
kind = "order_draft_v1"
listing_lookup = "fresh-eggs"
buyer_account_id = "acct_demo"

[order]
order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg"
listing_addr = "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg"
buyer_pubkey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
seller_pubkey = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"

[[order.items]]
bin_id = "bin-1"
bin_count = 2

[submission]
job_id = "job_watch_01"
"#,
    )
    .expect("write watch draft");

    let output = order_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "watch-token")
        .args([
            "--json",
            "order",
            "watch",
            "ord_AAAAAAAAAAAAAAAAAAAAAg",
            "--frames",
            "2",
            "--interval-ms",
            "1",
        ])
        .output()
        .expect("run order watch");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("watch json");
    assert_eq!(json["state"], "completed");
    assert_eq!(json["frames"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["frames"][0]["state"], "accepted");
    assert_eq!(json["frames"][1]["state"], "completed");
    assert_eq!(json["frames"][0]["signer_mode"], "nip46_session");
    assert_eq!(json["frames"][0]["signer_session_id"], "sess_order_01");
    assert_eq!(json["frames"][1]["signer_mode"], "nip46_session");
    assert_eq!(json["frames"][1]["signer_session_id"], "sess_order_01");

    let human_polls = Arc::new(Mutex::new(0usize));
    let human_watch_polls = Arc::clone(&human_polls);
    let human_server = MockRpcServer::start(move |body, _auth_header| {
        match body["method"].as_str().unwrap_or_default() {
            "bridge.job.status" => {
                let mut count = human_watch_polls.lock().expect("watch polls lock");
                *count += 1;
                if *count == 1 {
                    MockRpcResponse::success(sample_bridge_job(
                        "job_watch_01",
                        "accepted",
                        false,
                        "sess_order_01",
                    ))
                } else {
                    MockRpcResponse::success(sample_bridge_job(
                        "job_watch_01",
                        "completed",
                        true,
                        "sess_order_01",
                    ))
                }
            }
            other => panic!("unexpected mock rpc method {other}"),
        }
    });

    let human_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_URL", human_server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "watch-token")
        .args([
            "order",
            "watch",
            "ord_AAAAAAAAAAAAAAAAAAAAAg",
            "--frames",
            "2",
            "--interval-ms",
            "1",
        ])
        .output()
        .expect("run human order watch");
    assert!(human_output.status.success());
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Watching order ord_AAAAAAAAAAAAAAAAAAAAAg"));
    assert!(stdout.contains("Accepted"));
    assert!(stdout.contains("Completed"));
    assert!(stdout.contains("Summary"));
    assert!(!stdout.contains("order ·"));
    assert!(!stdout.contains("\u{1b}"));
}

#[test]
fn order_submit_uses_myc_binding_before_resolving_daemon_signer_session() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let account_id = account_json["account"]["id"]
        .as_str()
        .expect("account id")
        .to_owned();
    let public_identity = account_json["public_identity"].clone();
    let buyer_pubkey = public_identity["public_key_hex"]
        .as_str()
        .expect("buyer pubkey")
        .to_owned();

    let myc = write_fake_myc(
        dir.path(),
        successful_status_script(
            sample_myc_status_payload(
                account_id.as_str(),
                &public_identity,
                "conn_order_binding_01",
            )
            .to_string(),
        )
        .as_str(),
    );

    let new_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
            "--qty",
            "2",
        ])
        .output()
        .expect("run order new");
    assert!(new_output.status.success());
    let new_json: Value = serde_json::from_slice(new_output.stdout.as_slice()).expect("new json");
    let order_id = new_json["order_id"].as_str().expect("order id");

    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let session_account_id = account_id.clone();
    let server = MockRpcServer::start(move |body, auth_header| {
        recorded
            .lock()
            .expect("recorded requests lock")
            .push(MockRpcRequest {
                body: body.clone(),
                method: body["method"].as_str().unwrap_or_default().to_owned(),
                auth_header,
            });
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => {
                MockRpcResponse::success(json!([sample_session_with_authority(
                    "sess_order_02",
                    buyer_pubkey.as_str(),
                    &["sign_event"],
                    true,
                    Some(session_account_id.as_str()),
                    Some("conn_order_binding_01")
                )]))
            }
            "bridge.order.request" => MockRpcResponse::success(serde_json::json!({
                "deduplicated": false,
                "job": sample_bridge_job("job_order_02", "accepted", false, "sess_order_02"),
            })),
            other => panic!("unexpected mock rpc method {other}"),
        }
    });
    write_user_config(
        dir.path(),
        config_with_write_plane(
            format!(
                r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
managed_account_ref = "{account_id}"
"#
            )
            .as_str(),
            server.url().as_str(),
        )
        .as_str(),
    );

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "test-token")
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            myc.to_str().expect("myc path"),
            "order",
            "submit",
            order_id,
        ])
        .output()
        .expect("run order submit");

    let stdout = String::from_utf8(submit_output.stdout.clone()).expect("stdout utf8");
    let stderr = String::from_utf8(submit_output.stderr.clone()).expect("stderr utf8");
    assert!(
        submit_output.status.success(),
        "status: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        submit_output.status.code()
    );
    let submit_json: Value =
        serde_json::from_slice(submit_output.stdout.as_slice()).expect("submit json");
    assert_eq!(submit_json["state"], "accepted");
    assert_eq!(submit_json["signer_mode"], "nip46_session");
    assert_eq!(submit_json["signer_session_id"], "sess_order_02");
    assert_eq!(submit_json["requested_signer_session_id"], Value::Null);

    let recorded_requests = requests.lock().expect("requests lock");
    assert!(
        recorded_requests
            .iter()
            .any(|request| request.method == "nip46.session.list")
    );
    let request = recorded_requests
        .iter()
        .find(|request| request.method == "bridge.order.request")
        .expect("bridge order request");
    assert_eq!(request.body["params"]["signer_session_id"], "sess_order_02");
    assert_eq!(
        request.body["params"]["signer_authority"]["provider_runtime_id"],
        "myc"
    );
    assert_eq!(
        request.body["params"]["signer_authority"]["account_identity_id"],
        account_id
    );
    assert_eq!(
        request.body["params"]["signer_authority"]["provider_signer_session_id"],
        "conn_order_binding_01"
    );
}

#[test]
fn order_submit_rejects_myc_binding_that_resolves_the_wrong_actor() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let new_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
            "--qty",
            "2",
        ])
        .output()
        .expect("run order new");
    assert!(new_output.status.success());
    let new_json: Value = serde_json::from_slice(new_output.stdout.as_slice()).expect("new json");
    let order_id = new_json["order_id"].as_str().expect("order id");

    let mismatch_account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run mismatch account new");
    assert!(mismatch_account_output.status.success());
    let mismatch_account_json: Value =
        serde_json::from_slice(mismatch_account_output.stdout.as_slice()).expect("mismatch json");
    let mismatch_account_id = mismatch_account_json["account"]["id"]
        .as_str()
        .expect("mismatch account id");
    let mismatch_public_identity = mismatch_account_json["public_identity"].clone();

    let myc = write_fake_myc(
        dir.path(),
        successful_status_script(
            sample_myc_status_payload(
                mismatch_account_id,
                &mismatch_public_identity,
                "conn_order_binding_02",
            )
            .to_string(),
        )
        .as_str(),
    );

    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, auth_header| {
        recorded
            .lock()
            .expect("recorded requests lock")
            .push(MockRpcRequest {
                body,
                method: "unexpected".to_owned(),
                auth_header,
            });
        panic!("daemon write path should not be reached");
    });
    write_user_config(
        dir.path(),
        config_with_write_plane(
            format!(
                r#"
[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "managed_instance"
target = "default"
managed_account_ref = "{mismatch_account_id}"
"#
            )
            .as_str(),
            server.url().as_str(),
        )
        .as_str(),
    );

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "test-token")
        .args([
            "--json",
            "--signer",
            "myc",
            "--myc-executable",
            myc.to_str().expect("myc path"),
            "order",
            "submit",
            order_id,
        ])
        .output()
        .expect("run order submit");

    assert_eq!(submit_output.status.code(), Some(3));
    let submit_json: Value =
        serde_json::from_slice(submit_output.stdout.as_slice()).expect("submit json");
    assert_eq!(submit_json["state"], "unconfigured");
    assert_eq!(submit_json["signer_mode"], "myc");
    assert!(submit_json["reason"].as_str().is_some_and(|value| {
        value.contains("configured myc signer binding resolves signer pubkey")
    }));
    assert!(requests.lock().expect("requests lock").is_empty());
}

#[test]
fn order_submit_without_unique_matching_signer_session_exits_unconfigured() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());
    let account_json: Value =
        serde_json::from_slice(account_output.stdout.as_slice()).expect("account json");
    let buyer_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("buyer pubkey")
        .to_owned();

    let new_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run order new");
    assert!(new_output.status.success());
    let new_json: Value = serde_json::from_slice(new_output.stdout.as_slice()).expect("new json");
    let order_id = new_json["order_id"].as_str().expect("order id");

    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, _auth_header| {
        recorded
            .lock()
            .expect("recorded requests lock")
            .push(MockRpcRequest {
                body: body.clone(),
                method: body["method"].as_str().unwrap_or_default().to_owned(),
                auth_header: None,
            });
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([
                sample_session(
                    "sess_order_01",
                    buyer_pubkey.as_str(),
                    &["sign_event"],
                    true
                ),
                sample_session(
                    "sess_order_02",
                    buyer_pubkey.as_str(),
                    &["sign_event"],
                    true
                )
            ])),
            other => panic!("unexpected mock rpc method {other}"),
        }
    });
    write_user_config(
        dir.path(),
        config_with_write_plane("", server.url().as_str()).as_str(),
    );

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "test-token")
        .args(["--json", "order", "submit", order_id])
        .output()
        .expect("run order submit");
    assert_eq!(submit_output.status.code(), Some(3));
    let submit_json: Value =
        serde_json::from_slice(submit_output.stdout.as_slice()).expect("submit json");
    assert_eq!(submit_json["state"], "unconfigured");
    assert!(
        submit_json["reason"]
            .as_str()
            .expect("reason")
            .contains("multiple authorized signer sessions matched buyer pubkey")
    );

    let recorded_requests = requests.lock().expect("requests lock");
    assert_eq!(recorded_requests.len(), 1);
    assert_eq!(recorded_requests[0].method, "nip46.session.list");
}

#[test]
fn order_submit_rejects_requested_session_that_mismatches_buyer_pubkey() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");

    let account_output = order_command_in(dir.path())
        .args(["--json", "account", "new"])
        .output()
        .expect("run account new");
    assert!(account_output.status.success());

    let new_output = order_command_in(dir.path())
        .args([
            "--json",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
        ])
        .output()
        .expect("run order new");
    assert!(new_output.status.success());
    let new_json: Value = serde_json::from_slice(new_output.stdout.as_slice()).expect("new json");
    let order_id = new_json["order_id"].as_str().expect("order id");

    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |body, _auth_header| {
        recorded
            .lock()
            .expect("recorded requests lock")
            .push(MockRpcRequest {
                body: body.clone(),
                method: body["method"].as_str().unwrap_or_default().to_owned(),
                auth_header: None,
            });
        match body["method"].as_str().unwrap_or_default() {
            "nip46.session.list" => MockRpcResponse::success(json!([sample_session(
                "sess_wrong_01",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                &["sign_event"],
                true
            )])),
            other => panic!("unexpected mock rpc method {other}"),
        }
    });
    write_user_config(
        dir.path(),
        config_with_write_plane("", server.url().as_str()).as_str(),
    );

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_BEARER_TOKEN", "test-token")
        .args([
            "--json",
            "order",
            "submit",
            order_id,
            "--signer-session-id",
            "sess_wrong_01",
        ])
        .output()
        .expect("run order submit");
    assert_eq!(submit_output.status.code(), Some(3));
    let submit_json: Value =
        serde_json::from_slice(submit_output.stdout.as_slice()).expect("submit json");
    assert_eq!(submit_json["state"], "unconfigured");
    assert!(
        submit_json["reason"]
            .as_str()
            .expect("reason")
            .contains("does not match buyer pubkey")
    );

    let recorded_requests = requests.lock().expect("requests lock");
    assert_eq!(recorded_requests.len(), 1);
    assert_eq!(recorded_requests[0].method, "nip46.session.list");
}

fn sample_session(
    session_id: &str,
    signer_pubkey: &str,
    permissions: &[&str],
    authorized: bool,
) -> Value {
    sample_session_with_authority(
        session_id,
        signer_pubkey,
        permissions,
        authorized,
        None,
        None,
    )
}

fn sample_session_with_authority(
    session_id: &str,
    signer_pubkey: &str,
    permissions: &[&str],
    authorized: bool,
    account_identity_id: Option<&str>,
    provider_signer_session_id: Option<&str>,
) -> Value {
    json!({
        "session_id": session_id,
        "role": "remote_signer",
        "client_pubkey": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "signer_pubkey": signer_pubkey,
        "user_pubkey": Value::Null,
        "relays": ["wss://relay.one"],
        "permissions": permissions,
        "auth_required": false,
        "authorized": authorized,
        "expires_in_secs": Value::Null,
        "signer_authority": account_identity_id.map(|account_identity_id| json!({
            "provider_runtime_id": "myc",
            "account_identity_id": account_identity_id,
            "provider_signer_session_id": provider_signer_session_id
        }))
    })
}

#[test]
fn order_history_lists_submitted_order_drafts() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let drafts_dir = data_root(dir.path()).join("apps/cli/orders/drafts");
    fs::create_dir_all(&drafts_dir).expect("create drafts dir");
    fs::write(
        drafts_dir.join("ord_AAAAAAAAAAAAAAAAAAAAAg.toml"),
        r#"version = 1
kind = "order_draft_v1"
listing_lookup = "fresh-eggs"
buyer_account_id = "acct_demo"

[order]
order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg"
listing_addr = "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg"
buyer_pubkey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
seller_pubkey = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"

[[order.items]]
bin_id = "bin-1"
bin_count = 2

[submission]
job_id = "job_order_01"
state = "accepted"
command = "order.submit"
submitted_at_unix = 1712720000
"#,
    )
    .expect("write history draft");

    let json_output = order_command_in(dir.path())
        .args(["--json", "order", "history"])
        .output()
        .expect("run order history json");
    assert!(json_output.status.success());
    let json: Value = serde_json::from_slice(json_output.stdout.as_slice()).expect("history json");
    assert_eq!(json["count"], 1);
    assert_eq!(json["orders"][0]["id"], "ord_AAAAAAAAAAAAAAAAAAAAAg");
    assert_eq!(json["orders"][0]["state"], "accepted");

    let ndjson_output = order_command_in(dir.path())
        .args(["--ndjson", "order", "history"])
        .output()
        .expect("run order history ndjson");
    assert!(ndjson_output.status.success());
    let ndjson = String::from_utf8(ndjson_output.stdout).expect("history ndjson");
    let lines = ndjson.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("ord_AAAAAAAAAAAAAAAAAAAAAg"));
}

#[test]
fn order_cancel_is_truthfully_narrowed_when_trade_chain_state_is_unavailable() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let drafts_dir = data_root(dir.path()).join("apps/cli/orders/drafts");
    fs::create_dir_all(&drafts_dir).expect("create drafts dir");
    fs::write(
        drafts_dir.join("ord_AAAAAAAAAAAAAAAAAAAAAg.toml"),
        r#"version = 1
kind = "order_draft_v1"
listing_lookup = "fresh-eggs"
buyer_account_id = "acct_demo"

[order]
order_id = "ord_AAAAAAAAAAAAAAAAAAAAAg"
listing_addr = "30402:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef:AAAAAAAAAAAAAAAAAAAAAg"
buyer_pubkey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
seller_pubkey = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"

[[order.items]]
bin_id = "bin-1"
bin_count = 2

[submission]
job_id = "job_order_01"
state = "accepted"
command = "order.submit"
"#,
    )
    .expect("write cancel draft");

    let output = order_command_in(dir.path())
        .args(["--json", "order", "cancel", "ord_AAAAAAAAAAAAAAAAAAAAAg"])
        .output()
        .expect("run order cancel");
    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("cancel json");
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .expect("cancel reason")
            .contains("trade-chain")
    );
}
