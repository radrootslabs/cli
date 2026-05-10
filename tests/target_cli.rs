mod support;

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use radroots_events::RadrootsNostrEventPtr;
use radroots_events::kinds::{KIND_FARM, KIND_PROFILE};
use radroots_events::trade::{
    RadrootsTradeOrderEconomics, RadrootsTradeOrderItem, RadrootsTradeOrderRequested,
};
use radroots_events_codec::trade::active_trade_order_request_event_build;
use radroots_nostr::prelude::{RadrootsNostrEvent, radroots_nostr_build_event};
use radroots_replica_db::{farm, farm_member_claim, migrations};
use radroots_replica_db_schema::farm::IFarmFields;
use radroots_replica_db_schema::farm_member_claim::IFarmMemberClaimFields;
use radroots_replica_sync::{
    RadrootsReplicaPendingPublishBatch, radroots_replica_pending_publish_batch,
};
use radroots_sql_core::SqliteExecutor;
use serde_json::Value;
use serde_json::json;

use support::{
    RadrootsCliSandbox, assert_contains, assert_no_daemon_runtime_reference,
    assert_no_removed_command_reference, create_listing_draft, identity_public, identity_secret,
    make_listing_publishable, make_listing_publishable_with_seller, ndjson_from_stdout, radroots,
    remove_orderable_listing, replace_latest_listing_event_id, seed_orderable_listing, toml_string,
    write_public_identity_profile,
};

const LISTING_ADDR: &str =
    "30402:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";
const SYNC_PUSH_FARM_D_TAG: &str = "AAAAAAAAAAAAAAAAAAAAAA";

struct JsonRpcRequest {
    headers: String,
    body: Value,
}

struct OneShotJsonRpcServer {
    endpoint: String,
    requests: Receiver<JsonRpcRequest>,
    handle: JoinHandle<()>,
}

impl OneShotJsonRpcServer {
    fn listing_publish() -> Self {
        Self::listing_publish_response(json!({
            "jsonrpc": "2.0",
            "id": "radroots-sdk-listing-publish",
            "result": {
                "deduplicated": false,
                "job": {
                    "job_id": "job_listing_publish_test",
                    "command": "bridge.listing.publish",
                    "status": "published",
                    "terminal": true,
                    "recovered_after_restart": false,
                    "signer_mode": "nip46",
                    "signer_session_id": "session_test",
                    "event_kind": 30402,
                    "event_id": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                    "event_addr": null,
                    "relay_count": 2,
                    "acknowledged_relay_count": 1
                }
            }
        }))
    }

    fn farm_publish() -> Self {
        Self::jsonrpc_sequence(vec![
            json!({
                "jsonrpc": "2.0",
                "id": "radroots-sdk-profile-publish",
                "result": {
                    "deduplicated": false,
                    "job": {
                        "job_id": "job_profile_publish_test",
                        "command": "bridge.profile.publish",
                        "status": "published",
                        "terminal": true,
                        "recovered_after_restart": false,
                        "signer_mode": "nip46",
                        "signer_session_id": "session_test",
                        "event_kind": 0,
                        "event_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "event_addr": null,
                        "relay_count": 2,
                        "acknowledged_relay_count": 2
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": "radroots-sdk-farm-publish",
                "result": {
                    "deduplicated": false,
                    "job": {
                        "job_id": "job_farm_publish_test",
                        "command": "bridge.farm.publish",
                        "status": "published",
                        "terminal": true,
                        "recovered_after_restart": false,
                        "signer_mode": "nip46",
                        "signer_session_id": "session_test",
                        "event_kind": KIND_FARM,
                        "event_id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "event_addr": format!("{KIND_FARM}:daemon_test:radrootsd-farm"),
                        "relay_count": 2,
                        "acknowledged_relay_count": 1
                    }
                }
            }),
        ])
    }

    fn listing_publish_error(message: &str) -> Self {
        Self::listing_publish_response(json!({
            "jsonrpc": "2.0",
            "id": "radroots-sdk-listing-publish",
            "error": {
                "code": -32000,
                "message": message
            }
        }))
    }

    fn listing_publish_response(response: Value) -> Self {
        Self::jsonrpc_sequence(vec![response])
    }

    fn jsonrpc_sequence(responses: Vec<Value>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake radrootsd");
        let endpoint = format!(
            "http://{}/jsonrpc",
            listener.local_addr().expect("fake radrootsd addr")
        );
        let (tx, requests) = mpsc::channel();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept fake radrootsd request");
                let request = read_jsonrpc_request(&mut stream);
                tx.send(request).expect("send fake radrootsd request");
                let response = response.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .expect("write fake radrootsd response");
            }
        });
        Self {
            endpoint,
            requests,
            handle,
        }
    }

    fn take_request(self) -> JsonRpcRequest {
        self.take_requests(1)
            .into_iter()
            .next()
            .expect("one fake radrootsd request")
    }

    fn take_requests(self, count: usize) -> Vec<JsonRpcRequest> {
        let request = (0..count)
            .map(|_| {
                self.requests
                    .recv_timeout(Duration::from_secs(5))
                    .expect("fake radrootsd request")
            })
            .collect::<Vec<_>>();
        self.handle.join().expect("fake radrootsd join");
        request
    }
}

fn read_jsonrpc_request(stream: &mut TcpStream) -> JsonRpcRequest {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer).expect("read fake radrootsd");
        assert!(count > 0, "fake radrootsd request ended before headers");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(header_end) = find_header_end(&bytes) {
            let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
            let content_length = content_length(&headers);
            let body_start = header_end + 4;
            while bytes.len() < body_start + content_length {
                let count = stream.read(&mut buffer).expect("read fake radrootsd body");
                assert!(count > 0, "fake radrootsd request ended before body");
                bytes.extend_from_slice(&buffer[..count]);
            }
            let body = serde_json::from_slice(&bytes[body_start..body_start + content_length])
                .expect("fake radrootsd json body");
            return JsonRpcRequest { headers, body };
        }
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &str) -> usize {
    headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().expect("content length"))
        })
        .expect("content-length header")
}

struct RelayPublishServer {
    endpoint: String,
    requests: Receiver<Value>,
    handle: JoinHandle<()>,
}

impl RelayPublishServer {
    fn with_publish_outcomes(outcomes: Vec<(bool, &'static str)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind relay");
        let endpoint = format!("ws://{}", listener.local_addr().expect("relay addr"));
        let (tx, requests) = mpsc::channel();
        let handle = thread::spawn(move || {
            for (accepted, reason) in outcomes {
                let (stream, _) = listener.accept().expect("accept relay connection");
                handle_relay_publish_connection(stream, accepted, reason, &tx);
            }
        });

        Self {
            endpoint,
            requests,
            handle,
        }
    }

    fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    fn take_requests(self, count: usize) -> Vec<Value> {
        let requests = (0..count)
            .map(|_| {
                self.requests
                    .recv_timeout(Duration::from_secs(5))
                    .expect("relay publish request")
            })
            .collect::<Vec<_>>();
        self.handle.join().expect("relay server join");
        requests
    }
}

struct RelayFetchServer {
    endpoint: String,
    handle: JoinHandle<()>,
}

impl RelayFetchServer {
    fn with_events(events: Vec<RadrootsNostrEvent>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind relay fetch");
        let endpoint = format!("ws://{}", listener.local_addr().expect("relay fetch addr"));
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept relay fetch connection");
            handle_relay_fetch_connection(stream, events);
        });
        Self { endpoint, handle }
    }

    fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    fn join(self) {
        self.handle.join().expect("relay fetch server join");
    }
}

fn handle_relay_fetch_connection(stream: TcpStream, events: Vec<RadrootsNostrEvent>) {
    let mut websocket = tungstenite::accept(stream).expect("accept fetch websocket");
    let subscription_id = read_relay_req_subscription_id(&mut websocket);
    for event in events {
        websocket
            .send(tungstenite::Message::Text(
                json!(["EVENT", subscription_id, event]).to_string().into(),
            ))
            .expect("relay event send");
    }
    websocket
        .send(tungstenite::Message::Text(
            json!(["EOSE", subscription_id]).to_string().into(),
        ))
        .expect("relay eose send");
}

fn read_relay_req_subscription_id(websocket: &mut tungstenite::WebSocket<TcpStream>) -> String {
    loop {
        let message = websocket.read().expect("relay req message");
        if !message.is_text() {
            continue;
        }
        let value: Value =
            serde_json::from_str(message.to_text().expect("relay req text")).expect("relay json");
        if value.get(0).and_then(Value::as_str) == Some("REQ") {
            return value
                .get(1)
                .and_then(Value::as_str)
                .expect("subscription id")
                .to_owned();
        }
    }
}

fn handle_relay_publish_connection(
    stream: TcpStream,
    accepted: bool,
    reason: &str,
    tx: &mpsc::Sender<Value>,
) {
    let mut websocket = tungstenite::accept(stream).expect("accept websocket");
    let event = read_relay_event_message(&mut websocket);
    let event_id = event["id"].as_str().expect("event id").to_owned();
    tx.send(event).expect("relay request send");
    websocket
        .send(tungstenite::Message::Text(
            json!(["OK", event_id, accepted, reason]).to_string().into(),
        ))
        .expect("relay ok send");
}

fn read_relay_event_message(websocket: &mut tungstenite::WebSocket<TcpStream>) -> Value {
    loop {
        let message = websocket.read().expect("relay message");
        if !message.is_text() {
            continue;
        }
        let value: Value =
            serde_json::from_str(message.to_text().expect("relay text")).expect("relay json");
        if value.get(0).and_then(Value::as_str) == Some("EVENT") {
            return value.get(1).cloned().expect("relay event payload");
        }
    }
}

fn seed_sync_push_farm(sandbox: &RadrootsCliSandbox, d_tag: &str, pubkey: &str) {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica");
    migrations::run_all_up(&executor).expect("replica migrations");
    farm::create(
        &executor,
        &IFarmFields {
            d_tag: d_tag.to_owned(),
            pubkey: pubkey.to_owned(),
            name: "Sync Push Farm".to_owned(),
            about: Some("sync push process fixture".to_owned()),
            website: None,
            picture: None,
            banner: None,
            location_primary: None,
            location_city: None,
            location_region: None,
            location_country: None,
        },
    )
    .expect("seed sync push farm");
}

fn seed_sync_push_member_claim(
    sandbox: &RadrootsCliSandbox,
    member_pubkey: &str,
    farm_pubkey: &str,
) {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica");
    migrations::run_all_up(&executor).expect("replica migrations");
    farm_member_claim::create(
        &executor,
        &IFarmMemberClaimFields {
            member_pubkey: member_pubkey.to_owned(),
            farm_pubkey: farm_pubkey.to_owned(),
        },
    )
    .expect("seed sync push member claim");
}

fn sync_push_pending_batch(sandbox: &RadrootsCliSandbox) -> RadrootsReplicaPendingPublishBatch {
    let executor = SqliteExecutor::open(sandbox.replica_db_path()).expect("open replica");
    radroots_replica_pending_publish_batch(&executor).expect("sync push pending batch")
}

#[test]
fn root_help_exposes_only_target_namespaces() {
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
        "runtime", "job", "message", "approval", "agent",
    ] {
        assert!(
            !help_lists(&stdout, removed),
            "root help should not contain `{removed}`"
        );
    }
}

#[test]
fn root_help_explains_publish_modes() {
    let output = radroots().arg("--help").output().expect("run root help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.contains("nostr_relay uses direct relay publish"));
    assert!(stdout.contains("radrootsd uses daemon-backed publish"));
    assert!(stdout.contains("Relay mode never silently falls back"));
    assert!(stdout.contains("Inspect local readiness and mode-specific recovery steps"));
    assert!(
        stdout
            .contains("Select nostr_relay direct relay publish or radrootsd daemon-backed publish")
    );
}

fn help_lists(stdout: &str, command: &str) -> bool {
    stdout.lines().any(|line| {
        let line = line.trim_start();
        line == command || line.starts_with(&format!("{command} "))
    })
}

fn assert_public_signer_session_binding_message(value: &Value) {
    let message = value["errors"][0]["message"]
        .as_str()
        .expect("error message");
    assert!(message.contains("signer.remote_nip46"));
    assert!(message.contains("signer_session_ref"));
    assert!(
        !message.contains("signer_session_id"),
        "public CLI message should not reference unavailable explicit session input: {message}"
    );
}

fn assert_rpc_bearer_token_next_action(actions: &Value) {
    let action = actions
        .as_array()
        .expect("next actions")
        .iter()
        .find(|action| action["env_var"] == "RADROOTS_RPC_BEARER_TOKEN")
        .expect("rpc bearer token next action");

    assert_eq!(action["kind"], "operator_config");
    assert_eq!(action["label"], "configure rpc bearer token");
    assert_eq!(action["command"], Value::Null);
    assert_eq!(action["description"], "configure RADROOTS_RPC_BEARER_TOKEN");
}

fn assert_signer_session_next_action(actions: &Value) {
    let action = actions
        .as_array()
        .expect("next actions")
        .iter()
        .find(|action| action["config_key"] == "signer.remote_nip46.signer_session_ref")
        .expect("signer session next action");

    assert_eq!(action["kind"], "operator_config");
    assert_eq!(action["label"], "configure signer session binding");
    assert_eq!(action["command"], Value::Null);
    assert_eq!(
        action["description"],
        "configure signer.remote_nip46 signer_session_ref"
    );
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
fn config_get_exposes_resolved_publish_state() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.write_app_config("[publish]\nmode = \"radrootsd\"\n");

    let value = sandbox.json_success(&["--format", "json", "config", "get"]);

    assert_eq!(value["operation_id"], "config.get");
    assert_eq!(value["result"]["publish"]["mode"], "radrootsd");
    assert_eq!(
        value["result"]["publish"]["source"],
        "user config · local first"
    );
    assert_eq!(value["result"]["publish"]["transport_family"], "radrootsd");
    assert_eq!(value["result"]["publish"]["state"], "unconfigured");
    assert_eq!(value["result"]["publish"]["executable"], false);
    assert_contains(&value["result"]["publish"]["reason"], "bridge bearer token");
    assert_eq!(
        value["result"]["publish"]["provider"]["provider_runtime_id"],
        "radrootsd"
    );
    assert_eq!(
        value["result"]["write_plane"]["provider_runtime_id"],
        "radrootsd"
    );
    assert_eq!(
        value["result"]["write_plane"]["binding_model"],
        "radrootsd_bridge_publish"
    );
    assert_eq!(value["result"]["write_plane"]["state"], "unconfigured");
    assert_eq!(
        value["result"]["write_plane"]["bridge_auth_configured"],
        false
    );
    assert_eq!(value["result"]["rpc"]["bridge_auth_configured"], false);
    assert_eq!(
        value["result"]["actions"][0],
        "configure RADROOTS_RPC_BEARER_TOKEN"
    );
    assert_eq!(
        value["result"]["actions"][1],
        "configure signer.remote_nip46 signer_session_ref"
    );
    assert_rpc_bearer_token_next_action(&value["next_actions"]);
    assert_signer_session_next_action(&value["next_actions"]);
}

#[test]
fn config_get_radrootsd_missing_signer_binding_mirrors_operator_next_action() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.write_app_config("[publish]\nmode = \"radrootsd\"\n");

    let mut command = sandbox.command();
    command
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args(["--format", "json", "config", "get"]);
    let output = command.output().expect("run config get");
    let value: Value = serde_json::from_slice(&output.stdout).expect("json output");

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "config.get");
    assert_eq!(value["result"]["publish"]["mode"], "radrootsd");
    assert_eq!(value["result"]["publish"]["state"], "unconfigured");
    assert_eq!(
        value["result"]["actions"][0],
        "configure signer.remote_nip46 signer_session_ref"
    );
    assert_eq!(
        value["next_actions"]
            .as_array()
            .expect("next actions")
            .len(),
        1
    );
    assert_signer_session_next_action(&value["next_actions"]);
}

