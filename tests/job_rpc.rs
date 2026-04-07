use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use tempfile::tempdir;

fn job_rpc_command_in(workdir: &Path) -> Command {
    let mut command = Command::cargo_bin("radroots").expect("binary");
    command.current_dir(workdir);
    command.env("HOME", workdir.join("home"));
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
        "RADROOTS_IDENTITY_PATH",
        "RADROOTS_SIGNER",
        "RADROOTS_RELAYS",
        "RADROOTS_MYC_EXECUTABLE",
        "RADROOTS_RPC_URL",
        "RADROOTS_RPC_BEARER_TOKEN",
    ] {
        command.env_remove(key);
    }
    command
}

#[derive(Debug, Clone)]
struct MockRpcRequest {
    method: String,
    auth_header: Option<String>,
}

#[derive(Debug, Clone)]
struct MockRpcResponse {
    status_code: u16,
    body: Value,
}

impl MockRpcResponse {
    fn success(result: Value) -> Self {
        Self {
            status_code: 200,
            body: json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result,
            }),
        }
    }

    fn rpc_error(code: i64, message: &str) -> Self {
        Self {
            status_code: 200,
            body: json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {
                    "code": code,
                    "message": message,
                }
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
            handle.join().expect("join mock rpc server thread");
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
        serde_json::from_str(body).map_err(|error| format!("parse mock rpc json body: {error}"))?;
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
    let text = std::str::from_utf8(headers)
        .map_err(|error| format!("mock rpc header parse failed: {error}"))?;
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| format!("mock rpc content-length parse failed: {error}"));
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
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status_code,
        status_text(response.status_code),
        body.len(),
        body
    )
    .map_err(|error| format!("write mock rpc response: {error}"))?;
    stream
        .flush()
        .map_err(|error| format!("flush mock rpc response: {error}"))
}

fn status_text(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        401 => "Unauthorized",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn sample_bridge_status() -> Value {
    json!({
        "enabled": true,
        "ready": true,
        "auth_mode": "bearer_token",
        "signer_mode": "selectable_per_request",
        "default_signer_mode": "embedded_service_identity",
        "supported_signer_modes": ["embedded_service_identity", "nip46_session"],
        "available_nip46_signer_sessions": 2,
        "relay_count": 3,
        "job_status_retention": 32,
        "retained_jobs": 1,
        "accepted_jobs": 4,
        "published_jobs": 3,
        "failed_jobs": 1,
        "recovered_failed_jobs": 0,
        "methods": ["bridge.status", "bridge.job.list", "bridge.job.status", "nip46.session.list"]
    })
}

fn sample_job(job_id: &str, state: &str, terminal: bool, completed_at_unix: Option<u64>) -> Value {
    json!({
        "job_id": job_id,
        "command": "bridge.listing.publish",
        "status": state,
        "terminal": terminal,
        "recovered_after_restart": false,
        "requested_at_unix": 1_712_720_000,
        "completed_at_unix": completed_at_unix,
        "signer_mode": "embedded_service_identity",
        "event_id": "event-123",
        "event_addr": "30023:npub1seller:listing-123",
        "delivery_policy": "best_effort",
        "delivery_quorum": 2,
        "relay_count": 3,
        "acknowledged_relay_count": if terminal { 2 } else { 1 },
        "required_acknowledged_relay_count": 2,
        "attempt_count": if terminal { 2 } else { 1 },
        "relay_outcome_summary": if terminal { "published to 2 relays" } else { "awaiting quorum" },
        "attempt_summaries": if terminal {
            json!(["attempt 1: relay.one accepted", "attempt 2: relay.two accepted"])
        } else {
            json!(["attempt 1: relay.one accepted"])
        }
    })
}

#[test]
fn rpc_status_reports_bridge_ready_via_daemon_rpc() {
    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |method, auth_header| {
        recorded
            .lock()
            .expect("record requests")
            .push(MockRpcRequest {
                method: method.clone(),
                auth_header: auth_header.clone(),
            });
        match method.as_str() {
            "bridge.status" => MockRpcResponse::success(sample_bridge_status()),
            _ => MockRpcResponse::rpc_error(-32601, "method not found"),
        }
    });

    let dir = tempdir().expect("tempdir");
    let output = job_rpc_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "secret")
        .args(["--json", "rpc", "status"])
        .output()
        .expect("run rpc status");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["url"], server.url());
    assert_eq!(json["bridge_ready"], true);
    assert_eq!(json["retained_jobs"], 1);
    assert_eq!(json["session_surface_enabled"], true);

    let recorded = requests.lock().expect("recorded requests");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, "bridge.status");
    assert_eq!(recorded[0].auth_header.as_deref(), Some("Bearer secret"));
}

