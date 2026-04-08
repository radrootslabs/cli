use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::tempdir;

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

fn order_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("order test lock")
}

#[derive(Debug, Clone)]
struct MockRpcRequest {
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
        F: Fn(String, Option<String>) -> MockRpcResponse + Send + Sync + 'static,
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
        let handler: Arc<dyn Fn(String, Option<String>) -> MockRpcResponse + Send + Sync> =
            Arc::new(handler);
        let handle = thread::spawn(move || {
            while !shutdown_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Ok(request) = read_request(&mut stream) {
                            let response =
                                handler(request.method.clone(), request.auth_header.clone());
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

fn sample_bridge_job(job_id: &str, state: &str, terminal: bool) -> Value {
    serde_json::json!({
        "job_id": job_id,
        "command": "bridge.order.request",
        "idempotency_key": "order-submit-1",
        "status": state,
        "terminal": terminal,
        "recovered_after_restart": false,
        "requested_at_unix": 1_712_720_000,
        "completed_at_unix": terminal.then_some(1_712_720_030),
        "signer_mode": "embedded_service_identity",
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
    let account_id = account_json["account"]["id"].as_str().expect("account id");
    let buyer_pubkey = account_json["public_identity"]["public_key_hex"]
        .as_str()
        .expect("buyer pubkey");

    let output = order_command_in(dir.path())
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
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("order json");
    assert_eq!(json["state"], "draft_created");
    assert_eq!(json["buyer_account_id"], account_id);
    assert_eq!(json["buyer_pubkey"], buyer_pubkey);
    assert_eq!(
        json["seller_pubkey"],
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    );
    assert_eq!(json["ready_for_submit"], true);
    assert_eq!(json["items"][0]["bin_id"], "bin-1");
    assert_eq!(json["items"][0]["bin_count"], 2);

    let file = json["file"].as_str().expect("draft file");
    assert!(file.contains("/data/apps/cli/orders/drafts/ord_"));
    let contents = fs::read_to_string(file).expect("read order draft");
    assert!(contents.contains("kind = \"order_draft_v1\""));
    assert!(contents.contains("listing_lookup = \"pasture-eggs\""));
    assert!(contents.contains(&format!("buyer_account_id = \"{account_id}\"")));
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
        .args(["--json", "order", "new", "--listing", "carrots"])
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
    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |method, auth_header| {
        recorded
            .lock()
            .expect("recorded requests lock")
            .push(MockRpcRequest {
                method: method.clone(),
                auth_header,
            });
        match method.as_str() {
            "bridge.order.request" => MockRpcResponse::success(serde_json::json!({
                "deduplicated": false,
                "job": sample_bridge_job("job_order_01", "accepted", false),
            })),
            "bridge.job.status" => {
                MockRpcResponse::success(sample_bridge_job("job_order_01", "accepted", false))
            }
            other => panic!("unexpected mock rpc method {other}"),
        }
    });

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

    let submit_output = order_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
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
    assert_eq!(submit_json["job"]["job_id"], "job_order_01");
    assert_eq!(submit_json["job"]["command"], "order.submit");
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
    assert!(recorded_requests
        .iter()
        .any(|request| request.method == "bridge.order.request"));
    assert!(recorded_requests
        .iter()
        .any(|request| { request.auth_header.as_deref() == Some("Bearer test-token") }));
}

#[test]
fn order_watch_reports_job_frames_for_submitted_order() {
    let _guard = order_test_guard();
    let dir = tempdir().expect("tempdir");
    let polls = Arc::new(Mutex::new(0usize));
    let watch_polls = Arc::clone(&polls);
    let server = MockRpcServer::start(move |method, _auth_header| match method.as_str() {
        "bridge.job.status" => {
            let mut count = watch_polls.lock().expect("watch polls lock");
            *count += 1;
            if *count == 1 {
                MockRpcResponse::success(sample_bridge_job("job_watch_01", "accepted", false))
            } else {
                MockRpcResponse::success(sample_bridge_job("job_watch_01", "completed", true))
            }
        }
        other => panic!("unexpected mock rpc method {other}"),
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
    assert!(json["reason"]
        .as_str()
        .expect("cancel reason")
        .contains("trade-chain"));
}