#[test]
fn config_get_marks_radrootsd_listing_publish_ready_with_bridge_auth_and_session_binding() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.write_app_config(
        r#"[publish]
mode = "radrootsd"

[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_ready"
"#,
    );

    let mut command = sandbox.command();
    command
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args(["--format", "json", "config", "get"]);
    let output = command.output().expect("run config get");
    let value: Value = serde_json::from_slice(&output.stdout).expect("json output");

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "config.get");
    assert_eq!(value["result"]["publish"]["mode"], "radrootsd");
    assert_eq!(value["result"]["publish"]["relay"]["ready"], false);
    assert_eq!(value["result"]["publish"]["state"], "ready");
    assert_eq!(value["result"]["publish"]["executable"], true);
    assert_contains(
        &value["result"]["publish"]["reason"],
        "live bridge readiness is verified when publish runs",
    );
    assert_eq!(value["result"]["publish"]["provider"]["state"], "ready");
    assert_eq!(value["result"]["rpc"]["bridge_auth_configured"], true);
}

#[test]
fn config_get_distinguishes_relay_ready_from_missing_signed_write_account() {
    let sandbox = RadrootsCliSandbox::new();

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:19001",
        "config",
        "get",
    ]);

    assert_eq!(value["operation_id"], "config.get");
    assert_eq!(value["result"]["publish"]["mode"], "nostr_relay");
    assert_eq!(value["result"]["publish"]["relay"]["ready"], true);
    assert_eq!(value["result"]["publish"]["signed_write_required"], true);
    assert_eq!(value["result"]["publish"]["state"], "unconfigured");
    assert_eq!(value["result"]["publish"]["executable"], false);
    assert_contains(
        &value["result"]["publish"]["reason"],
        "write-capable local account",
    );
    assert_eq!(
        value["result"]["publish"]["provider"]["state"],
        "unconfigured"
    );
    assert_eq!(
        value["result"]["write_plane"]["provider_runtime_id"],
        "nostr_relay"
    );
    assert_eq!(
        value["result"]["write_plane"]["binding_model"],
        "direct_relay_publish"
    );
    assert_eq!(value["result"]["write_plane"]["state"], "unconfigured");
    assert_eq!(value["result"]["rpc"], Value::Null);
    assert_contains(
        &value["result"]["write_plane"]["detail"],
        "write-capable local account",
    );
    assert_eq!(value["result"]["actions"][0], "radroots account create");
    assert_eq!(
        value["next_actions"][0]["command"],
        "radroots account create"
    );
    assert_no_daemon_runtime_reference(
        &value,
        &[
            "--format",
            "json",
            "--relay",
            "ws://127.0.0.1:19001",
            "config",
            "get",
        ],
    );
}

#[test]
fn config_get_marks_relay_publish_ready_with_secret_backed_local_account() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:19002",
        "config",
        "get",
    ]);

    assert_eq!(value["result"]["publish"]["mode"], "nostr_relay");
    assert_eq!(value["result"]["publish"]["relay"]["ready"], true);
    assert_eq!(value["result"]["publish"]["signed_write_required"], true);
    assert_eq!(value["result"]["publish"]["state"], "ready");
    assert_eq!(value["result"]["publish"]["executable"], true);
    assert_eq!(value["result"]["publish"]["reason"], Value::Null);
    assert_eq!(value["result"]["publish"]["provider"]["state"], "ready");
}

#[test]
fn config_get_marks_relay_publish_unavailable_with_deferred_signer_mode() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    sandbox.write_app_config("[signer]\nmode = \"myc\"\n");

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:19003",
        "config",
        "get",
    ]);

    assert_eq!(value["result"]["publish"]["mode"], "nostr_relay");
    assert_eq!(value["result"]["publish"]["relay"]["ready"], true);
    assert_eq!(value["result"]["publish"]["signed_write_required"], true);
    assert_eq!(value["result"]["publish"]["state"], "unavailable");
    assert_eq!(value["result"]["publish"]["executable"], false);
    assert_contains(&value["result"]["publish"]["reason"], "signer mode `local`");
    assert_eq!(
        value["result"]["publish"]["provider"]["state"],
        "unavailable"
    );
}

#[test]
fn config_get_marks_relay_publish_unconfigured_with_watch_only_account() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(41);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "publish-readiness-watch-only", &public_identity);
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

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:19004",
        "config",
        "get",
    ]);

    assert_eq!(value["result"]["publish"]["relay"]["ready"], true);
    assert_eq!(value["result"]["publish"]["signed_write_required"], true);
    assert_eq!(value["result"]["publish"]["state"], "unconfigured");
    assert_eq!(value["result"]["publish"]["executable"], false);
    assert_contains(&value["result"]["publish"]["reason"], "watch_only");
}

#[test]
fn health_surfaces_publish_state_under_deferred_signer_mode() {
    let sandbox = RadrootsCliSandbox::new();
    let missing_myc = sandbox.root().join("bin/missing-myc");
    sandbox.write_app_config(&format!(
        "[publish]\nmode = \"radrootsd\"\n\n[signer]\nmode = \"myc\"\n\n[myc]\nexecutable = \"{}\"\n",
        toml_string(missing_myc.display().to_string().as_str())
    ));

    let value = sandbox.json_success(&["--format", "json", "health", "status", "get"]);

    assert_eq!(value["operation_id"], "health.status.get");
    assert_eq!(value["result"]["state"], "needs_attention");
    assert_eq!(value["result"]["publish"]["mode"], "radrootsd");
    assert_eq!(value["result"]["publish"]["executable"], false);
    assert_eq!(
        value["result"]["publish"]["provider"]["state"],
        "unconfigured"
    );
    assert_contains(&value["result"]["publish"]["reason"], "bridge bearer token");
    assert_eq!(value["result"]["actions"][0], "radroots store init");
    assert_eq!(value["result"]["actions"][1], "radroots account create");
    assert_eq!(
        value["result"]["actions"][2],
        "configure RADROOTS_RPC_BEARER_TOKEN"
    );
    assert_eq!(
        value["result"]["actions"][3],
        "configure signer.remote_nip46 signer_session_ref"
    );
    assert_eq!(value["next_actions"][0]["command"], "radroots store init");
    assert_eq!(
        value["next_actions"][1]["command"],
        "radroots account create"
    );
    assert_rpc_bearer_token_next_action(&value["next_actions"]);
    assert_signer_session_next_action(&value["next_actions"]);
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn health_status_distinguishes_relay_ready_from_missing_signed_write_account() {
    let sandbox = RadrootsCliSandbox::new();

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:19005",
        "health",
        "status",
        "get",
    ]);

    assert_eq!(value["operation_id"], "health.status.get");
    assert_eq!(value["result"]["state"], "needs_attention");
    assert_eq!(value["result"]["publish"]["relay"]["ready"], true);
    assert_eq!(value["result"]["publish"]["signed_write_required"], true);
    assert_eq!(value["result"]["publish"]["state"], "unconfigured");
    assert_eq!(value["result"]["publish"]["executable"], false);
    assert_contains(
        &value["result"]["publish"]["reason"],
        "write-capable local account",
    );
    assert_eq!(value["result"]["actions"][0], "radroots store init");
    assert_eq!(value["result"]["actions"][1], "radroots account create");
    assert_eq!(value["next_actions"][0]["command"], "radroots store init");
    assert_eq!(
        value["next_actions"][1]["command"],
        "radroots account create"
    );
}

#[test]
fn health_check_exposes_publish_readiness() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.write_app_config("[publish]\nmode = \"radrootsd\"\n");

    let value = sandbox.json_success(&["--format", "json", "health", "check", "run"]);

    assert_eq!(value["operation_id"], "health.check.run");
    assert_eq!(value["result"]["state"], "needs_attention");
    assert_eq!(value["result"]["checks"]["publish"]["mode"], "radrootsd");
    assert_eq!(
        value["result"]["checks"]["publish"]["state"],
        "unconfigured"
    );
    assert_eq!(value["result"]["checks"]["publish"]["executable"], false);
    assert_eq!(value["result"]["actions"][0], "radroots store init");
    assert_eq!(value["result"]["actions"][1], "radroots account create");
    assert_eq!(
        value["result"]["actions"][2],
        "configure RADROOTS_RPC_BEARER_TOKEN"
    );
    assert_eq!(
        value["result"]["actions"][3],
        "configure signer.remote_nip46 signer_session_ref"
    );
    assert_eq!(value["next_actions"][0]["command"], "radroots store init");
    assert_eq!(
        value["next_actions"][1]["command"],
        "radroots account create"
    );
    assert_rpc_bearer_token_next_action(&value["next_actions"]);
    assert_signer_session_next_action(&value["next_actions"]);
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn health_check_marks_relay_publish_ready_with_secret_backed_local_account() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "workspace", "init"]);
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let value = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:19006",
        "health",
        "check",
        "run",
    ]);

    assert_eq!(value["operation_id"], "health.check.run");
    assert_eq!(value["result"]["state"], "ready");
    assert_eq!(value["result"]["checks"]["publish"]["mode"], "nostr_relay");
    assert_eq!(value["result"]["checks"]["publish"]["state"], "ready");
    assert_eq!(value["result"]["checks"]["publish"]["executable"], true);
    assert_eq!(
        value["result"]["actions"]
            .as_array()
            .expect("actions")
            .len(),
        0
    );
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn farm_readiness_check_reports_mode_specific_publish_gates() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Ready Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);

    let (_, relay_value) = sandbox.json_output(&["--format", "json", "farm", "readiness", "check"]);
    let relay_detail = if relay_value["result"].is_null() {
        &relay_value["errors"][0]["detail"]
    } else {
        &relay_value["result"]
    };
    assert_eq!(relay_detail["publish_mode"], "nostr_relay");
    assert_eq!(relay_detail["publish_state"], "unconfigured");
    assert_eq!(relay_detail["publish_executable"], false);
    assert_eq!(relay_detail["missing"][0], "Configured relay");

    sandbox.write_app_config(
        r#"[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_test"
"#,
    );
    let output = sandbox
        .command()
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args([
            "--format",
            "json",
            "--publish-mode",
            "radrootsd",
            "farm",
            "readiness",
            "check",
        ])
        .output()
        .expect("run radrootsd farm readiness");
    let radrootsd_value: Value = serde_json::from_slice(&output.stdout).expect("json output");

    assert!(output.status.success());
    assert_eq!(radrootsd_value["operation_id"], "farm.readiness.check");
    assert_eq!(radrootsd_value["result"]["publish_mode"], "radrootsd");
    assert_eq!(radrootsd_value["result"]["publish_state"], "ready");
    assert_eq!(radrootsd_value["result"]["publish_executable"], true);
    assert_eq!(
        radrootsd_value["result"]["actions"][0],
        "radroots farm publish"
    );
}

#[test]
fn radrootsd_listing_publish_reaches_listing_router_without_relay_config() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Router Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let listing_file = create_listing_draft(&sandbox, "radrootsd-router");
    make_listing_publishable(
        &listing_file,
        farm["result"]["config"]["farm_d_tag"]
            .as_str()
            .expect("farm d tag"),
    );
    sandbox.write_app_config(
        r#"[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_test"
"#,
    );
    let server = OneShotJsonRpcServer::listing_publish();

    let output = sandbox
        .command()
        .env("RADROOTS_RPC_URL", &server.endpoint)
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args([
            "--format",
            "json",
            "--publish-mode",
            "radrootsd",
            "--approval-token",
            "approve",
            "--idempotency-key",
            "idem_listing",
            "listing",
            "publish",
            listing_file.to_string_lossy().as_ref(),
        ])
        .output()
        .expect("run radrootsd listing publish");
    let value: Value = serde_json::from_slice(&output.stdout).expect("json output");
    let request = server.take_request();

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(
        value["result"]["source"],
        "radrootsd publish transport · signer session"
    );
    assert_eq!(value["result"]["job_id"], "job_listing_publish_test");
    assert_eq!(value["result"]["job_status"], "published");
    assert_eq!(value["result"]["event_id"], "e".repeat(64));
    assert_eq!(
        value["result"]["event_addr"],
        value["result"]["listing_addr"]
    );
    assert!(value["result"]["listing_id"].is_string());
    assert!(value["result"]["seller_pubkey"].is_string());
    assert_eq!(value["result"]["signer_mode"], "nip46");
    assert_eq!(value["result"]["signer_session_id"], "session_test");
    assert_eq!(
        value["result"]["requested_signer_session_id"],
        "session_test"
    );
    assert_eq!(value["result"]["idempotency_key"], "idem_listing");
    assert_eq!(
        value["result"]["job"]["rpc_method"],
        "bridge.listing.publish"
    );
    assert_eq!(value["result"]["job"]["relay_count"], 2);
    assert_eq!(value["result"]["job"]["acknowledged_relay_count"], 1);
    assert_eq!(request.body["method"], "bridge.listing.publish");
    assert_eq!(request.body["params"]["kind"], 30402);
    assert_eq!(request.body["params"]["signer_session_id"], "session_test");
    assert_eq!(request.body["params"]["idempotency_key"], "idem_listing");
    assert!(
        request
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer bridge_test")
    );
}