#[test]
fn rpc_sessions_ndjson_emits_public_session_entries() {
    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |method, auth_header| {
        recorded
            .lock()
            .expect("record requests")
            .push(MockRpcRequest {
                method: method.clone(),
                auth_header: auth_header.clone(),
            });
        match method.as_str() {
            "nip46.session.list" => MockRpcResponse::success(json!([
                {
                    "session_id": "session-1",
                    "role": "client",
                    "client_pubkey": "client-1",
                    "signer_pubkey": "signer-1",
                    "user_pubkey": "user-1",
                    "relays": ["wss://relay.one"],
                    "permissions": ["sign_event"],
                    "auth_required": false,
                    "authorized": true,
                    "expires_in_secs": 60
                },
                {
                    "session_id": "session-2",
                    "role": "admin",
                    "client_pubkey": "client-2",
                    "signer_pubkey": "signer-2",
                    "user_pubkey": null,
                    "relays": ["wss://relay.two", "wss://relay.three"],
                    "permissions": ["describe"],
                    "auth_required": true,
                    "authorized": false,
                    "expires_in_secs": null
                }
            ])),
            _ => MockRpcResponse::rpc_error(-32601, "method not found"),
        }
    });

    let dir = tempdir().expect("tempdir");
    let output = job_rpc_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .args(["--ndjson", "rpc", "sessions"])
        .output()
        .expect("run rpc sessions");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"session_id\":\"session-1\""));
    assert!(lines[1].contains("\"authorized\":false"));

    let recorded = requests.lock().expect("recorded requests");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, "nip46.session.list");
    assert_eq!(recorded[0].auth_header, None);
}

#[test]
fn job_commands_require_bridge_bearer_token() {
    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |method, auth_header| {
        recorded
            .lock()
            .expect("record requests")
            .push(MockRpcRequest {
                method,
                auth_header,
            });
        MockRpcResponse::rpc_error(-32601, "method not found")
    });

    let dir = tempdir().expect("tempdir");
    let output = job_rpc_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .args(["--json", "job", "ls"])
        .output()
        .expect("run job ls");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(output.stdout.as_slice()).expect("json output");
    assert_eq!(json["state"], "unconfigured");
    assert!(
        json["reason"]
            .as_str()
            .expect("reason")
            .contains("bridge bearer token is not configured")
    );
    assert!(requests.lock().expect("recorded requests").is_empty());
}

#[test]
fn job_ls_and_get_report_retained_bridge_jobs() {
    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let recorded = Arc::clone(&requests);
    let server = MockRpcServer::start(move |method, auth_header| {
        recorded
            .lock()
            .expect("record requests")
            .push(MockRpcRequest {
                method: method.clone(),
                auth_header: auth_header.clone(),
            });
        match method.as_str() {
            "bridge.job.list" => {
                MockRpcResponse::success(json!([sample_job("job-123", "publishing", false, None)]))
            }
            "bridge.job.status" => MockRpcResponse::success(sample_job(
                "job-123",
                "published",
                true,
                Some(1_712_720_030),
            )),
            _ => MockRpcResponse::rpc_error(-32601, "method not found"),
        }
    });

    let dir = tempdir().expect("tempdir");
    let list = job_rpc_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "secret")
        .args(["--json", "job", "ls"])
        .output()
        .expect("run job ls");
    assert!(list.status.success());
    let list_json: Value = serde_json::from_slice(list.stdout.as_slice()).expect("list json");
    assert_eq!(list_json["state"], "ready");
    assert_eq!(list_json["count"], 1);
    assert_eq!(list_json["jobs"][0]["id"], "job-123");
    assert_eq!(list_json["jobs"][0]["command"], "listing.publish");

    let get = job_rpc_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "secret")
        .args(["--json", "job", "get", "job-123"])
        .output()
        .expect("run job get");
    assert!(get.status.success());
    let get_json: Value = serde_json::from_slice(get.stdout.as_slice()).expect("get json");
    assert_eq!(get_json["state"], "ready");
    assert_eq!(get_json["job"]["id"], "job-123");
    assert_eq!(
        get_json["job"]["relay_outcome_summary"],
        "published to 2 relays"
    );

    let recorded = requests.lock().expect("recorded requests");
    assert_eq!(recorded.len(), 2);
    assert!(
        recorded
            .iter()
            .all(|request| request.auth_header.as_deref() == Some("Bearer secret"))
    );
}

#[test]
fn job_watch_ndjson_emits_one_frame_per_poll_until_terminal() {
    let sequence = Arc::new(Mutex::new(0_usize));
    let requests = Arc::new(Mutex::new(Vec::<MockRpcRequest>::new()));
    let observed = Arc::clone(&requests);
    let counter = Arc::clone(&sequence);
    let server = MockRpcServer::start(move |method, auth_header| {
        observed
            .lock()
            .expect("record requests")
            .push(MockRpcRequest {
                method: method.clone(),
                auth_header,
            });
        match method.as_str() {
            "bridge.job.status" => {
                let mut count = counter.lock().expect("watch count");
                *count += 1;
                if *count == 1 {
                    MockRpcResponse::success(sample_job("job-123", "publishing", false, None))
                } else {
                    MockRpcResponse::success(sample_job(
                        "job-123",
                        "published",
                        true,
                        Some(1_712_720_030),
                    ))
                }
            }
            _ => MockRpcResponse::rpc_error(-32601, "method not found"),
        }
    });

    let dir = tempdir().expect("tempdir");
    let output = job_rpc_command_in(dir.path())
        .env("RADROOTS_RPC_URL", server.url())
        .env("RADROOTS_RPC_BEARER_TOKEN", "secret")
        .args([
            "--ndjson",
            "job",
            "watch",
            "job-123",
            "--frames",
            "3",
            "--interval-ms",
            "1",
        ])
        .output()
        .expect("run job watch");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"sequence\":1"));
    assert!(lines[0].contains("\"state\":\"publishing\""));
    assert!(lines[1].contains("\"sequence\":2"));
    assert!(lines[1].contains("\"terminal\":true"));
}