#[test]
fn radrootsd_farm_publish_submits_profile_and_farm_without_relay_config() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Router Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let farm_d_tag = farm["result"]["config"]["farm_d_tag"]
        .as_str()
        .expect("farm d tag")
        .to_owned();
    sandbox.write_app_config(
        r#"[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_test"
"#,
    );
    let server = OneShotJsonRpcServer::farm_publish();

    let output = sandbox
        .command()
        .env("RADROOTS_RPC_URL", &server.endpoint)
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args([
            "--format",
            "json",
            "--publish-mode",
            "radrootsd",
            "--approval-token",
            "approve",
            "--idempotency-key",
            "idem_farm",
            "farm",
            "publish",
        ])
        .output()
        .expect("run radrootsd farm publish");
    let value: Value = serde_json::from_slice(&output.stdout).expect("json output");
    let requests = server.take_requests(2);
    let profile_request = &requests[0];
    let farm_request = &requests[1];

    assert!(output.status.success());
    assert_eq!(value["operation_id"], "farm.publish");
    assert_eq!(value["result"]["state"], "published");
    assert_eq!(
        value["result"]["source"],
        "radrootsd publish transport · signer session"
    );
    assert_eq!(
        value["result"]["requested_signer_session_id"],
        "session_test"
    );
    assert_eq!(
        value["result"]["profile"]["job_id"],
        "job_profile_publish_test"
    );
    assert_eq!(value["result"]["farm"]["job_id"], "job_farm_publish_test");
    assert_eq!(value["result"]["profile"]["event_kind"], KIND_PROFILE);
    assert_eq!(value["result"]["farm"]["event_kind"], KIND_FARM);
    assert_eq!(value["result"]["profile"]["event_id"], "a".repeat(64));
    assert_eq!(value["result"]["farm"]["event_id"], "b".repeat(64));
    assert_eq!(
        value["result"]["profile"]["rpc_method"],
        "bridge.profile.publish"
    );
    assert_eq!(value["result"]["farm"]["rpc_method"], "bridge.farm.publish");
    assert_eq!(profile_request.body["method"], "bridge.profile.publish");
    assert_eq!(farm_request.body["method"], "bridge.farm.publish");
    assert_eq!(profile_request.body["params"]["profile_type"], "farm");
    assert_eq!(
        profile_request.body["params"]["signer_session_id"],
        "session_test"
    );
    assert_eq!(
        farm_request.body["params"]["signer_session_id"],
        "session_test"
    );
    assert_eq!(
        profile_request.body["params"]["idempotency_key"],
        "idem_farm:profile"
    );
    assert_eq!(
        farm_request.body["params"]["idempotency_key"],
        "idem_farm:farm"
    );
    assert_eq!(farm_request.body["params"]["kind"], KIND_FARM);
    assert_eq!(farm_request.body["params"]["farm"]["d_tag"], farm_d_tag);
    assert!(
        profile_request
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer bridge_test")
    );

    let persisted = sandbox.json_success(&["--format", "json", "farm", "get"]);
    assert_eq!(
        persisted["result"]["document"]["publication"]["profile_state"],
        "published"
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["farm_state"],
        "published"
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["profile_event_id"],
        "a".repeat(64)
    );
    assert_eq!(
        persisted["result"]["document"]["publication"]["farm_event_id"],
        "b".repeat(64)
    );
}

#[test]
fn radrootsd_farm_publish_missing_signer_binding_points_to_capability_binding() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Binding Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    sandbox.write_app_config("[publish]\nmode = \"radrootsd\"\n");

    let dry_run_output = sandbox
        .command()
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args(["--format", "json", "--dry-run", "farm", "publish"])
        .output()
        .expect("run radrootsd farm publish dry-run");
    let dry_run: Value = serde_json::from_slice(&dry_run_output.stdout).expect("json output");

    assert!(!dry_run_output.status.success());
    assert_eq!(dry_run["operation_id"], "farm.publish");
    assert_eq!(dry_run["errors"][0]["code"], "signer_unconfigured");
    assert_public_signer_session_binding_message(&dry_run);

    let live_output = sandbox
        .command()
        .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
        .args([
            "--format",
            "json",
            "--approval-token",
            "approve",
            "farm",
            "publish",
        ])
        .output()
        .expect("run radrootsd farm publish");
    let live: Value = serde_json::from_slice(&live_output.stdout).expect("json output");

    assert!(!live_output.status.success());
    assert_eq!(live["operation_id"], "farm.publish");
    assert_eq!(live["errors"][0]["code"], "signer_unconfigured");
    assert_public_signer_session_binding_message(&live);
}

#[test]
fn radrootsd_listing_writes_dry_run_reject_missing_invocation_account() {
    for operation in ["publish", "update", "archive"] {
        let sandbox = RadrootsCliSandbox::new();
        let seller = identity_public(42);
        let listing_file = create_listing_draft(
            &sandbox,
            format!("radrootsd-no-account-dry-run-{operation}").as_str(),
        );
        make_listing_publishable_with_seller(
            &listing_file,
            "AAAAAAAAAAAAAAAAAAAAAw",
            seller.public_key_hex.as_str(),
        );
        sandbox.write_app_config(
            r#"[publish]
mode = "radrootsd"

[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_test"
"#,
        );

        let mut command = sandbox.command();
        command
            .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
            .args([
                "--format",
                "json",
                "--account-id",
                "missing-local-account",
                "--dry-run",
                "listing",
                operation,
                listing_file.to_string_lossy().as_ref(),
            ]);
        let output = command
            .output()
            .expect("run radrootsd dry-run listing write");
        let value: Value = serde_json::from_slice(&output.stdout).expect("json output");

        assert!(!output.status.success());
        assert_eq!(value["operation_id"], format!("listing.{operation}"));
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "account_unresolved");
        assert_eq!(value["errors"][0]["detail"]["class"], "account");
    }
}

#[test]
fn radrootsd_listing_writes_reject_missing_invocation_account() {
    for operation in ["publish", "update", "archive"] {
        let sandbox = RadrootsCliSandbox::new();
        let seller = identity_public(43);
        let listing_file = create_listing_draft(
            &sandbox,
            format!("radrootsd-no-account-{operation}").as_str(),
        );
        make_listing_publishable_with_seller(
            &listing_file,
            "AAAAAAAAAAAAAAAAAAAAAw",
            seller.public_key_hex.as_str(),
        );
        sandbox.write_app_config(
            r#"[publish]
mode = "radrootsd"

[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_test"
"#,
        );
        let mut command = sandbox.command();
        command
            .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
            .args([
                "--format",
                "json",
                "--account-id",
                "missing-local-account",
                "--approval-token",
                "approve",
                "listing",
                operation,
                listing_file.to_string_lossy().as_ref(),
            ]);
        let output = command.output().expect("run radrootsd listing write");
        let value: Value = serde_json::from_slice(&output.stdout).expect("json output");

        assert!(!output.status.success());
        assert_eq!(value["operation_id"], format!("listing.{operation}"));
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "account_unresolved");
        assert_eq!(value["errors"][0]["detail"]["class"], "account");
    }
}

#[test]
fn radrootsd_listing_publish_bridge_errors_are_classified() {
    for (message, code, class) in [
        (
            "unauthorized bridge bearer token",
            "auth_unauthorized",
            "auth",
        ),
        ("signer session unavailable", "signer_unavailable", "signer"),
        (
            "provider runtime unavailable",
            "provider_unavailable",
            "provider",
        ),
        (
            "bridge.listing.publish is disabled",
            "operation_unavailable",
            "operation",
        ),
    ] {
        let sandbox = RadrootsCliSandbox::new();
        let listing_file =
            create_listing_draft(&sandbox, format!("radrootsd-bridge-error-{class}").as_str());
        make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
        sandbox.write_app_config(
            r#"[publish]
mode = "radrootsd"

[[capability_binding]]
capability = "signer.remote_nip46"
provider = "myc"
target_kind = "explicit_endpoint"
target = "http://myc.invalid"
signer_session_ref = "session_test"
"#,
        );
        let server = OneShotJsonRpcServer::listing_publish_error(message);

        let mut command = sandbox.command();
        command
            .env("RADROOTS_RPC_URL", &server.endpoint)
            .env("RADROOTS_RPC_BEARER_TOKEN", "bridge_test")
            .args([
                "--format",
                "json",
                "--approval-token",
                "approve",
                "listing",
                "publish",
                listing_file.to_string_lossy().as_ref(),
            ]);
        let output = command.output().expect("run radrootsd listing publish");
        let value: Value = serde_json::from_slice(&output.stdout).expect("json output");
        let request = server.take_request();

        assert!(!output.status.success());
        assert_eq!(value["operation_id"], "listing.publish");
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], code);
        assert_eq!(value["errors"][0]["detail"]["class"], class);
        assert_contains(&value["errors"][0]["message"], message);
        assert_eq!(request.body["method"], "bridge.listing.publish");
    }
}

#[test]
fn radrootsd_listing_publish_bypasses_relay_signer_preflight() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Deferred Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let listing_file = create_listing_draft(&sandbox, "radrootsd-myc-router");
    make_listing_publishable(
        &listing_file,
        farm["result"]["config"]["farm_d_tag"]
            .as_str()
            .expect("farm d tag"),
    );
    sandbox.write_app_config("[publish]\nmode = \"radrootsd\"\n\n[signer]\nmode = \"myc\"\n");

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
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["errors"][0]["code"], "signer_unconfigured");
    assert_eq!(value["errors"][0]["detail"]["class"], "signer");
    assert_public_signer_session_binding_message(&value);
    assert!(
        !value["errors"][0]["message"]
            .as_str()
            .expect("error message")
            .contains("signer mode `myc`")
    );
}

#[test]
fn radrootsd_publish_mode_routes_listing_update() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Update Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let listing_file = create_listing_draft(&sandbox, "radrootsd-update-router");
    make_listing_publishable(
        &listing_file,
        farm["result"]["config"]["farm_d_tag"]
            .as_str()
            .expect("farm d tag"),
    );

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--publish-mode",
        "radrootsd",
        "--approval-token",
        "approve",
        "listing",
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(value["operation_id"], "listing.update");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "signer_unconfigured");
    assert_eq!(value["errors"][0]["detail"]["class"], "signer");
    assert_public_signer_session_binding_message(&value);
}

#[test]
fn listing_update_publish_attempts_direct_relay_with_approval() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let farm = sandbox.json_success(&[
        "--format",
        "json",
        "farm",
        "create",
        "--name",
        "Update Farm",
        "--location",
        "farmstand",
        "--country",
        "US",
        "--delivery-method",
        "pickup",
    ]);
    let listing_file = create_listing_draft(&sandbox, "update-unavailable");
    make_listing_publishable(
        &listing_file,
        farm["result"]["config"]["farm_d_tag"]
            .as_str()
            .expect("farm d tag"),
    );

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "listing",
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.update");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable");
    assert_eq!(value["errors"][0]["detail"]["class"], "network");
    assert_contains(
        &value["errors"][0]["message"],
        "direct relay connection failed",
    );
    assert!(
        !value["errors"][0]["message"]
            .as_str()
            .expect("error message")
            .contains("not implemented")
    );
    assert_no_removed_command_reference(&value, &["listing", "update"]);
    assert_no_daemon_runtime_reference(&value, &["listing", "update"]);
}

#[test]
fn removed_order_submit_watch_flag_is_rejected_publicly() {
    let output = radroots()
        .args(["order", "submit", "--watch"])
        .output()
        .expect("run removed order submit watch flag");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("unexpected argument") || stderr.contains("unrecognized"));
}

#[test]
fn removed_command_families_are_rejected_publicly() {
    for command in [
        "setup", "status", "doctor", "sell", "find", "local", "net", "myc", "rpc", "product",
        "runtime", "job", "message", "approval", "agent",
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
fn seller_order_decision_and_status_commands_are_public() {
    for (operation_id, args) in [
        (
            "order.accept",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "accept",
                "ord_public",
            ]
            .as_slice(),
        ),
        (
            "order.decline",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "decline",
                "ord_public",
                "--reason",
                "out_of_stock",
            ]
            .as_slice(),
        ),
        (
            "order.cancel",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "cancel",
                "ord_public",
                "--reason",
                "changed plans",
            ]
            .as_slice(),
        ),
        (
            "order.status.get",
            ["--format", "json", "order", "status", "get", "ord_public"].as_slice(),
        ),
        (
            "order.fulfillment.update",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "fulfillment",
                "update",
                "ord_public",
                "--state",
                "ready_for_pickup",
            ]
            .as_slice(),
        ),
        (
            "order.receipt.record",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "receipt",
                "record",
                "ord_public",
                "--received",
            ]
            .as_slice(),
        ),
        (
            "order.payment.record",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "payment",
                "record",
                "ord_public",
                "--amount",
                "12",
                "--currency",
                "USD",
                "--method",
                "cash",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.accept",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "settlement",
                "accept",
                "ord_public",
                "--payment-event-id",
                "1",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.reject",
            [
                "--format",
                "json",
                "--dry-run",
                "order",
                "settlement",
                "reject",
                "ord_public",
                "--payment-event-id",
                "1",
                "--reason",
                "reference mismatch",
            ]
            .as_slice(),
        ),
    ] {
        let output = radroots()
            .args(args)
            .output()
            .expect("run seller order command");
        let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

        assert_eq!(value["operation_id"], operation_id);
        assert_ne!(
            String::from_utf8(output.stderr).expect("utf8 stderr"),
            "unrecognized subcommand"
        );
    }

    let output = radroots()
        .args(["order", "decision", "accept", "ord_deferred"])
        .output()
        .expect("run removed nested decision command");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("unrecognized subcommand"));
}

#[test]
fn payment_commands_return_not_implemented_before_mutation_preflight() {
    let sandbox = RadrootsCliSandbox::new();

    for (operation_id, args) in [
        (
            "order.payment.record",
            [
                "--format",
                "json",
                "order",
                "payment",
                "record",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.payment.record",
            [
                "--format",
                "json",
                "--relay",
                "not-a-url",
                "order",
                "payment",
                "record",
                "ord_pending",
                "--method",
                "card",
            ]
            .as_slice(),
        ),
        (
            "order.payment.record",
            [
                "--format",
                "json",
                "--offline",
                "order",
                "payment",
                "record",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.accept",
            [
                "--format",
                "json",
                "order",
                "settlement",
                "accept",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.accept",
            [
                "--format",
                "json",
                "--relay",
                "not-a-url",
                "order",
                "settlement",
                "accept",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.accept",
            [
                "--format",
                "json",
                "--online",
                "--relay",
                "not-a-url",
                "order",
                "settlement",
                "accept",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.reject",
            [
                "--format",
                "json",
                "order",
                "settlement",
                "reject",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.reject",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "settlement",
                "reject",
                "ord_pending",
            ]
            .as_slice(),
        ),
    ] {
        let (output, value) = sandbox.json_output(args);

        assert!(!output.status.success());
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(value["operation_id"], operation_id);
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "not_implemented");
        assert_eq!(value["errors"][0]["exit_code"], 3);
        let message = value["errors"][0]["message"].as_str().expect("message");
        assert!(message.contains("not implemented"));
        assert!(message.contains("future phase"));
        assert!(!message.contains("approval_token"));
        assert!(!message.contains("relay"));
    }
}

#[test]
fn payment_commands_return_not_implemented_for_ndjson_output() {
    let sandbox = RadrootsCliSandbox::new();

    for (operation_id, args) in [
        (
            "order.payment.record",
            [
                "--format",
                "ndjson",
                "order",
                "payment",
                "record",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.accept",
            [
                "--format",
                "ndjson",
                "order",
                "settlement",
                "accept",
                "ord_pending",
            ]
            .as_slice(),
        ),
        (
            "order.settlement.reject",
            [
                "--format",
                "ndjson",
                "order",
                "settlement",
                "reject",
                "ord_pending",
            ]
            .as_slice(),
        ),
    ] {
        let output = sandbox.command().args(args).output().expect("run command");
        let frames = ndjson_from_stdout(&output);
        let error_frame = frames.last().expect("error frame");
        let message = error_frame["errors"][0]["message"]
            .as_str()
            .expect("message");

        assert!(!output.status.success());
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(error_frame["operation_id"], operation_id);
        assert_eq!(error_frame["frame_type"], "error");
        assert_eq!(error_frame["errors"][0]["code"], "not_implemented");
        assert_eq!(error_frame["errors"][0]["exit_code"], 3);
        assert!(message.contains("not implemented"));
        assert!(message.contains("future phase"));
        assert!(!message.contains("ndjson"));
        assert!(!message.contains("relay"));
        assert!(!message.contains("approval"));
        assert!(!message.contains("signer"));
    }
}

#[test]
fn target_outputs_do_not_suggest_removed_command_families() {
    let sandbox = RadrootsCliSandbox::new();

    for args in [
        ["--format", "json", "market", "product", "search", "eggs"].as_slice(),
        ["--format", "json", "market", "listing", "get", "eggs"].as_slice(),
        ["--format", "json", "listing", "get", "eggs"].as_slice(),
        ["--format", "json", "listing", "list"].as_slice(),
        [
            "--format",
            "json",
            "order",
            "get",
            "ord_AAAAAAAAAAAAAAAAAAAAAA",
        ]
        .as_slice(),
    ] {
        let value = sandbox.json_success(args);
        assert_no_removed_command_reference(&value, args);
    }

    let sync_args = ["--format", "json", "sync", "status", "get"];
    let (output, value) = sandbox.json_output(&sync_args);
    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "sync.status.get");
    assert_eq!(value["errors"][0]["code"], "operation_unavailable");
    assert_no_removed_command_reference(&value, &sync_args);
}

#[test]
fn listing_list_reports_empty_local_draft_state_truthfully() {
    let sandbox = RadrootsCliSandbox::new();
    let value = sandbox.json_success(&["--format", "json", "listing", "list"]);

    assert_eq!(value["operation_id"], "listing.list");
    assert_eq!(value["result"]["state"], "empty");
    assert_eq!(value["result"]["count"], 0);
    assert_eq!(
        value["result"]["listings"]
            .as_array()
            .expect("listings")
            .len(),
        0
    );
    assert!(
        value["result"]["draft_dir"]
            .as_str()
            .expect("draft dir")
            .ends_with("listings/drafts")
    );
    assert_no_removed_command_reference(&value, &["listing", "list"]);
}

#[test]
fn listing_list_reports_default_local_drafts() {
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
    let create = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "create",
        "--key",
        "eggs",
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
    let listing_file = create["result"]["file"].as_str().expect("listing file");
    assert!(Path::new(listing_file).exists());

    let value = sandbox.json_success(&["--format", "json", "listing", "list"]);
    let listing = &value["result"]["listings"][0];

    assert_eq!(value["operation_id"], "listing.list");
    assert_eq!(value["result"]["state"], "ready");
    assert_eq!(value["result"]["count"], 1);
    assert_eq!(listing["id"], create["result"]["listing_id"]);
    assert_eq!(listing["state"], "ready");
    assert_eq!(listing["file"], listing_file);
    assert_eq!(listing["product_key"], "eggs");
    assert_eq!(listing["title"], "Eggs");
    assert_eq!(listing["category"], "eggs");
    assert_eq!(listing["location_primary"], "farmstand");
    assert!(listing["seller_pubkey"].is_string());
    assert!(listing["farm_d_tag"].is_string());
    assert_no_removed_command_reference(&value, &["listing", "list"]);
}

#[test]
fn listing_rebind_updates_seller_actor_with_approval() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let listing_file = create_listing_draft(&sandbox, "rebind-listing");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");
    let initial_validation = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "validate",
        listing_file.to_string_lossy().as_ref(),
    ]);
    let first_pubkey = initial_validation["result"]["seller_pubkey"]
        .as_str()
        .expect("first pubkey");
    let before = fs::read_to_string(&listing_file).expect("listing before");
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");

    let dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "rebind",
        listing_file.to_string_lossy().as_ref(),
        second_account_id,
        "--farm-d-tag",
        "AAAAAAAAAAAAAAAAAAAAAw",
    ]);
    assert_eq!(dry_run["operation_id"], "listing.rebind");
    assert_eq!(dry_run["result"]["state"], "dry_run");
    assert_eq!(
        dry_run["result"]["from_seller_account_id"],
        first_account_id
    );
    assert_eq!(dry_run["result"]["from_seller_pubkey"], first_pubkey);
    assert_eq!(dry_run["result"]["to_seller_account_id"], second_account_id);
    let second_pubkey = dry_run["result"]["to_seller_pubkey"]
        .as_str()
        .expect("second pubkey");
    assert_eq!(dry_run["result"]["seller_pubkey_changed"], true);
    assert_eq!(
        fs::read_to_string(&listing_file).expect("listing after dry-run"),
        before
    );

    let unapproved = sandbox.json_output(&[
        "--format",
        "json",
        "listing",
        "rebind",
        listing_file.to_string_lossy().as_ref(),
        second_account_id,
        "--farm-d-tag",
        "AAAAAAAAAAAAAAAAAAAAAw",
    ]);
    assert!(!unapproved.0.status.success());
    assert_eq!(unapproved.1["errors"][0]["code"], "approval_required");

    let rebound = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "rebind",
        listing_file.to_string_lossy().as_ref(),
        second_account_id,
        "--farm-d-tag",
        "AAAAAAAAAAAAAAAAAAAAAw",
    ]);
    assert_eq!(rebound["operation_id"], "listing.rebind");
    assert_eq!(rebound["result"]["state"], "rebound");
    let after = fs::read_to_string(&listing_file).expect("listing after rebind");
    assert!(after.contains("[seller_actor]"));
    assert!(after.contains(second_account_id));
    assert!(after.contains("source = \"listing_rebind\""));

    let validation = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "validate",
        listing_file.to_string_lossy().as_ref(),
    ]);
    assert_eq!(validation["result"]["valid"], true);
    assert_eq!(validation["result"]["seller_account_id"], second_account_id);
    assert_eq!(validation["result"]["seller_pubkey"], second_pubkey);
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
fn next_actions_mirror_result_actions_for_json_and_ndjson() {
    let sandbox = RadrootsCliSandbox::new();

    let value = sandbox.json_success(&["--format", "json", "market", "refresh"]);

    assert_eq!(value["result"]["actions"][0], "radroots store init");
    assert_eq!(value["next_actions"][0]["label"], "store init");
    assert_eq!(value["next_actions"][0]["command"], "radroots store init");

    let output = sandbox
        .command()
        .args(["--format", "ndjson", "market", "refresh"])
        .output()
        .expect("run market refresh ndjson");
    let frames = ndjson_from_stdout(&output);
    let terminal = frames.last().expect("terminal ndjson frame");

    assert!(output.status.success());
    assert_eq!(
        terminal["payload"]["next_actions"][0]["command"],
        "radroots store init"
    );

    for args in [
        &["--format", "ndjson", "config", "get"][..],
        &["--format", "ndjson", "health", "status", "get"][..],
        &["--format", "ndjson", "health", "check", "run"][..],
    ] {
        let daemon = RadrootsCliSandbox::new();
        daemon.write_app_config("[publish]\nmode = \"radrootsd\"\n");
        let output = daemon.command().args(args).output().expect("run ndjson");
        let frames = ndjson_from_stdout(&output);
        let terminal = frames.last().expect("terminal ndjson frame");

        assert!(output.status.success(), "{args:?}");
        assert_rpc_bearer_token_next_action(&terminal["payload"]["next_actions"]);
        assert_signer_session_next_action(&terminal["payload"]["next_actions"]);
    }
}

#[test]
fn default_human_output_is_concise_and_not_json() {
    let output = radroots()
        .args(["workspace", "get"])
        .output()
        .expect("run workspace get");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("workspace.get: ok\n"));
    assert!(stdout.contains("request_id: req_workspace_get_"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn human_health_status_surfaces_publish_reason_and_actions() {
    let sandbox = RadrootsCliSandbox::new();

    let output = sandbox
        .command()
        .args(["--relay", "ws://127.0.0.1:19007", "health", "status", "get"])
        .output()
        .expect("run human health status");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("health.status.get: needs_attention\n"));
    assert!(stdout.contains("publish_mode: nostr_relay"));
    assert!(stdout.contains("publish_state: unconfigured"));
    assert!(stdout.contains("reason: nostr_relay publish mode requires a selected or default write-capable local account"));
    assert!(stdout.contains("- radroots store init"));
    assert!(stdout.contains("- radroots account create"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn human_market_refresh_missing_store_shows_action() {
    let sandbox = RadrootsCliSandbox::new();

    let output = sandbox
        .command()
        .args(["market", "refresh"])
        .output()
        .expect("run human market refresh");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("market.refresh: unconfigured\n"));
    assert!(stdout.contains("reason: local replica database is not initialized"));
    assert!(stdout.contains("- radroots store init"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn human_failure_output_preserves_error_code_and_message() {
    let output = radroots()
        .args(["--format", "human", "order", "submit"])
        .output()
        .expect("run order submit");

    assert_eq!(output.status.code(), Some(6));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("order.submit: error\n"));
    assert!(stdout.contains("request_id: req_order_submit_"));
    assert!(stdout.contains("error: approval_required"));
    assert!(stdout.contains("message: missing required `approval_token` input"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn human_failure_output_renders_structured_error_detail() {
    let output = radroots()
        .args([
            "--format",
            "human",
            "order",
            "event",
            "watch",
            "ord_missing",
        ])
        .output()
        .expect("run order event watch");

    assert_eq!(output.status.code(), Some(3));
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");

    assert!(stdout.starts_with("order.event.watch: error\n"));
    assert!(stdout.contains("request_id: req_order_event_watch_"));
    assert!(stdout.contains("error: not_implemented"));
    assert!(stdout.contains("state: not_implemented"));
    assert!(stdout.contains("reason: relay-backed order event watch is not implemented"));
    assert!(stdout.contains("- radroots order status get ord_missing"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn request_ids_are_invocation_unique_and_preserve_caller_fields() {
    let first = radroots()
        .args([
            "--format",
            "json",
            "--correlation-id",
            "corr_test",
            "--idempotency-key",
            "idem_test",
            "workspace",
            "get",
        ])
        .output()
        .expect("run first workspace get");
    let second = radroots()
        .args([
            "--format",
            "json",
            "--correlation-id",
            "corr_test",
            "--idempotency-key",
            "idem_test",
            "workspace",
            "get",
        ])
        .output()
        .expect("run second workspace get");

    assert!(first.status.success());
    assert!(second.status.success());
    let first: Value = serde_json::from_slice(&first.stdout).expect("first json envelope");
    let second: Value = serde_json::from_slice(&second.stdout).expect("second json envelope");

    assert_eq!(first["correlation_id"], "corr_test");
    assert_eq!(first["idempotency_key"], "idem_test");
    assert_eq!(second["correlation_id"], "corr_test");
    assert_eq!(second["idempotency_key"], "idem_test");
    assert!(
        first["request_id"]
            .as_str()
            .expect("first request id")
            .starts_with("req_workspace_get_")
    );
    assert_ne!(first["request_id"], second["request_id"]);
}

#[test]
fn supported_ndjson_outputs_started_and_completed_frames() {
    let sandbox = RadrootsCliSandbox::new();
    let output = sandbox
        .command()
        .args(["--format", "ndjson", "account", "list"])
        .output()
        .expect("run account list ndjson");

    assert!(output.status.success());
    let frames = ndjson_from_stdout(&output);

    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["schema_version"], "radroots.cli.output.v1");
    assert_eq!(frames[0]["operation_id"], "account.list");
    assert_eq!(frames[0]["frame_type"], "started");
    assert_eq!(frames[0]["sequence"], 0);
    assert_eq!(frames[1]["operation_id"], "account.list");
    assert_eq!(frames[1]["frame_type"], "completed");
    assert_eq!(frames[1]["sequence"], 1);
    assert_eq!(frames[1]["errors"].as_array().expect("errors").len(), 0);
    assert_eq!(frames[0]["request_id"], frames[1]["request_id"]);
}

#[test]
fn unsupported_ndjson_returns_structured_invalid_input() {
    let output = radroots()
        .args(["--format", "ndjson", "workspace", "get"])
        .output()
        .expect("run workspace get ndjson");

    assert_eq!(output.status.code(), Some(2));
    let frames = ndjson_from_stdout(&output);

    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["operation_id"], "workspace.get");
    assert_eq!(frames[0]["frame_type"], "started");
    assert_eq!(frames[1]["operation_id"], "workspace.get");
    assert_eq!(frames[1]["frame_type"], "error");
    assert_eq!(frames[1]["errors"][0]["code"], "invalid_input");
    assert_eq!(frames[1]["errors"][0]["exit_code"], 2);

    let watch_output = radroots()
        .args([
            "--format",
            "ndjson",
            "order",
            "event",
            "watch",
            "ord_missing",
        ])
        .output()
        .expect("run order event watch ndjson");

    assert_eq!(watch_output.status.code(), Some(2));
    let watch_frames = ndjson_from_stdout(&watch_output);
    assert_eq!(watch_frames.len(), 2);
    assert_eq!(watch_frames[0]["operation_id"], "order.event.watch");
    assert_eq!(watch_frames[0]["frame_type"], "started");
    assert_eq!(watch_frames[1]["operation_id"], "order.event.watch");
    assert_eq!(watch_frames[1]["frame_type"], "error");
    assert_eq!(watch_frames[1]["errors"][0]["code"], "invalid_input");
    assert_eq!(watch_frames[1]["errors"][0]["exit_code"], 2);
}

#[test]
fn offline_forbids_external_network_operations() {
    for (operation_id, args) in [
        (
            "sync.pull",
            ["--format", "json", "--offline", "sync", "pull"].as_slice(),
        ),
        (
            "sync.push",
            ["--format", "json", "--offline", "sync", "push"].as_slice(),
        ),
        (
            "market.refresh",
            ["--format", "json", "--offline", "market", "refresh"].as_slice(),
        ),
        (
            "order.submit",
            ["--format", "json", "--offline", "order", "submit"].as_slice(),
        ),
        (
            "order.cancel",
            [
                "--format",
                "json",
                "--offline",
                "order",
                "cancel",
                "ord_offline_cancel",
                "--reason",
                "changed plans",
            ]
            .as_slice(),
        ),
        (
            "order.revision.propose",
            [
                "--format",
                "json",
                "--offline",
                "--approval-token",
                "approve",
                "order",
                "revision",
                "propose",
                "ord_offline_revision",
                "--reason",
                "update count",
                "--bin-id",
                "bin-1",
                "--bin-count",
                "2",
            ]
            .as_slice(),
        ),
        (
            "order.revision.accept",
            [
                "--format",
                "json",
                "--offline",
                "--approval-token",
                "approve",
                "order",
                "revision",
                "accept",
                "ord_offline_revision",
                "--revision-id",
                "revision_1",
            ]
            .as_slice(),
        ),
        (
            "order.revision.decline",
            [
                "--format",
                "json",
                "--offline",
                "--approval-token",
                "approve",
                "order",
                "revision",
                "decline",
                "ord_offline_revision",
                "--revision-id",
                "revision_1",
                "--reason",
                "keep original",
            ]
            .as_slice(),
        ),
        (
            "order.fulfillment.update",
            [
                "--format",
                "json",
                "--offline",
                "order",
                "fulfillment",
                "update",
                "ord_offline_fulfillment",
                "--state",
                "ready_for_pickup",
            ]
            .as_slice(),
        ),
        (
            "order.receipt.record",
            [
                "--format",
                "json",
                "--offline",
                "order",
                "receipt",
                "record",
                "ord_offline_receipt",
                "--received",
            ]
            .as_slice(),
        ),
    ] {
        let output = radroots()
            .args(args)
            .output()
            .expect("run offline external command");

        assert!(!output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

        assert_eq!(value["operation_id"], operation_id);
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "offline_forbidden");
        assert_eq!(value["errors"][0]["exit_code"], 8);
    }
}

#[test]
fn offline_allows_supported_external_dry_run() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let listing_file = create_listing_draft(&sandbox, "offline-dry-run");
    make_listing_publishable(&listing_file, "AAAAAAAAAAAAAAAAAAAAAw");

    let publish = sandbox.json_success(&[
        "--format",
        "json",
        "--offline",
        "--dry-run",
        "listing",
        "publish",
        listing_file.to_string_lossy().as_ref(),
    ]);

    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");

    sandbox.json_success(&["--format", "json", "store", "init"]);
    let sync_push = sandbox.json_success(&[
        "--format",
        "json",
        "--offline",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "sync",
        "push",
    ]);

    assert_eq!(sync_push["operation_id"], "sync.push");
    assert_eq!(sync_push["result"]["state"], "ready");
}

#[test]
fn offline_rejects_order_decision_dry_run() {
    for (operation_id, args) in [
        (
            "order.accept",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "accept",
                "ord_offline_decision",
            ]
            .as_slice(),
        ),
        (
            "order.decline",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "decline",
                "ord_offline_decision",
                "--reason",
                "unavailable",
            ]
            .as_slice(),
        ),
        (
            "order.cancel",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "cancel",
                "ord_offline_decision",
                "--reason",
                "changed plans",
            ]
            .as_slice(),
        ),
        (
            "order.revision.propose",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "revision",
                "propose",
                "ord_offline_revision",
                "--reason",
                "update count",
                "--bin-id",
                "bin-1",
                "--bin-count",
                "2",
            ]
            .as_slice(),
        ),
        (
            "order.revision.accept",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "revision",
                "accept",
                "ord_offline_revision",
                "--revision-id",
                "revision_1",
            ]
            .as_slice(),
        ),
        (
            "order.revision.decline",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "revision",
                "decline",
                "ord_offline_revision",
                "--revision-id",
                "revision_1",
                "--reason",
                "keep original",
            ]
            .as_slice(),
        ),
        (
            "order.fulfillment.update",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "fulfillment",
                "update",
                "ord_offline_decision",
                "--state",
                "ready_for_pickup",
            ]
            .as_slice(),
        ),
        (
            "order.receipt.record",
            [
                "--format",
                "json",
                "--offline",
                "--dry-run",
                "order",
                "receipt",
                "record",
                "ord_offline_decision",
                "--issue",
                "damaged items",
            ]
            .as_slice(),
        ),
    ] {
        let output = radroots()
            .args(args)
            .output()
            .expect("run offline order decision dry-run");
        let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

        assert_eq!(output.status.code(), Some(8));
        assert_eq!(value["operation_id"], operation_id);
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "offline_forbidden");
        assert_eq!(value["errors"][0]["exit_code"], 8);
    }
}

#[test]
fn listing_publish_dry_run_validates_missing_file() {
    let sandbox = RadrootsCliSandbox::new();
    let missing = sandbox.root().join("missing-listing.toml");
    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        missing.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "not_found");
    assert_eq!(value["errors"][0]["exit_code"], 4);
    assert_no_removed_command_reference(
        &value,
        &["listing", "publish", "--dry-run", "missing-listing.toml"],
    );
}

#[test]
fn listing_publish_invalid_draft_returns_validation_failure() {
    let sandbox = RadrootsCliSandbox::new();
    let invalid = sandbox.root().join("invalid-listing.toml");
    fs::write(&invalid, "listing = [").expect("write invalid listing");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        invalid.to_string_lossy().as_ref(),
    ]);

    assert!(!output.status.success());
    assert_eq!(value["operation_id"], "listing.publish");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "validation_failed");
    assert_eq!(value["errors"][0]["exit_code"], 10);
}

#[test]
fn online_requires_relay_for_external_network_operations() {
    for (operation_id, args) in [
        (
            "sync.pull",
            ["--format", "json", "--online", "sync", "pull"].as_slice(),
        ),
        (
            "sync.push",
            ["--format", "json", "--online", "sync", "push"].as_slice(),
        ),
        (
            "market.refresh",
            ["--format", "json", "--online", "market", "refresh"].as_slice(),
        ),
        (
            "order.event.list",
            ["--format", "json", "--online", "order", "event", "list"].as_slice(),
        ),
        (
            "order.status.get",
            [
                "--format",
                "json",
                "--online",
                "order",
                "status",
                "get",
                "ord_missing",
            ]
            .as_slice(),
        ),
        (
            "order.cancel",
            [
                "--format",
                "json",
                "--online",
                "order",
                "cancel",
                "ord_missing",
                "--reason",
                "changed plans",
            ]
            .as_slice(),
        ),
        (
            "order.revision.propose",
            [
                "--format",
                "json",
                "--online",
                "--approval-token",
                "approve",
                "order",
                "revision",
                "propose",
                "ord_missing",
                "--reason",
                "update count",
                "--bin-id",
                "bin-1",
                "--bin-count",
                "2",
            ]
            .as_slice(),
        ),
        (
            "order.revision.accept",
            [
                "--format",
                "json",
                "--online",
                "--dry-run",
                "order",
                "revision",
                "accept",
                "ord_missing",
                "--revision-id",
                "revision_1",
            ]
            .as_slice(),
        ),
        (
            "order.revision.decline",
            [
                "--format",
                "json",
                "--online",
                "--dry-run",
                "order",
                "revision",
                "decline",
                "ord_missing",
                "--revision-id",
                "revision_1",
                "--reason",
                "keep original",
            ]
            .as_slice(),
        ),
        (
            "order.fulfillment.update",
            [
                "--format",
                "json",
                "--online",
                "order",
                "fulfillment",
                "update",
                "ord_missing",
                "--state",
                "ready_for_pickup",
            ]
            .as_slice(),
        ),
        (
            "order.receipt.record",
            [
                "--format",
                "json",
                "--online",
                "order",
                "receipt",
                "record",
                "ord_missing",
                "--received",
            ]
            .as_slice(),
        ),
    ] {
        let output = radroots()
            .args(args)
            .output()
            .expect("run online external command");

        assert!(!output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).expect("json envelope");

        assert_eq!(value["operation_id"], operation_id);
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "network_unavailable");
        assert_eq!(value["errors"][0]["exit_code"], 8);
        assert!(
            value["errors"][0]["message"]
                .as_str()
                .expect("message")
                .contains("requires at least one configured relay")
        );
    }
}

#[test]
fn radrootsd_sync_push_failure_exposes_nostr_relay_recovery_action() {
    let sandbox = RadrootsCliSandbox::new();
    let (json_output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--publish-mode",
        "radrootsd",
        "sync",
        "push",
    ]);

    assert_eq!(json_output.status.code(), Some(3));
    assert_eq!(value["operation_id"], "sync.push");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(value["errors"][0]["detail"]["state"], "unavailable");
    assert_eq!(value["errors"][0]["detail"]["publish"]["mode"], "radrootsd");
    assert_eq!(
        value["errors"][0]["detail"]["publish"]["state"],
        "unavailable"
    );
    assert_eq!(
        value["errors"][0]["detail"]["actions"][0],
        "radroots --publish-mode nostr_relay sync push"
    );
    assert_eq!(value["next_actions"][0]["kind"], "cli_command");
    assert_eq!(
        value["next_actions"][0]["command"],
        "radroots --publish-mode nostr_relay sync push"
    );

    let ndjson_output = sandbox
        .command()
        .args([
            "--format",
            "ndjson",
            "--publish-mode",
            "radrootsd",
            "sync",
            "push",
        ])
        .output()
        .expect("run sync push ndjson");
    let frames = ndjson_from_stdout(&ndjson_output);
    let terminal = frames.last().expect("terminal frame");

    assert_eq!(ndjson_output.status.code(), Some(3));
    assert_eq!(terminal["operation_id"], "sync.push");
    assert_eq!(terminal["frame_type"], "error");
    assert_eq!(
        terminal["payload"]["next_actions"][0]["command"],
        "radroots --publish-mode nostr_relay sync push"
    );

    let human_output = sandbox
        .command()
        .args(["--publish-mode", "radrootsd", "sync", "push"])
        .output()
        .expect("run sync push human");
    let stdout = String::from_utf8(human_output.stdout).expect("utf8 stdout");

    assert_eq!(human_output.status.code(), Some(3));
    assert!(stdout.starts_with("sync.push: error\n"));
    assert!(stdout.contains("error: operation_unavailable"));
    assert!(stdout.contains("state: unavailable"));
    assert!(stdout.contains("publish_mode: radrootsd"));
    assert!(stdout.contains("publish_state: unavailable"));
    assert!(
        stdout.contains("reason: `radroots sync push` cannot run with publish mode `radrootsd`")
    );
    assert!(stdout.contains("- radroots --publish-mode nostr_relay sync push"));
    assert!(serde_json::from_str::<Value>(&stdout).is_err());
}

#[test]
fn online_order_event_watch_returns_deferred_without_relay_preflight() {
    let sandbox = RadrootsCliSandbox::new();
    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--online",
        "order",
        "event",
        "watch",
        "ord_missing",
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(value["operation_id"], "order.event.watch");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "not_implemented");
    assert_eq!(value["errors"][0]["detail"]["state"], "not_implemented");
    assert_eq!(value["errors"][0]["detail"]["order_id"], "ord_missing");
    assert_eq!(
        value["next_actions"][0]["command"],
        "radroots order status get ord_missing"
    );
    assert!(
        !value["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains("configured relay")
    );
    assert_no_daemon_runtime_reference(&value, &["order", "event", "watch"]);
}

#[test]
fn online_allows_local_diagnostics() {
    let value = RadrootsCliSandbox::new().json_success(&[
        "--format",
        "json",
        "--online",
        "workspace",
        "get",
    ]);

    assert_eq!(value["operation_id"], "workspace.get");
    assert_eq!(value["errors"].as_array().expect("errors").len(), 0);
}

#[test]
fn store_export_dry_run_is_structured_unsupported() {
    let sandbox = RadrootsCliSandbox::new();
    let (output, value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "store", "export"]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(value["operation_id"], "store.export");
    assert_eq!(value["errors"][0]["code"], "invalid_input");
    assert_eq!(value["errors"][0]["exit_code"], 2);
}

#[test]
fn store_backup_dry_run_preflights_initialized_store_without_writing_file() {
    let sandbox = RadrootsCliSandbox::new();
    let (missing_output, missing_value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "store", "backup", "create"]);

    assert!(!missing_output.status.success());
    assert_eq!(missing_value["operation_id"], "store.backup.create");
    assert_eq!(missing_value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(missing_value["errors"][0]["exit_code"], 3);

    let init = sandbox.json_success(&["--format", "json", "store", "init"]);
    assert_eq!(init["operation_id"], "store.init");

    let backup =
        sandbox.json_success(&["--format", "json", "--dry-run", "store", "backup", "create"]);
    let file = backup["result"]["file"].as_str().expect("backup file");

    assert_eq!(backup["operation_id"], "store.backup.create");
    assert_eq!(backup["dry_run"], true);
    assert_eq!(backup["result"]["state"], "dry_run");
    assert_eq!(backup["result"]["size_bytes"], 0);
    assert!(!Path::new(file).exists());
}

#[test]
fn core_account_store_dry_runs_preflight_without_mutating_local_state() {
    let sandbox = RadrootsCliSandbox::new();

    let workspace = sandbox.json_success(&["--format", "json", "--dry-run", "workspace", "init"]);
    let workspace_db = workspace["result"]["local"]["path"]
        .as_str()
        .expect("workspace db path");
    assert_eq!(workspace["operation_id"], "workspace.init");
    assert_eq!(workspace["dry_run"], true);
    assert_eq!(workspace["result"]["state"], "dry_run");
    assert_eq!(workspace["result"]["local"]["replica_db"], "missing");
    assert!(!Path::new(workspace_db).exists());

    let store = sandbox.json_success(&["--format", "json", "--dry-run", "store", "init"]);
    let store_db = store["result"]["path"].as_str().expect("store db path");
    assert_eq!(store["operation_id"], "store.init");
    assert_eq!(store["dry_run"], true);
    assert_eq!(store["result"]["state"], "dry_run");
    assert_eq!(store["result"]["replica_db"], "missing");
    assert!(!Path::new(store_db).exists());

    let account_create =
        sandbox.json_success(&["--format", "json", "--dry-run", "account", "create"]);
    assert_eq!(account_create["operation_id"], "account.create");
    assert_eq!(account_create["dry_run"], true);
    assert_eq!(account_create["result"]["state"], "dry_run");
    assert_eq!(account_create["result"]["secret_backend"]["state"], "ready");

    let account_list = sandbox.json_success(&["--format", "json", "account", "list"]);
    assert_eq!(account_list["result"]["count"], 0);

    let created = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = created["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    let clear = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "account",
        "selection",
        "clear",
    ]);
    assert_eq!(clear["operation_id"], "account.selection.clear");
    assert_eq!(clear["result"]["state"], "dry_run");
    assert_eq!(clear["result"]["cleared_account"]["id"], account_id);
    assert_eq!(clear["result"]["remaining_account_count"], 1);

    let selection = sandbox.json_success(&["--format", "json", "account", "selection", "get"]);
    assert_eq!(
        selection["result"]["account_resolution"]["default_account"]["id"],
        account_id
    );
}

#[test]
fn seller_dry_runs_preflight_without_mutating_farm_or_listing_files() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let farm_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
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
    let farm_path = farm_dry_run["result"]["config"]["path"]
        .as_str()
        .expect("farm path");
    assert_eq!(farm_dry_run["operation_id"], "farm.create");
    assert_eq!(farm_dry_run["result"]["state"], "dry_run");
    assert!(!Path::new(farm_path).exists());

    let missing_update = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "profile",
        "update",
        "--value",
        "Dry Name",
    ]);
    assert_eq!(missing_update["operation_id"], "farm.profile.update");
    assert_eq!(missing_update["result"]["state"], "unconfigured");
    assert!(!Path::new(farm_path).exists());

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
    let farm_path = farm["result"]["config"]["path"]
        .as_str()
        .expect("farm path");
    let farm_before = fs::read_to_string(farm_path).expect("farm before");
    let farm_update = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "farm",
        "profile",
        "update",
        "--value",
        "Dry Name",
    ]);
    assert_eq!(farm_update["operation_id"], "farm.profile.update");
    assert_eq!(farm_update["result"]["state"], "dry_run");
    assert_eq!(farm_update["result"]["config"]["name"], "Dry Name");
    assert_eq!(
        fs::read_to_string(farm_path).expect("farm after dry-run"),
        farm_before
    );

    let listing_path = sandbox.root().join("dry-listing.toml");
    let listing_path_arg = listing_path.to_string_lossy();
    let listing_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "create",
        "--output",
        listing_path_arg.as_ref(),
        "--key",
        "eggs",
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
    assert_eq!(listing_dry_run["operation_id"], "listing.create");
    assert_eq!(listing_dry_run["result"]["state"], "dry_run");
    assert_eq!(listing_dry_run["result"]["file"], listing_path_arg.as_ref());
    assert!(!listing_path.exists());

    fs::write(&listing_path, "existing").expect("existing listing path");
    let (collision_output, collision) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "create",
        "--output",
        listing_path_arg.as_ref(),
        "--key",
        "eggs",
    ]);
    assert!(!collision_output.status.success());
    assert_eq!(collision["operation_id"], "listing.create");
    assert_eq!(collision["errors"][0]["code"], "validation_failed");

    let listing_file = create_listing_draft(&sandbox, "seller-dry-run");
    make_listing_publishable(
        &listing_file,
        farm["result"]["config"]["farm_d_tag"]
            .as_str()
            .expect("farm d tag"),
    );
    let listing_before = fs::read_to_string(&listing_file).expect("listing before");
    let listing_update = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "update",
        listing_file.to_string_lossy().as_ref(),
    ]);
    assert_eq!(listing_update["operation_id"], "listing.update");
    assert_eq!(listing_update["result"]["state"], "dry_run");
    assert_eq!(
        fs::read_to_string(&listing_file).expect("listing after dry-run"),
        listing_before
    );
}

#[test]
fn sync_push_partial_mixed_author_queue_reports_error_envelope() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    let signer = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    let selected_pubkey = signer["result"]["local"]["public_identity"]["public_key_hex"]
        .as_str()
        .expect("selected public key");
    sandbox.json_success(&["--format", "json", "store", "init"]);
    let other_pubkey = identity_public(81).public_key_hex;
    let other_pubkey_canonical = other_pubkey.to_ascii_lowercase();
    seed_sync_push_farm(&sandbox, SYNC_PUSH_FARM_D_TAG, selected_pubkey);
    seed_sync_push_member_claim(&sandbox, other_pubkey.as_str(), selected_pubkey);
    let batch = sync_push_pending_batch(&sandbox);
    let expected_publishable_count = batch
        .pending_events
        .iter()
        .filter(|event| event.author.eq_ignore_ascii_case(selected_pubkey))
        .count();
    let expected_skipped_count = batch.pending_count - expected_publishable_count;
    assert!(expected_publishable_count > 0);
    assert!(expected_skipped_count > 0);
    let relay =
        RelayPublishServer::with_publish_outcomes(vec![(true, ""); expected_publishable_count]);
    let relay_url = relay.endpoint().to_owned();

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        relay_url.as_str(),
        "--approval-token",
        "approve",
        "sync",
        "push",
    ]);

    assert!(!output.status.success(), "{value}");
    assert_eq!(value["operation_id"], "sync.push");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "network_unavailable", "{value}");
    assert_eq!(value["errors"][0]["detail"]["state"], "partial");
    assert_eq!(
        value["errors"][0]["detail"]["queue"]["pending_count"],
        json!(expected_skipped_count)
    );
    assert_eq!(
        value["errors"][0]["detail"]["published_count"],
        json!(expected_publishable_count)
    );
    assert_eq!(
        value["errors"][0]["detail"]["skipped_count"],
        json!(expected_skipped_count)
    );
    assert_contains(
        &value["errors"][0]["detail"]["reason"],
        "belong to another author",
    );
    assert_eq!(
        value["errors"][0]["detail"]["actions"][1],
        "radroots account list"
    );
    assert_eq!(
        value["errors"][0]["detail"]["actions"][2],
        format!("radroots --account-id {other_pubkey_canonical} sync push")
    );
    assert_eq!(
        value["next_actions"][2]["command"],
        format!("radroots --account-id {other_pubkey_canonical} sync push")
    );
    let requests = relay.take_requests(expected_publishable_count);
    assert_eq!(requests.len(), expected_publishable_count);
    assert!(
        requests
            .iter()
            .all(|request| request["pubkey"] == selected_pubkey)
    );
    assert_no_removed_command_reference(&value, &["sync", "push"]);
    assert_no_daemon_runtime_reference(&value, &["sync", "push"]);
}

#[test]
fn sync_push_other_author_only_queue_reports_unconfigured_error_envelope() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    sandbox.json_success(&["--format", "json", "store", "init"]);
    let other_pubkey = identity_public(82).public_key_hex;
    let other_pubkey_canonical = other_pubkey.to_ascii_lowercase();
    seed_sync_push_farm(&sandbox, SYNC_PUSH_FARM_D_TAG, other_pubkey.as_str());

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "sync",
        "push",
    ]);

    assert!(!output.status.success(), "{value}");
    assert_eq!(value["operation_id"], "sync.push");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(
        value["errors"][0]["code"], "operation_unavailable",
        "{value}"
    );
    assert_eq!(value["errors"][0]["detail"]["state"], "unconfigured");
    let pending_count = value["errors"][0]["detail"]["queue"]["pending_count"]
        .as_u64()
        .expect("pending count");
    assert!(pending_count > 0);
    assert_eq!(value["errors"][0]["detail"]["publishable_count"], 0);
    assert_eq!(value["errors"][0]["detail"]["published_count"], 0);
    assert_eq!(value["errors"][0]["detail"]["skipped_count"], pending_count);
    assert_contains(
        &value["errors"][0]["detail"]["reason"],
        "belong to another author",
    );
    assert_eq!(
        value["errors"][0]["detail"]["actions"][1],
        "radroots account list"
    );
    assert_eq!(
        value["errors"][0]["detail"]["actions"][2],
        format!("radroots --account-id {other_pubkey_canonical} sync push")
    );
    assert_eq!(
        value["next_actions"][2]["command"],
        format!("radroots --account-id {other_pubkey_canonical} sync push")
    );
    assert_no_removed_command_reference(&value, &["sync", "push"]);
    assert_no_daemon_runtime_reference(&value, &["sync", "push"]);
}

#[test]
fn buyer_market_sync_basket_dry_runs_preflight_without_mutating_local_state() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);

    let market = sandbox.json_success(&["--format", "json", "--dry-run", "market", "refresh"]);
    assert_eq!(market["operation_id"], "market.refresh");
    assert_eq!(market["dry_run"], true);
    assert_eq!(market["result"]["state"], "unconfigured");
    assert_eq!(market["result"]["replica_db"], "missing");

    let (sync_pull_output, sync_pull) =
        sandbox.json_output(&["--format", "json", "--dry-run", "sync", "pull"]);
    assert!(!sync_pull_output.status.success());
    assert_eq!(sync_pull["operation_id"], "sync.pull");
    assert_eq!(sync_pull["dry_run"], true);
    assert_eq!(sync_pull["errors"][0]["code"], "operation_unavailable");
    assert_eq!(sync_pull["errors"][0]["detail"]["state"], "unconfigured");
    assert_eq!(sync_pull["errors"][0]["detail"]["replica_db"], "missing");

    let (sync_push_output, sync_push) =
        sandbox.json_output(&["--format", "json", "--dry-run", "sync", "push"]);
    assert!(!sync_push_output.status.success());
    assert_eq!(sync_push["operation_id"], "sync.push");
    assert_eq!(sync_push["dry_run"], true);
    assert_eq!(sync_push["errors"][0]["code"], "operation_unavailable");
    assert_eq!(sync_push["errors"][0]["detail"]["state"], "unconfigured");
    assert_eq!(sync_push["errors"][0]["detail"]["replica_db"], "missing");

    sandbox.json_success(&["--format", "json", "store", "init"]);
    let relay_refresh = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "market",
        "refresh",
    ]);
    assert_eq!(relay_refresh["operation_id"], "market.refresh");
    assert_eq!(relay_refresh["dry_run"], true);
    assert_eq!(relay_refresh["result"]["state"], "ready");
    assert_eq!(
        relay_refresh["result"]["target_relays"][0],
        "ws://127.0.0.1:9"
    );
    assert_eq!(relay_refresh["result"]["fetched_count"], 0);
    assert_eq!(relay_refresh["result"]["ingested_count"], 0);

    let sync_push_ready = sandbox.json_success(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--dry-run",
        "sync",
        "push",
    ]);
    assert_eq!(sync_push_ready["operation_id"], "sync.push");
    assert_eq!(sync_push_ready["dry_run"], true);
    assert_eq!(sync_push_ready["result"]["state"], "ready");
    assert_eq!(
        sync_push_ready["result"]["target_relays"][0],
        "ws://127.0.0.1:9"
    );
    assert_eq!(sync_push_ready["result"]["publishable_count"], 0);
    assert_eq!(sync_push_ready["result"]["published_count"], 0);

    let empty_search =
        sandbox.json_success(&["--format", "json", "market", "product", "search", "eggs"]);
    assert_eq!(empty_search["operation_id"], "market.product.search");
    assert_eq!(empty_search["result"]["state"], "empty");

    let create_dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "create",
        "basket_probe",
    ]);
    let basket_file = create_dry_run["result"]["file"]
        .as_str()
        .expect("basket file");
    assert_eq!(create_dry_run["operation_id"], "basket.create");
    assert_eq!(create_dry_run["result"]["state"], "dry_run");
    assert!(!Path::new(basket_file).exists());

    sandbox.json_success(&["--format", "json", "basket", "create", "basket_probe"]);
    let (collision_output, collision) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "create",
        "basket_probe",
    ]);
    assert!(!collision_output.status.success());
    assert_eq!(collision["operation_id"], "basket.create");
    assert_eq!(collision["errors"][0]["code"], "invalid_input");

    let before_add = sandbox.json_success(&["--format", "json", "basket", "get", "basket_probe"]);
    let add = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "item",
        "add",
        "basket_probe",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    assert_eq!(add["operation_id"], "basket.item.add");
    assert_eq!(add["result"]["state"], "dry_run");
    let after_add = sandbox.json_success(&["--format", "json", "basket", "get", "basket_probe"]);
    assert_eq!(after_add["result"], before_add["result"]);

    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "basket_probe",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "basket",
        "quote",
        "create",
        "basket_probe",
    ]);
    let order_file = quote["result"]["order"]["file"]
        .as_str()
        .expect("order file");
    assert_eq!(quote["operation_id"], "basket.quote.create");
    assert_eq!(quote["result"]["state"], "dry_run");
    assert_eq!(quote["result"]["order"]["state"], "dry_run");
    assert!(!Path::new(order_file).exists());

    let basket_after_quote =
        sandbox.json_success(&["--format", "json", "basket", "get", "basket_probe"]);
    assert_eq!(basket_after_quote["result"]["quote"], Value::Null);
}

#[test]
fn required_approval_token_rejects_absent_empty_and_whitespace_values() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(61);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "approval-import", &public_identity);
    let public_identity_path = public_identity_file.to_string_lossy();

    assert_required_approval_token_rejected(
        &sandbox,
        "account.import",
        &["account", "import", public_identity_path.as_ref()],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "account.remove",
        &["account", "remove", "acct_missing"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "farm.rebind",
        &["farm", "rebind", "acct_missing"],
    );
    assert_required_approval_token_rejected(&sandbox, "farm.publish", &["farm", "publish"]);
    assert_required_approval_token_rejected(
        &sandbox,
        "listing.publish",
        &["listing", "publish", "missing-listing.toml"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "listing.update",
        &["listing", "update", "missing-listing.toml"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "listing.archive",
        &["listing", "archive", "missing-listing.toml"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "sync.push",
        &["--relay", "ws://127.0.0.1:9", "sync", "push"],
    );
    assert_required_approval_token_rejected(&sandbox, "order.submit", &["order", "submit"]);
    assert_required_approval_token_rejected(
        &sandbox,
        "order.rebind",
        &["order", "rebind", "ord_missing", "acct_missing"],
    );
    assert_required_approval_token_rejected(&sandbox, "order.accept", &["order", "accept"]);
    assert_required_approval_token_rejected(
        &sandbox,
        "order.decline",
        &["order", "decline", "--reason", "out_of_stock"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "order.cancel",
        &["order", "cancel", "--reason", "changed plans"],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "order.revision.accept",
        &[
            "order",
            "revision",
            "accept",
            "ord_pending",
            "--revision-id",
            "rev_pending",
        ],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "order.revision.decline",
        &[
            "order",
            "revision",
            "decline",
            "ord_pending",
            "--revision-id",
            "rev_pending",
            "--reason",
            "keep original order",
        ],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "order.fulfillment.update",
        &[
            "order",
            "fulfillment",
            "update",
            "ord_pending_fulfillment",
            "--state",
            "ready_for_pickup",
        ],
    );
    assert_required_approval_token_rejected(
        &sandbox,
        "order.receipt.record",
        &["order", "receipt", "record", "ord_pending", "--received"],
    );
}

fn assert_required_approval_token_rejected(
    sandbox: &RadrootsCliSandbox,
    operation_id: &str,
    command_args: &[&str],
) {
    for token in [None, Some(""), Some(" \t ")] {
        let mut args = vec!["--format", "json"];
        if let Some(token) = token {
            args.push("--approval-token");
            args.push(token);
        }
        args.extend_from_slice(command_args);

        let (output, value) = sandbox.json_output(&args);

        assert_eq!(output.status.code(), Some(6), "`{args:?}` should fail");
        assert_eq!(value["operation_id"], operation_id);
        assert_eq!(value["errors"][0]["code"], "approval_required");
        assert_eq!(value["errors"][0]["exit_code"], 6);
        assert_no_removed_command_reference(&value, &args);
    }
}

#[test]
fn order_fulfillment_update_requires_state_before_approval() {
    let sandbox = RadrootsCliSandbox::new();

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "order",
        "fulfillment",
        "update",
        "ord_missing_state",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(value["operation_id"], "order.fulfillment.update");
    assert_eq!(value["result"], Value::Null);
    assert_eq!(value["errors"][0]["code"], "invalid_input");
    assert_eq!(value["errors"][0]["exit_code"], 2);
    assert!(
        value["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains("state")
    );
    assert_no_removed_command_reference(&value, &["order", "fulfillment", "update"]);
    assert_no_daemon_runtime_reference(&value, &["order", "fulfillment", "update"]);
}

#[test]
fn order_submit_missing_order_returns_not_found_while_read_view_stays_successful() {
    let sandbox = RadrootsCliSandbox::new();

    let get = sandbox.json_success(&[
        "--format",
        "json",
        "order",
        "get",
        "ord_missing_submit_target",
    ]);
    assert_eq!(get["operation_id"], "order.get");
    assert_eq!(get["result"]["state"], "missing");
    assert_eq!(get["errors"].as_array().expect("errors").len(), 0);

    let (output, submit) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "order",
        "submit",
        "ord_missing_submit_target",
    ]);

    assert_eq!(output.status.code(), Some(4));
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["errors"][0]["code"], "not_found");
    assert_eq!(submit["errors"][0]["exit_code"], 4);
    assert_eq!(submit["errors"][0]["detail"]["class"], "resource");
    assert_no_removed_command_reference(&submit, &["order", "submit"]);
    assert_no_daemon_runtime_reference(&submit, &["order", "submit"]);
}

fn create_ready_order(sandbox: &RadrootsCliSandbox, basket_id: &str) -> String {
    sandbox.json_success(&["--format", "json", "account", "create"]);
    seed_orderable_listing(sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", basket_id]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        basket_id,
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&["--format", "json", "basket", "quote", "create", basket_id]);
    quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id")
        .to_owned()
}

fn rewrite_order_bin(sandbox: &RadrootsCliSandbox, order_id: &str, bin_id: &str) {
    let path = sandbox
        .root()
        .join("data/apps/cli/orders/drafts")
        .join(format!("{order_id}.toml"));
    let contents = fs::read_to_string(&path).expect("read order draft");
    let updated = contents.replace(
        "bin_id = \"bin-1\"",
        format!("bin_id = \"{bin_id}\"").as_str(),
    );
    assert_ne!(updated, contents);
    fs::write(path, updated).expect("rewrite order draft bin");
}

fn rewrite_order_buyer_actor_pubkey(sandbox: &RadrootsCliSandbox, order_id: &str, pubkey: &str) {
    let path = sandbox
        .root()
        .join("data/apps/cli/orders/drafts")
        .join(format!("{order_id}.toml"));
    let contents = fs::read_to_string(&path).expect("read order draft");
    let mut in_buyer_actor = false;
    let mut replaced = false;
    let updated = contents
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') {
                in_buyer_actor = trimmed == "[buyer_actor]";
            }
            if in_buyer_actor && trimmed.starts_with("pubkey =") {
                replaced = true;
                format!("{}pubkey = \"{}\"", line_indent(line), pubkey)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(replaced, "buyer_actor pubkey field");
    fs::write(path, format!("{updated}\n")).expect("rewrite order draft buyer actor");
}

fn line_indent(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}

fn signed_order_request_event_for_quote(
    buyer: &radroots_identity::RadrootsIdentity,
    order_id: &str,
    listing_event_id: &str,
    economics: RadrootsTradeOrderEconomics,
) -> RadrootsNostrEvent {
    let buyer_pubkey = buyer.public_key_hex();
    let seller_pubkey = "1".repeat(64);
    let payload = RadrootsTradeOrderRequested {
        order_id: order_id.to_owned(),
        listing_addr: LISTING_ADDR.to_owned(),
        buyer_pubkey,
        seller_pubkey,
        items: vec![RadrootsTradeOrderItem {
            bin_id: "bin-1".to_owned(),
            bin_count: 2,
        }],
        economics,
    };
    let parts = active_trade_order_request_event_build(
        &RadrootsNostrEventPtr {
            id: listing_event_id.to_owned(),
            relays: None,
        },
        &payload,
    )
    .expect("order request parts");
    radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
        .expect("nostr event builder")
        .sign_with_keys(buyer.keys())
        .expect("signed order request")
}

#[test]
fn buyer_target_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();

    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    assert_eq!(account["operation_id"], "account.create");
    assert_eq!(account["result"]["account"]["signer"], "local");
    assert_eq!(account["result"]["account"]["custody"], "secret_backed");
    assert_eq!(account["result"]["account"]["write_capable"], true);
    assert_no_removed_command_reference(&account, &["account", "create"]);

    let signer = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(signer["operation_id"], "signer.status.get");
    assert_eq!(signer["result"]["mode"], "local");
    assert_eq!(signer["result"]["state"], "ready");
    assert_eq!(signer["result"]["signer_account_id"], account_id);
    assert_no_removed_command_reference(&signer, &["signer", "status", "get"]);

    let listing_event_id = seed_orderable_listing(&sandbox, LISTING_ADDR);

    let search = sandbox.json_success(&["--format", "json", "market", "product", "search", "eggs"]);
    assert_eq!(search["operation_id"], "market.product.search");
    assert_eq!(search["errors"].as_array().expect("errors").len(), 0);
    assert_no_removed_command_reference(&search, &["market", "product", "search"]);

    let create = sandbox.json_success(&["--format", "json", "basket", "create", "basket_flow"]);
    assert_eq!(create["operation_id"], "basket.create");
    assert_eq!(create["result"]["basket_id"], "basket_flow");
    assert_no_removed_command_reference(&create, &["basket", "create"]);

    let add = sandbox.json_success(&[
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
    ]);
    assert_eq!(add["operation_id"], "basket.item.add");
    assert_eq!(add["result"]["ready_for_quote"], true);
    assert_no_removed_command_reference(&add, &["basket", "item", "add"]);

    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "basket_flow",
    ]);
    assert_eq!(quote["operation_id"], "basket.quote.create");
    assert_eq!(quote["result"]["state"], "quoted");
    assert_no_removed_command_reference(&quote, &["basket", "quote", "create"]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");
    let quote_economics = &quote["result"]["quote"]["economics"];
    let order_file = quote["result"]["order"]["file"]
        .as_str()
        .expect("order file");
    assert_eq!(quote["result"]["quote"]["ready_for_submit"], true);
    assert_eq!(quote["result"]["quote"]["quote_version"], 1);
    assert_eq!(
        quote["result"]["quote"]["quote_id"],
        quote_economics["quote_id"]
    );
    assert_eq!(quote_economics["quote_version"], 1);
    assert_eq!(quote_economics["pricing_basis"], "listing_event");
    assert_eq!(quote_economics["currency"], "USD");
    assert_eq!(quote_economics["items"][0]["bin_id"], "bin-1");
    assert_eq!(quote_economics["items"][0]["bin_count"], 2);
    assert_eq!(quote_economics["discounts"], Value::Array(Vec::new()));
    assert_eq!(quote_economics["adjustments"], Value::Array(Vec::new()));
    assert_eq!(
        quote["result"]["order"]["economics"],
        quote_economics.clone()
    );
    let order_draft = fs::read_to_string(order_file).expect("read order draft");
    assert!(order_draft.contains("[buyer_actor]"));
    assert!(order_draft.contains("source = \"resolved_account\""));
    assert!(order_draft.contains("[order.economics]"));
    assert!(order_draft.contains("pricing_basis = \"listing_event\""));
    assert_eq!(quote["result"]["order"]["buyer_account_id"], account_id);
    assert_eq!(
        quote["result"]["order"]["buyer_actor_source"],
        "resolved_account"
    );
    assert_eq!(
        quote["result"]["order"]["listing_event_id"],
        listing_event_id
    );

    let orders = sandbox.json_success(&["--format", "json", "order", "list"]);
    assert_eq!(orders["operation_id"], "order.list");
    assert_eq!(orders["result"]["state"], "ready");
    assert_eq!(orders["result"]["count"], 1);
    assert_eq!(orders["result"]["orders"][0]["id"], order_id);
    assert_eq!(orders["result"]["orders"][0]["ready_for_submit"], true);
    assert_eq!(
        orders["result"]["orders"][0]["listing_event_id"],
        listing_event_id
    );
    assert_eq!(
        orders["result"]["orders"][0]["buyer_account_id"],
        account_id
    );
    assert_eq!(
        orders["result"]["orders"][0]["buyer_actor_source"],
        "resolved_account"
    );
    assert_eq!(
        orders["result"]["orders"][0]["economics"],
        quote_economics.clone()
    );
    assert_eq!(orders["result"]["orders"][0]["issues"], Value::Null);
    assert_no_removed_command_reference(&orders, &["order", "list"]);
    assert_no_daemon_runtime_reference(&orders, &["order", "list"]);

    let (dry_output, submit) =
        sandbox.json_output(&["--format", "json", "--dry-run", "order", "submit", order_id]);
    assert!(!dry_output.status.success());
    assert_eq!(dry_output.status.code(), Some(8));
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["dry_run"], true);
    assert_eq!(submit["result"], Value::Null);
    assert_eq!(submit["errors"][0]["code"], "network_unavailable");
    assert_eq!(submit["errors"][0]["detail"]["class"], "network");
    assert!(
        submit["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains(
                "order submit requires at least one configured relay before publish preflight"
            )
    );
    assert_no_removed_command_reference(&submit, &["order", "submit", "--dry-run"]);
    assert_no_daemon_runtime_reference(&submit, &["order", "submit", "--dry-run"]);

    let (output, unavailable_submit) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id,
    ]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(8));
    assert_eq!(unavailable_submit["operation_id"], "order.submit");
    assert_eq!(unavailable_submit["result"], Value::Null);
    assert_eq!(
        unavailable_submit["errors"][0]["code"],
        "network_unavailable"
    );
    assert_eq!(
        unavailable_submit["errors"][0]["detail"]["class"],
        "network"
    );
    assert!(
        unavailable_submit["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains(
                "order submit requires at least one configured relay before publish preflight"
            )
    );
    assert_no_removed_command_reference(&unavailable_submit, &["order", "submit"]);
    assert_no_daemon_runtime_reference(&unavailable_submit, &["order", "submit"]);

    let order_after_submit = sandbox.json_success(&["--format", "json", "order", "get", order_id]);
    assert_eq!(order_after_submit["operation_id"], "order.get");
    assert_eq!(order_after_submit["result"]["state"], "ready");
    assert_eq!(
        order_after_submit["result"]["economics"],
        quote_economics.clone()
    );
    assert_eq!(order_after_submit["result"]["job"], Value::Null);
    assert_eq!(order_after_submit["result"]["workflow"], Value::Null);
    assert_no_daemon_runtime_reference(&order_after_submit, &["order", "get"]);

    let (watch_output, watch) =
        sandbox.json_output(&["--format", "json", "order", "event", "watch", order_id]);
    assert!(!watch_output.status.success());
    assert_eq!(watch_output.status.code(), Some(3));
    assert_eq!(watch["operation_id"], "order.event.watch");
    assert_eq!(watch["result"], Value::Null);
    assert_eq!(watch["errors"][0]["code"], "not_implemented");
    assert_eq!(watch["errors"][0]["detail"]["state"], "not_implemented");
    assert_eq!(watch["errors"][0]["detail"]["order_id"], order_id);
    assert_eq!(
        watch["next_actions"][0]["command"],
        format!("radroots order status get {order_id}")
    );
    assert_no_daemon_runtime_reference(&watch, &["order", "event", "watch"]);
    assert!(
        !serde_json::to_string(&watch)
            .expect("watch json")
            .contains("local order drafts")
    );
}

#[test]
fn order_get_and_list_report_missing_bound_buyer_account() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "missing_buyer_account");

    let ready = sandbox.json_success(&["--format", "json", "order", "get", order_id.as_str()]);
    let account_id = ready["result"]["buyer_account_id"]
        .as_str()
        .expect("buyer account id");
    assert_eq!(ready["result"]["state"], "ready");
    assert_eq!(ready["result"]["buyer_account_id"], account_id);
    assert_eq!(ready["result"]["buyer_custody"], "secret_backed");
    assert_eq!(ready["result"]["buyer_write_capable"], true);

    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "remove",
        account_id,
    ]);

    let missing = sandbox.json_success(&["--format", "json", "order", "get", order_id.as_str()]);
    assert_eq!(missing["operation_id"], "order.get");
    assert_eq!(missing["result"]["state"], "draft");
    assert_eq!(missing["result"]["ready_for_submit"], false);
    assert_eq!(missing["result"]["buyer_account_id"], account_id);
    assert_eq!(missing["result"]["buyer_custody"], Value::Null);
    assert!(
        missing["result"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue["code"] == "account_unresolved")
    );
    assert!(
        missing["result"]["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action == "radroots account import <path>")
    );
    assert!(
        missing["result"]["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action
                == &Value::String(format!("radroots order rebind {order_id} <selector>")))
    );

    let list = sandbox.json_success(&["--format", "json", "order", "list"]);
    assert_eq!(list["result"]["state"], "degraded");
    assert_eq!(list["result"]["orders"][0]["ready_for_submit"], false);
    assert!(
        list["result"]["orders"][0]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue["code"] == "account_unresolved")
    );

    let (submit_output, submit) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "order",
        "submit",
        order_id.as_str(),
    ]);
    assert!(!submit_output.status.success());
    assert_eq!(submit_output.status.code(), Some(5));
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["errors"][0]["code"], "account_unresolved");
    assert_eq!(submit["errors"][0]["detail"]["order_id"], order_id);
}

#[test]
fn order_get_marks_watch_only_bound_buyer_unready() {
    let sandbox = RadrootsCliSandbox::new();
    let public_identity = identity_public(92);
    let public_identity_file =
        write_public_identity_profile(&sandbox, "order-watch-only-buyer", &public_identity);
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
        .expect("watch account id");
    assert_eq!(imported["result"]["account"]["custody"], "watch_only");

    seed_orderable_listing(&sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", "watch_buyer"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "watch_buyer",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "watch_buyer",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");

    let get = sandbox.json_success(&["--format", "json", "order", "get", order_id]);
    assert_eq!(get["result"]["state"], "draft");
    assert_eq!(get["result"]["ready_for_submit"], false);
    assert_eq!(get["result"]["buyer_account_id"], account_id);
    assert_eq!(get["result"]["buyer_custody"], "watch_only");
    assert_eq!(get["result"]["buyer_write_capable"], false);
    assert!(
        get["result"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue["code"] == "account_watch_only")
    );
    assert!(
        get["result"]["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action
                == &Value::String(format!(
                    "radroots account attach-secret {account_id} <path>"
                )))
    );

    let (submit_output, submit) =
        sandbox.json_output(&["--format", "json", "--dry-run", "order", "submit", order_id]);
    assert!(!submit_output.status.success());
    assert_eq!(submit_output.status.code(), Some(7));
    assert_eq!(submit["operation_id"], "order.submit");
    assert_eq!(submit["errors"][0]["code"], "account_watch_only");
    assert_eq!(
        submit["errors"][0]["detail"]["order_buyer_account_id"],
        account_id
    );
}

#[test]
fn order_get_marks_bound_buyer_pubkey_mismatch_unready() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "mismatched_buyer_actor");
    let other_pubkey = identity_public(93).public_key_hex;
    rewrite_order_buyer_actor_pubkey(&sandbox, order_id.as_str(), other_pubkey.as_str());

    let get = sandbox.json_success(&["--format", "json", "order", "get", order_id.as_str()]);
    assert_eq!(get["result"]["state"], "draft");
    assert_eq!(get["result"]["ready_for_submit"], false);
    assert_eq!(get["result"]["buyer_custody"], "secret_backed");
    assert_eq!(get["result"]["buyer_write_capable"], true);
    assert!(
        get["result"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue["code"] == "account_mismatch")
    );
    assert!(
        get["result"]["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| action
                == &Value::String(format!("radroots order rebind {order_id} <selector>")))
    );
}

#[test]
fn order_rebind_previews_and_writes_bound_buyer_actor_updates() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "order_rebind");
    let order_file = sandbox
        .root()
        .join("data/apps/cli/orders/drafts")
        .join(format!("{order_id}.toml"));
    let before = fs::read_to_string(&order_file).expect("order before rebind");
    let second = sandbox.json_success(&["--format", "json", "account", "create"]);
    let second_account_id = second["result"]["account"]["id"]
        .as_str()
        .expect("second account id");

    let dry_run = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "order",
        "rebind",
        order_id.as_str(),
        second_account_id,
    ]);
    assert_eq!(dry_run["operation_id"], "order.rebind");
    assert_eq!(dry_run["result"]["state"], "dry_run");
    assert_eq!(dry_run["result"]["from_order_id"], order_id);
    assert_eq!(dry_run["result"]["order_id_changed"], true);
    assert_eq!(dry_run["result"]["buyer_pubkey_changed"], true);
    assert_eq!(dry_run["result"]["to_buyer_account_id"], second_account_id);
    assert_eq!(
        dry_run["result"]["existing_request_check"],
        "skipped_no_relays"
    );
    assert_eq!(
        fs::read_to_string(&order_file).expect("order after dry-run rebind"),
        before
    );

    let (unapproved_output, unapproved) = sandbox.json_output(&[
        "--format",
        "json",
        "order",
        "rebind",
        order_id.as_str(),
        second_account_id,
    ]);
    assert!(!unapproved_output.status.success());
    assert_eq!(unapproved["operation_id"], "order.rebind");
    assert_eq!(unapproved["errors"][0]["code"], "approval_required");

    let rebound = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "order",
        "rebind",
        order_id.as_str(),
        second_account_id,
    ]);
    assert_eq!(rebound["operation_id"], "order.rebind");
    assert_eq!(rebound["result"]["state"], "rebound");
    assert_eq!(rebound["result"]["from_order_id"], order_id);
    assert_eq!(rebound["result"]["order_id_changed"], true);
    let rebound_order_id = rebound["result"]["to_order_id"]
        .as_str()
        .expect("rebound order id");
    assert_ne!(rebound_order_id, order_id);
    let rebound_file = rebound["result"]["file"].as_str().expect("rebound file");
    assert!(!order_file.exists());
    let after = fs::read_to_string(rebound_file).expect("order after rebind");
    assert!(after.contains("[buyer_actor]"));
    assert!(after.contains("source = \"order_rebind\""));
    assert!(after.contains(format!("order_id = \"{rebound_order_id}\"").as_str()));
    assert!(after.contains(format!("quote_id = \"quote_{rebound_order_id}\"").as_str()));

    let get = sandbox.json_success(&["--format", "json", "order", "get", rebound_order_id]);
    assert_eq!(get["result"]["state"], "ready");
    assert_eq!(get["result"]["buyer_account_id"], second_account_id);
    assert_eq!(get["result"]["buyer_actor_source"], "order_rebind");

    let same = sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "order",
        "rebind",
        rebound_order_id,
        second_account_id,
    ]);
    assert_eq!(same["result"]["state"], "rebound");
    assert_eq!(same["result"]["order_id_changed"], false);
    assert_eq!(same["result"]["to_order_id"], rebound_order_id);
}

#[test]
fn order_rebind_refuses_visible_published_request() {
    let sandbox = RadrootsCliSandbox::new();
    let buyer = identity_secret(94);
    let buyer_public = buyer.to_public();
    let buyer_public_file =
        write_public_identity_profile(&sandbox, "rebind-visible-buyer", &buyer_public);
    sandbox.json_success(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "account",
        "import",
        "--default",
        buyer_public_file.to_string_lossy().as_ref(),
    ]);
    let listing_event_id = seed_orderable_listing(&sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", "visible_rebind"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "visible_rebind",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "visible_rebind",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");
    let economics: RadrootsTradeOrderEconomics =
        serde_json::from_value(quote["result"]["quote"]["economics"].clone())
            .expect("quote economics");
    let event = signed_order_request_event_for_quote(
        &buyer,
        order_id,
        listing_event_id.as_str(),
        economics,
    );
    let target = sandbox.json_success(&["--format", "json", "account", "create"]);
    let target_account_id = target["result"]["account"]["id"]
        .as_str()
        .expect("target account id");
    let relay = RelayFetchServer::with_events(vec![event]);

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "--relay",
        relay.endpoint(),
        "order",
        "rebind",
        order_id,
        target_account_id,
    ]);
    relay.join();

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(10));
    assert_eq!(value["operation_id"], "order.rebind");
    assert_eq!(value["errors"][0]["code"], "validation_failed");
    assert_eq!(
        value["errors"][0]["detail"]["existing_request_check"],
        "blocked_existing_request"
    );
    assert_eq!(
        value["errors"][0]["detail"]["existing_request_event_ids"]
            .as_array()
            .expect("existing request ids")
            .len(),
        1
    );
}

#[test]
fn order_submit_requires_local_replica_freshness_before_signing() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "freshness_missing_db");
    fs::remove_file(sandbox.replica_db_path()).expect("remove replica db");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id.as_str(),
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(value["errors"][0]["detail"]["state"], "unconfigured");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["field"],
        "order.listing_addr"
    );
    assert!(
        value["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains("run `radroots store init` and `radroots market refresh`")
    );
}

#[test]
fn order_submit_dry_run_requires_local_replica_freshness() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "dry_freshness_missing_db");
    fs::remove_file(sandbox.replica_db_path()).expect("remove replica db");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--dry-run",
        "order",
        "submit",
        order_id.as_str(),
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(value["errors"][0]["detail"]["state"], "unconfigured");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["field"],
        "order.listing_addr"
    );
}

#[test]
fn order_submit_rejects_missing_or_archived_local_listing_before_publish() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "freshness_missing_listing");
    remove_orderable_listing(&sandbox, LISTING_ADDR);

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id.as_str(),
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["field"],
        "order.listing_addr"
    );
    assert!(
        value["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains("listing is not active")
    );
}

#[test]
fn order_submit_rejects_superseded_local_listing_event_before_publish() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "freshness_superseded_listing");
    let replacement_event_id = "3".repeat(64);
    replace_latest_listing_event_id(&sandbox, LISTING_ADDR, replacement_event_id.as_str());

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id.as_str(),
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "operation_unavailable");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["field"],
        "order.listing_event_id"
    );
    assert!(
        value["errors"][0]["detail"]["issues"][0]["message"]
            .as_str()
            .expect("issue message")
            .contains(replacement_event_id.as_str())
    );
}

#[test]
fn order_submit_rejects_over_available_quantity_before_publish() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    seed_orderable_listing(&sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", "over_quantity"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "over_quantity",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "6",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "over_quantity",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id,
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(10));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "validation_failed");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["code"],
        "order_quantity_exceeds_available"
    );
    assert!(
        value["errors"][0]["detail"]["issues"][0]["message"]
            .as_str()
            .expect("issue message")
            .contains("available quantity 5")
    );
    assert_no_removed_command_reference(&value, &["order", "submit"]);
    assert_no_daemon_runtime_reference(&value, &["order", "submit"]);
}

#[test]
fn order_submit_rejects_unknown_local_listing_bin_before_publish() {
    let sandbox = RadrootsCliSandbox::new();
    let order_id = create_ready_order(&sandbox, "unknown_bin");
    rewrite_order_bin(&sandbox, order_id.as_str(), "unknown-bin");

    let (output, value) = sandbox.json_output(&[
        "--format",
        "json",
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id.as_str(),
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(10));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["errors"][0]["code"], "validation_failed");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["code"],
        "order_bin_unknown"
    );
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["field"],
        "order.items[0].bin_id"
    );
    assert!(
        value["errors"][0]["detail"]["issues"][0]["message"]
            .as_str()
            .expect("issue message")
            .contains("expected primary bin `bin-1`")
    );
    assert_no_removed_command_reference(&value, &["order", "submit"]);
    assert_no_daemon_runtime_reference(&value, &["order", "submit"]);
}

#[test]
fn order_submit_dry_run_rejects_over_available_quantity_before_relay_preflight() {
    let sandbox = RadrootsCliSandbox::new();
    sandbox.json_success(&["--format", "json", "account", "create"]);
    seed_orderable_listing(&sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", "dry_over_quantity"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "dry_over_quantity",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "6",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "dry_over_quantity",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");

    let (output, value) =
        sandbox.json_output(&["--format", "json", "--dry-run", "order", "submit", order_id]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(10));
    assert_eq!(value["operation_id"], "order.submit");
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["errors"][0]["code"], "validation_failed");
    assert_eq!(
        value["errors"][0]["detail"]["issues"][0]["code"],
        "order_quantity_exceeds_available"
    );
}

#[test]
fn ready_order_submit_dry_run_validates_local_buyer_authority() {
    let sandbox = RadrootsCliSandbox::new();
    let first = sandbox.json_success(&["--format", "json", "account", "create"]);
    let first_account_id = first["result"]["account"]["id"]
        .as_str()
        .expect("first account id");
    let listing_event_id = seed_orderable_listing(&sandbox, LISTING_ADDR);
    sandbox.json_success(&["--format", "json", "basket", "create", "ready_order"]);
    sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "item",
        "add",
        "ready_order",
        "--listing-addr",
        LISTING_ADDR,
        "--bin-id",
        "bin-1",
        "--quantity",
        "2",
    ]);
    let quote = sandbox.json_success(&[
        "--format",
        "json",
        "basket",
        "quote",
        "create",
        "ready_order",
    ]);
    let order_id = quote["result"]["quote"]["order_id"]
        .as_str()
        .expect("order id");
    assert_eq!(quote["result"]["quote"]["ready_for_submit"], true);
    assert_eq!(
        quote["result"]["order"]["buyer_account_id"],
        first_account_id
    );
    assert_eq!(
        quote["result"]["order"]["listing_event_id"],
        listing_event_id
    );

    let (dry_output, dry_run) =
        sandbox.json_output(&["--format", "json", "--dry-run", "order", "submit", order_id]);

    assert!(!dry_output.status.success());
    assert_eq!(dry_output.status.code(), Some(8));
    assert_eq!(dry_run["operation_id"], "order.submit");
    assert_eq!(dry_run["dry_run"], true);
    assert_eq!(dry_run["result"], Value::Null);
    assert_eq!(dry_run["errors"][0]["code"], "network_unavailable");
    assert!(
        dry_run["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains(
                "order submit requires at least one configured relay before publish preflight"
            )
    );
    assert_no_daemon_runtime_reference(&dry_run, &["order", "submit", "--dry-run"]);

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
    let (drift_output, drift) =
        sandbox.json_output(&["--format", "json", "--dry-run", "order", "submit", order_id]);
    assert!(!drift_output.status.success());
    assert_eq!(drift_output.status.code(), Some(8));
    assert_eq!(drift["operation_id"], "order.submit");
    assert_eq!(drift["errors"][0]["code"], "network_unavailable");
    assert!(
        drift["errors"][0]["message"]
            .as_str()
            .expect("message")
            .contains(
                "order submit requires at least one configured relay before publish preflight"
            )
    );

    let (output, mismatch) = sandbox.json_output(&[
        "--format",
        "json",
        "--account-id",
        second_account_id,
        "--dry-run",
        "order",
        "submit",
        order_id,
    ]);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(mismatch["operation_id"], "order.submit");
    assert_eq!(mismatch["errors"][0]["code"], "account_mismatch");
    assert_eq!(mismatch["errors"][0]["detail"]["class"], "account");
    assert_no_removed_command_reference(&mismatch, &["order", "submit", "--dry-run"]);
    assert_no_daemon_runtime_reference(&mismatch, &["order", "submit", "--dry-run"]);

    let (network_output, network_mismatch) = sandbox.json_output(&[
        "--format",
        "json",
        "--account-id",
        second_account_id,
        "--relay",
        "ws://127.0.0.1:9",
        "--approval-token",
        "approve",
        "order",
        "submit",
        order_id,
    ]);

    assert!(!network_output.status.success());
    assert_eq!(network_output.status.code(), Some(5));
    assert_eq!(network_mismatch["operation_id"], "order.submit");
    assert_eq!(network_mismatch["result"], Value::Null);
    assert_eq!(network_mismatch["errors"][0]["code"], "account_mismatch");
    assert_eq!(network_mismatch["errors"][0]["detail"]["class"], "account");
    assert_no_daemon_runtime_reference(&network_mismatch, &["order", "submit"]);
}

#[test]
fn seller_target_flow_acceptance_uses_target_operations() {
    let sandbox = RadrootsCliSandbox::new();

    let account = sandbox.json_success(&["--format", "json", "account", "create"]);
    let account_id = account["result"]["account"]["id"]
        .as_str()
        .expect("account id");
    assert_eq!(account["operation_id"], "account.create");
    assert_eq!(account["result"]["account"]["signer"], "local");
    assert_eq!(account["result"]["account"]["custody"], "secret_backed");
    assert_eq!(account["result"]["account"]["write_capable"], true);
    assert_no_removed_command_reference(&account, &["account", "create"]);

    let signer = sandbox.json_success(&["--format", "json", "signer", "status", "get"]);
    assert_eq!(signer["operation_id"], "signer.status.get");
    assert_eq!(signer["result"]["mode"], "local");
    assert_eq!(signer["result"]["state"], "ready");
    assert_eq!(signer["result"]["signer_account_id"], account_id);
    assert_no_removed_command_reference(&signer, &["signer", "status", "get"]);

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
    assert_eq!(farm["operation_id"], "farm.create");
    assert_eq!(farm["result"]["state"], "saved");
    assert_no_removed_command_reference(&farm, &["farm", "create"]);

    let create = sandbox.json_success(&[
        "--format",
        "json",
        "listing",
        "create",
        "--key",
        "eggs",
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
    let listing_file = create["result"]["file"].as_str().expect("listing file");
    assert_eq!(create["operation_id"], "listing.create");
    assert!(Path::new(listing_file).exists());
    assert_no_removed_command_reference(&create, &["listing", "create"]);

    let list = sandbox.json_success(&["--format", "json", "listing", "list"]);
    assert_eq!(list["operation_id"], "listing.list");
    assert_eq!(list["result"]["state"], "ready");
    assert_eq!(list["result"]["count"], 1);
    assert_eq!(
        list["result"]["listings"][0]["id"],
        create["result"]["listing_id"]
    );
    assert_eq!(list["result"]["listings"][0]["state"], "ready");
    assert_no_removed_command_reference(&list, &["listing", "list"]);

    let validate = sandbox.json_success(&["--format", "json", "listing", "validate", listing_file]);
    assert_eq!(validate["operation_id"], "listing.validate");
    assert_eq!(validate["result"]["valid"], true);
    assert_eq!(validate["result"]["issues"], Value::Null);
    assert_no_removed_command_reference(&validate, &["listing", "validate"]);

    let publish = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "publish",
        listing_file,
    ]);
    assert_eq!(publish["operation_id"], "listing.publish");
    assert_eq!(publish["result"]["state"], "dry_run");
    assert_no_removed_command_reference(&publish, &["listing", "publish", "--dry-run"]);
    assert_no_daemon_runtime_reference(&publish, &["listing", "publish", "--dry-run"]);

    let archive = sandbox.json_success(&[
        "--format",
        "json",
        "--dry-run",
        "listing",
        "archive",
        listing_file,
    ]);
    assert_eq!(archive["operation_id"], "listing.archive");
    assert_eq!(archive["result"]["state"], "dry_run");
    assert_eq!(archive["result"]["operation"], "archive");
    assert_no_removed_command_reference(&archive, &["listing", "archive", "--dry-run"]);
    assert_no_daemon_runtime_reference(&archive, &["listing", "archive", "--dry-run"]);

    let (publish_output, unavailable_publish) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "publish",
        listing_file,
    ]);
    assert!(!publish_output.status.success());
    assert_eq!(unavailable_publish["operation_id"], "listing.publish");
    assert_eq!(
        unavailable_publish["errors"][0]["code"],
        "network_unavailable"
    );
    assert_eq!(
        unavailable_publish["errors"][0]["detail"]["class"],
        "network"
    );
    assert_no_removed_command_reference(&unavailable_publish, &["listing", "publish"]);
    assert_no_daemon_runtime_reference(&unavailable_publish, &["listing", "publish"]);

    let (archive_output, unavailable_archive) = sandbox.json_output(&[
        "--format",
        "json",
        "--approval-token",
        "approve",
        "listing",
        "archive",
        listing_file,
    ]);
    assert!(!archive_output.status.success());
    assert_eq!(unavailable_archive["operation_id"], "listing.archive");
    assert_eq!(
        unavailable_archive["errors"][0]["code"],
        "network_unavailable"
    );
    assert_eq!(
        unavailable_archive["errors"][0]["detail"]["class"],
        "network"
    );
    assert_no_removed_command_reference(&unavailable_archive, &["listing", "archive"]);
    assert_no_daemon_runtime_reference(&unavailable_archive, &["listing", "archive"]);
}
