use std::time::Duration;

use radroots_events::listing::RadrootsListing;
use radroots_events::trade::RadrootsTradeOrder;
use radroots_sdk::{
    RadrootsSdkConfig, RadrootsdAuth, SdkPublishError, SdkRadrootsdListingPublishOptions,
    SdkRadrootsdSignerAuthority, SdkRadrootsdSignerSessionRef, SdkTransportMode, SignerConfig,
};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::runtime::{
    CommandOutput, CommandView, JobDetailView, JobSummaryView, RpcSessionView, RpcSessionsView,
    RpcStatusView,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::provider;
use crate::runtime::signer::ActorWriteSignerAuthority;

const RPC_SOURCE: &str = "daemon rpc · durable write plane";
const BRIDGE_SOURCE: &str = "daemon bridge · durable write plane";
const RPC_TIMEOUT_SECS: u64 = 2;

#[derive(Debug)]
pub enum DaemonRpcError {
    Unconfigured(String),
    Unauthorized(String),
    MethodUnavailable(String),
    UnknownJob(String),
    External(String),
    InvalidResponse(String),
    Remote(String),
}

#[derive(Debug, Clone, Copy)]
enum RpcAuthMode {
    None,
    BridgeBearer,
}

#[derive(Debug, Clone)]
struct RpcTarget {
    url: String,
    bridge_bearer_token: Option<String>,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcResponseError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponseError {
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BridgeStatusRemote {
    enabled: bool,
    ready: bool,
    auth_mode: String,
    signer_mode: String,
    default_signer_mode: String,
    #[serde(default)]
    supported_signer_modes: Vec<String>,
    available_nip46_signer_sessions: usize,
    relay_count: usize,
    job_status_retention: usize,
    retained_jobs: usize,
    accepted_jobs: usize,
    published_jobs: usize,
    failed_jobs: usize,
    recovered_failed_jobs: usize,
    #[serde(default)]
    methods: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct BridgeJobRemote {
    job_id: String,
    command: String,
    status: String,
    terminal: bool,
    recovered_after_restart: bool,
    requested_at_unix: u64,
    completed_at_unix: Option<u64>,
    signer_mode: String,
    #[serde(default)]
    signer_session_id: Option<String>,
    event_id: Option<String>,
    event_addr: Option<String>,
    delivery_policy: String,
    delivery_quorum: Option<usize>,
    relay_count: usize,
    acknowledged_relay_count: usize,
    required_acknowledged_relay_count: usize,
    attempt_count: usize,
    relay_outcome_summary: String,
    #[serde(default)]
    attempt_summaries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Nip46SessionRemote {
    session_id: String,
    role: String,
    client_pubkey: String,
    signer_pubkey: String,
    user_pubkey: Option<String>,
    #[serde(default)]
    relays: Vec<String>,
    #[serde(default)]
    permissions: Vec<String>,
    auth_required: bool,
    authorized: bool,
    expires_in_secs: Option<u64>,
    #[serde(default)]
    signer_authority: Option<ActorWriteSignerAuthority>,
}

#[derive(Debug, Clone)]
pub struct BridgeListingPublishResult {
    pub deduplicated: bool,
    pub job_id: String,
    pub idempotency_key: Option<String>,
    pub status: String,
    pub signer_mode: String,
    pub signer_session_id: Option<String>,
    pub event_kind: Option<u32>,
    pub event_id: Option<String>,
    pub event_addr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeOrderRequestResult {
    pub deduplicated: bool,
    pub job_id: String,
    pub idempotency_key: Option<String>,
    pub status: String,
    pub signer_mode: String,
    pub signer_session_id: Option<String>,
    pub event_id: Option<String>,
    pub event_addr: Option<String>,
}

pub fn status(config: &RuntimeConfig) -> CommandOutput {
    match bridge_status(config) {
        Ok(status) => CommandOutput::success(CommandView::RpcStatus(RpcStatusView {
            state: if status.ready {
                "ready".to_owned()
            } else {
                "degraded".to_owned()
            },
            source: RPC_SOURCE.to_owned(),
            url: config.rpc.url.clone(),
            reason: if status.ready {
                None
            } else {
                Some("bridge is reachable but not ready for durable publish traffic".to_owned())
            },
            auth_mode: Some(status.auth_mode),
            signer_mode: Some(status.signer_mode),
            default_signer_mode: Some(status.default_signer_mode),
            supported_signer_modes: status.supported_signer_modes,
            bridge_enabled: Some(status.enabled),
            bridge_ready: Some(status.ready),
            relay_count: Some(status.relay_count),
            available_nip46_signer_sessions: Some(status.available_nip46_signer_sessions),
            job_status_retention: Some(status.job_status_retention),
            retained_jobs: Some(status.retained_jobs),
            accepted_jobs: Some(status.accepted_jobs),
            published_jobs: Some(status.published_jobs),
            failed_jobs: Some(status.failed_jobs),
            recovered_failed_jobs: Some(status.recovered_failed_jobs),
            session_surface_enabled: status
                .methods
                .iter()
                .any(|method| method == "nip46.session.list"),
            methods_count: status.methods.len(),
            actions: if status.ready {
                Vec::new()
            } else {
                vec!["radroots relay ls".to_owned()]
            },
        })),
        Err(DaemonRpcError::Unconfigured(reason))
        | Err(DaemonRpcError::Unauthorized(reason))
        | Err(DaemonRpcError::MethodUnavailable(reason)) => {
            CommandOutput::unconfigured(CommandView::RpcStatus(RpcStatusView {
                state: "unconfigured".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                reason: Some(reason),
                auth_mode: None,
                signer_mode: None,
                default_signer_mode: None,
                supported_signer_modes: Vec::new(),
                bridge_enabled: None,
                bridge_ready: None,
                relay_count: None,
                available_nip46_signer_sessions: None,
                job_status_retention: None,
                retained_jobs: None,
                accepted_jobs: None,
                published_jobs: None,
                failed_jobs: None,
                recovered_failed_jobs: None,
                session_surface_enabled: false,
                methods_count: 0,
                actions: vec![
                    "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                    "start radrootsd with bridge ingress enabled".to_owned(),
                ],
            }))
        }
        Err(DaemonRpcError::External(reason)) => {
            CommandOutput::external_unavailable(CommandView::RpcStatus(RpcStatusView {
                state: "unavailable".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                reason: Some(reason),
                auth_mode: None,
                signer_mode: None,
                default_signer_mode: None,
                supported_signer_modes: Vec::new(),
                bridge_enabled: None,
                bridge_ready: None,
                relay_count: None,
                available_nip46_signer_sessions: None,
                job_status_retention: None,
                retained_jobs: None,
                accepted_jobs: None,
                published_jobs: None,
                failed_jobs: None,
                recovered_failed_jobs: None,
                session_surface_enabled: false,
                methods_count: 0,
                actions: vec!["start radrootsd and verify the rpc url".to_owned()],
            }))
        }
        Err(DaemonRpcError::InvalidResponse(reason)) | Err(DaemonRpcError::Remote(reason)) => {
            CommandOutput::internal_error(CommandView::RpcStatus(RpcStatusView {
                state: "error".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                reason: Some(reason),
                auth_mode: None,
                signer_mode: None,
                default_signer_mode: None,
                supported_signer_modes: Vec::new(),
                bridge_enabled: None,
                bridge_ready: None,
                relay_count: None,
                available_nip46_signer_sessions: None,
                job_status_retention: None,
                retained_jobs: None,
                accepted_jobs: None,
                published_jobs: None,
                failed_jobs: None,
                recovered_failed_jobs: None,
                session_surface_enabled: false,
                methods_count: 0,
                actions: vec!["inspect the daemon rpc response contract".to_owned()],
            }))
        }
        Err(DaemonRpcError::UnknownJob(reason)) => {
            CommandOutput::internal_error(CommandView::RpcStatus(RpcStatusView {
                state: "error".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                reason: Some(reason),
                auth_mode: None,
                signer_mode: None,
                default_signer_mode: None,
                supported_signer_modes: Vec::new(),
                bridge_enabled: None,
                bridge_ready: None,
                relay_count: None,
                available_nip46_signer_sessions: None,
                job_status_retention: None,
                retained_jobs: None,
                accepted_jobs: None,
                published_jobs: None,
                failed_jobs: None,
                recovered_failed_jobs: None,
                session_surface_enabled: false,
                methods_count: 0,
                actions: Vec::new(),
            }))
        }
    }
}

pub fn sessions(config: &RuntimeConfig) -> CommandOutput {
    match nip46_sessions(config) {
        Ok(sessions) => {
            let entries = sessions
                .into_iter()
                .map(map_session_view)
                .collect::<Vec<_>>();
            let state = if entries.is_empty() { "empty" } else { "ready" };
            CommandOutput::success(CommandView::RpcSessions(RpcSessionsView {
                state: state.to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                count: entries.len(),
                reason: None,
                sessions: entries,
                actions: Vec::new(),
            }))
        }
        Err(DaemonRpcError::MethodUnavailable(reason)) => {
            CommandOutput::unconfigured(CommandView::RpcSessions(RpcSessionsView {
                state: "unconfigured".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                count: 0,
                reason: Some(reason),
                sessions: Vec::new(),
                actions: vec!["enable nip46.public_jsonrpc_enabled in radrootsd".to_owned()],
            }))
        }
        Err(DaemonRpcError::External(reason)) => {
            CommandOutput::external_unavailable(CommandView::RpcSessions(RpcSessionsView {
                state: "unavailable".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                count: 0,
                reason: Some(reason),
                sessions: Vec::new(),
                actions: vec!["start radrootsd and verify the rpc url".to_owned()],
            }))
        }
        Err(DaemonRpcError::Unconfigured(reason))
        | Err(DaemonRpcError::Unauthorized(reason))
        | Err(DaemonRpcError::InvalidResponse(reason))
        | Err(DaemonRpcError::Remote(reason))
        | Err(DaemonRpcError::UnknownJob(reason)) => {
            CommandOutput::internal_error(CommandView::RpcSessions(RpcSessionsView {
                state: "error".to_owned(),
                source: RPC_SOURCE.to_owned(),
                url: config.rpc.url.clone(),
                count: 0,
                reason: Some(reason),
                sessions: Vec::new(),
                actions: Vec::new(),
            }))
        }
    }
}

pub fn bridge_job_list(config: &RuntimeConfig) -> Result<Vec<JobSummaryView>, DaemonRpcError> {
    bridge_jobs(config).map(|jobs| jobs.into_iter().map(map_job_summary_view).collect())
}

pub fn bridge_job(
    config: &RuntimeConfig,
    job_id: &str,
) -> Result<Option<JobDetailView>, DaemonRpcError> {
    match bridge_job_status(config, job_id) {
        Ok(job) => Ok(Some(map_job_detail_view(job))),
        Err(DaemonRpcError::UnknownJob(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn bridge_listing_publish(
    config: &RuntimeConfig,
    listing: &RadrootsListing,
    kind: u32,
    idempotency_key: Option<&str>,
    signer_session_id: Option<&str>,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> Result<BridgeListingPublishResult, DaemonRpcError> {
    if kind != 30402 {
        return Err(DaemonRpcError::External(format!(
            "sdk listing publish only supports kind 30402, got {kind}"
        )));
    }

    let Some(signer_session_id) = signer_session_id else {
        return Err(DaemonRpcError::Unconfigured(
            "listing publish requires a signer session id".to_owned(),
        ));
    };

    let sdk = actor_write_sdk_client(config)?;
    let session = SdkRadrootsdSignerSessionRef::from_session_id(signer_session_id.to_owned());
    let mut options = SdkRadrootsdListingPublishOptions::from_signer_session_ref(&session);
    if let Some(idempotency_key) = idempotency_key {
        options = options.with_idempotency_key(idempotency_key.to_owned());
    }
    if let Some(signer_authority) = signer_authority {
        options = options.with_signer_authority(sdk_signer_authority(signer_authority));
    }

    let receipt = block_on_sdk(sdk.listing().publish_listing_via_radrootsd_with_options(
        listing,
        &options,
    ))?
    .map_err(map_sdk_publish_error)?;

    map_listing_publish_receipt(receipt, idempotency_key)
}

pub fn bridge_order_request(
    config: &RuntimeConfig,
    order: &RadrootsTradeOrder,
    idempotency_key: Option<&str>,
    signer_session_id: Option<&str>,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> Result<BridgeOrderRequestResult, DaemonRpcError> {
    let Some(signer_session_id) = signer_session_id else {
        return Err(DaemonRpcError::Unconfigured(
            "order publish requires a signer session id".to_owned(),
        ));
    };

    let sdk = actor_write_sdk_client(config)?;
    let session = SdkRadrootsdSignerSessionRef::from_session_id(signer_session_id.to_owned());
    let mut options = radroots_sdk::SdkRadrootsdOrderRequestPublishOptions::from_signer_session_ref(
        &session,
    );
    if let Some(idempotency_key) = idempotency_key {
        options = options.with_idempotency_key(idempotency_key.to_owned());
    }
    if let Some(signer_authority) = signer_authority {
        options = options.with_signer_authority(sdk_signer_authority(signer_authority));
    }

    let receipt = block_on_sdk(
        sdk.trade()
            .publish_order_request_via_radrootsd_with_options(order, &options),
    )?
    .map_err(map_sdk_publish_error)?;

    map_order_request_receipt(receipt, idempotency_key)
}

fn bridge_status(config: &RuntimeConfig) -> Result<BridgeStatusRemote, DaemonRpcError> {
    call(
        &default_target(config),
        "bridge.status",
        None,
        RpcAuthMode::BridgeBearer,
    )
}

fn bridge_jobs(config: &RuntimeConfig) -> Result<Vec<BridgeJobRemote>, DaemonRpcError> {
    call(
        &default_target(config),
        "bridge.job.list",
        None,
        RpcAuthMode::BridgeBearer,
    )
}

fn bridge_job_status(
    config: &RuntimeConfig,
    job_id: &str,
) -> Result<BridgeJobRemote, DaemonRpcError> {
    call(
        &default_target(config),
        "bridge.job.status",
        Some(serde_json::json!({ "job_id": job_id })),
        RpcAuthMode::BridgeBearer,
    )
}

fn nip46_sessions(config: &RuntimeConfig) -> Result<Vec<Nip46SessionRemote>, DaemonRpcError> {
    nip46_sessions_with_target(&default_target(config))
}

fn nip46_sessions_with_target(
    target: &RpcTarget,
) -> Result<Vec<Nip46SessionRemote>, DaemonRpcError> {
    call(target, "nip46.session.list", None, RpcAuthMode::None)
}

fn actor_write_target(config: &RuntimeConfig) -> Result<RpcTarget, DaemonRpcError> {
    let resolved =
        provider::resolve_actor_write_plane_target(config).map_err(DaemonRpcError::Unconfigured)?;
    Ok(RpcTarget {
        url: resolved.url,
        bridge_bearer_token: Some(resolved.bridge_bearer_token),
    })
}

fn default_target(config: &RuntimeConfig) -> RpcTarget {
    RpcTarget {
        url: config.rpc.url.clone(),
        bridge_bearer_token: config.rpc.bridge_bearer_token.clone(),
    }
}

fn actor_write_sdk_client(config: &RuntimeConfig) -> Result<radroots_sdk::RadrootsSdkClient, DaemonRpcError> {
    let target = actor_write_target(config)?;
    let mut sdk_config = RadrootsSdkConfig::custom();
    sdk_config.transport = SdkTransportMode::Radrootsd;
    sdk_config.signer = SignerConfig::Nip46;
    sdk_config.radrootsd.endpoint = Some(target.url);
    let Some(bridge_bearer_token) = target.bridge_bearer_token else {
        return Err(DaemonRpcError::Unconfigured(
            "actor write plane target is missing a bridge bearer token".to_owned(),
        ));
    };
    sdk_config.radrootsd.auth = RadrootsdAuth::BearerToken(bridge_bearer_token);
    radroots_sdk::RadrootsSdkClient::from_config(sdk_config)
        .map_err(|err| DaemonRpcError::Unconfigured(err.to_string()))
}

fn sdk_signer_authority(value: &ActorWriteSignerAuthority) -> SdkRadrootsdSignerAuthority {
    SdkRadrootsdSignerAuthority {
        provider_runtime_id: value.provider_runtime_id.clone(),
        account_identity_id: value.account_identity_id.clone(),
        provider_signer_session_id: value.provider_signer_session_id.clone(),
    }
}

fn map_sdk_publish_error(error: SdkPublishError) -> DaemonRpcError {
    match error {
        SdkPublishError::Config(err) => DaemonRpcError::Unconfigured(err.to_string()),
        SdkPublishError::Radrootsd(message) => DaemonRpcError::Remote(message),
        other => DaemonRpcError::External(other.to_string()),
    }
}

fn map_listing_publish_receipt(
    receipt: radroots_sdk::SdkPublishReceipt,
    idempotency_key: Option<&str>,
) -> Result<BridgeListingPublishResult, DaemonRpcError> {
    let radroots_sdk::SdkTransportReceipt::Radrootsd(transport_receipt) = receipt.transport_receipt else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk listing publish returned a non-radrootsd transport receipt".to_owned(),
        ));
    };
    let Some(job_id) = transport_receipt.job_id else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk listing publish did not return a job id".to_owned(),
        ));
    };
    let Some(status) = transport_receipt.status else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk listing publish did not return a job status".to_owned(),
        ));
    };
    let Some(signer_mode) = transport_receipt.signer_mode else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk listing publish did not return a signer mode".to_owned(),
        ));
    };
    Ok(BridgeListingPublishResult {
        deduplicated: transport_receipt.deduplicated,
        job_id,
        idempotency_key: idempotency_key.map(str::to_owned),
        status,
        signer_mode,
        signer_session_id: transport_receipt.signer_session_id,
        event_kind: receipt.event_kind,
        event_id: receipt.event_id,
        event_addr: transport_receipt.event_addr,
    })
}

fn map_order_request_receipt(
    receipt: radroots_sdk::SdkPublishReceipt,
    idempotency_key: Option<&str>,
) -> Result<BridgeOrderRequestResult, DaemonRpcError> {
    let radroots_sdk::SdkTransportReceipt::Radrootsd(transport_receipt) = receipt.transport_receipt else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk order publish returned a non-radrootsd transport receipt".to_owned(),
        ));
    };
    let Some(job_id) = transport_receipt.job_id else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk order publish did not return a job id".to_owned(),
        ));
    };
    let Some(status) = transport_receipt.status else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk order publish did not return a job status".to_owned(),
        ));
    };
    let Some(signer_mode) = transport_receipt.signer_mode else {
        return Err(DaemonRpcError::InvalidResponse(
            "sdk order publish did not return a signer mode".to_owned(),
        ));
    };
    Ok(BridgeOrderRequestResult {
        deduplicated: transport_receipt.deduplicated,
        job_id,
        idempotency_key: idempotency_key.map(str::to_owned),
        status,
        signer_mode,
        signer_session_id: transport_receipt.signer_session_id,
        event_id: receipt.event_id,
        event_addr: transport_receipt.event_addr,
    })
}

fn block_on_sdk<F, T>(future: F) -> Result<T, DaemonRpcError>
where
    F: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| DaemonRpcError::External(format!("build sdk runtime: {err}")))?;
    Ok(runtime.block_on(future))
}

pub fn resolve_signer_session_id(
    config: &RuntimeConfig,
    actor_role: &str,
    actor_pubkey: &str,
    event_kind: u32,
    requested_session_id: Option<&str>,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> Result<String, DaemonRpcError> {
    let target = actor_write_target(config)?;
    let sessions = nip46_sessions_with_target(&target)?;

    if let Some(session_id) = requested_session_id {
        let Some(session) = sessions
            .into_iter()
            .find(|session| session.session_id == session_id)
        else {
            return Err(DaemonRpcError::Unconfigured(format!(
                "requested signer session `{session_id}` was not found"
            )));
        };
        validate_signer_session(
            &session,
            actor_role,
            actor_pubkey,
            event_kind,
            signer_authority,
        )?;
        return Ok(session.session_id);
    }

    let mut matches = sessions
        .into_iter()
        .filter(|session| {
            session_matches_actor(session, actor_pubkey, event_kind, signer_authority)
        })
        .map(|session| session.session_id)
        .collect::<Vec<_>>();

    match matches.len() {
        1 => Ok(matches.pop().expect("exactly one signer session")),
        0 => Err(DaemonRpcError::Unconfigured(format!(
            "no authorized signer session matched {actor_role} pubkey `{actor_pubkey}` for sign_event:{event_kind}; connect a signer session or pass --signer-session-id"
        ))),
        _ => Err(DaemonRpcError::Unconfigured(format!(
            "multiple authorized signer sessions matched {actor_role} pubkey `{actor_pubkey}` for sign_event:{event_kind}; pass --signer-session-id"
        ))),
    }
}

fn validate_signer_session(
    session: &Nip46SessionRemote,
    actor_role: &str,
    actor_pubkey: &str,
    event_kind: u32,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> Result<(), DaemonRpcError> {
    if !session.authorized {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` is not authorized",
            session.session_id
        )));
    }
    if !session.signer_pubkey.eq_ignore_ascii_case(actor_pubkey) {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` signer pubkey `{}` does not match {actor_role} pubkey `{actor_pubkey}`",
            session.session_id, session.signer_pubkey
        )));
    }
    if !sign_event_allowed(&session.permissions, event_kind) {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` is not approved for sign_event:{event_kind}",
            session.session_id
        )));
    }
    validate_signer_authority(session, signer_authority)?;
    Ok(())
}

fn session_matches_actor(
    session: &Nip46SessionRemote,
    actor_pubkey: &str,
    event_kind: u32,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> bool {
    session.authorized
        && session.signer_pubkey.eq_ignore_ascii_case(actor_pubkey)
        && sign_event_allowed(&session.permissions, event_kind)
        && signer_authority_matches(session, signer_authority)
}

fn validate_signer_authority(
    session: &Nip46SessionRemote,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> Result<(), DaemonRpcError> {
    let Some(expected) = signer_authority else {
        return Ok(());
    };
    let Some(actual) = session.signer_authority.as_ref() else {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` is missing signer authority continuity metadata",
            session.session_id
        )));
    };
    if actual.provider_runtime_id != expected.provider_runtime_id {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` provider `{}` does not match required provider `{}`",
            session.session_id, actual.provider_runtime_id, expected.provider_runtime_id
        )));
    }
    if actual.account_identity_id != expected.account_identity_id {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` account identity `{}` does not match required account `{}`",
            session.session_id, actual.account_identity_id, expected.account_identity_id
        )));
    }
    if actual.provider_signer_session_id != expected.provider_signer_session_id {
        return Err(DaemonRpcError::Unconfigured(format!(
            "requested signer session `{}` provider signer session `{}` does not match required provider session `{}`",
            session.session_id,
            actual
                .provider_signer_session_id
                .as_deref()
                .unwrap_or("<none>"),
            expected
                .provider_signer_session_id
                .as_deref()
                .unwrap_or("<none>")
        )));
    }
    Ok(())
}

fn signer_authority_matches(
    session: &Nip46SessionRemote,
    signer_authority: Option<&ActorWriteSignerAuthority>,
) -> bool {
    validate_signer_authority(session, signer_authority).is_ok()
}

fn sign_event_allowed(perms: &[String], kind: u32) -> bool {
    perms.iter().any(|entry| entry == "sign_event")
        || perms
            .iter()
            .any(|entry| entry == &format!("sign_event:{kind}"))
}

fn call<T: DeserializeOwned>(
    target: &RpcTarget,
    method: &str,
    params: Option<Value>,
    auth_mode: RpcAuthMode,
) -> Result<T, DaemonRpcError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(RPC_TIMEOUT_SECS))
        .build()
        .map_err(|error| DaemonRpcError::InvalidResponse(format!("build rpc client: {error}")))?;

    let mut request = client.post(target.url.as_str()).json(&JsonRpcRequest {
        jsonrpc: "2.0",
        id: 1,
        method,
        params,
    });

    if matches!(auth_mode, RpcAuthMode::BridgeBearer) {
        let Some(token) = target.bridge_bearer_token.as_deref() else {
            return Err(DaemonRpcError::Unconfigured(
                "bridge bearer token is not configured".to_owned(),
            ));
        };
        request = request.bearer_auth(token);
    }

    let response = request.send().map_err(|error| {
        DaemonRpcError::External(format!(
            "failed to reach daemon rpc at {}: {error}",
            target.url
        ))
    })?;
    let status = response.status();
    let body = response.text().map_err(|error| {
        DaemonRpcError::InvalidResponse(format!("read daemon rpc response: {error}"))
    })?;
    if !status.is_success() {
        return Err(DaemonRpcError::External(format!(
            "daemon rpc returned http {}",
            status.as_u16()
        )));
    }

    let envelope: JsonRpcResponse<T> = serde_json::from_str(body.as_str()).map_err(|error| {
        DaemonRpcError::InvalidResponse(format!("parse daemon rpc response: {error}"))
    })?;
    if let Some(result) = envelope.result {
        return Ok(result);
    }
    let Some(error) = envelope.error else {
        return Err(DaemonRpcError::InvalidResponse(
            "daemon rpc response did not include a result".to_owned(),
        ));
    };
    Err(map_rpc_error(method, error))
}

fn map_rpc_error(method: &str, error: JsonRpcResponseError) -> DaemonRpcError {
    match error.code {
        -32601 => DaemonRpcError::MethodUnavailable(error.message),
        -32001 => DaemonRpcError::Unauthorized(error.message),
        -32000
            if method == "bridge.job.status"
                && error.message.starts_with("unknown bridge job:") =>
        {
            DaemonRpcError::UnknownJob(error.message)
        }
        -32000 => DaemonRpcError::Remote(error.message),
        _ => DaemonRpcError::InvalidResponse(format!(
            "daemon rpc returned unexpected error {}: {}",
            error.code, error.message
        )),
    }
}

fn map_job_command(command: String) -> String {
    match command.as_str() {
        "bridge.listing.publish" => "listing.publish".to_owned(),
        "bridge.order.request" => "order.submit".to_owned(),
        other => other.to_owned(),
    }
}

fn map_job_summary_view(job: BridgeJobRemote) -> JobSummaryView {
    JobSummaryView {
        id: job.job_id,
        command: map_job_command(job.command),
        state: job.status,
        terminal: job.terminal,
        signer: job.signer_mode,
        signer_session_id: job.signer_session_id,
        requested_at_unix: job.requested_at_unix,
        completed_at_unix: job.completed_at_unix,
        recovered_after_restart: job.recovered_after_restart,
    }
}

fn map_job_detail_view(job: BridgeJobRemote) -> JobDetailView {
    JobDetailView {
        id: job.job_id,
        command: map_job_command(job.command),
        state: job.status,
        terminal: job.terminal,
        signer: job.signer_mode,
        signer_session_id: job.signer_session_id,
        requested_at_unix: job.requested_at_unix,
        completed_at_unix: job.completed_at_unix,
        recovered_after_restart: job.recovered_after_restart,
        event_id: job.event_id,
        event_addr: job.event_addr,
        delivery_policy: job.delivery_policy,
        delivery_quorum: job.delivery_quorum,
        relay_count: job.relay_count,
        acknowledged_relay_count: job.acknowledged_relay_count,
        required_acknowledged_relay_count: job.required_acknowledged_relay_count,
        attempt_count: job.attempt_count,
        relay_outcome_summary: job.relay_outcome_summary,
        attempt_summaries: job.attempt_summaries,
    }
}

fn map_session_view(session: Nip46SessionRemote) -> RpcSessionView {
    RpcSessionView {
        session_id: session.session_id,
        role: session.role,
        client_pubkey: session.client_pubkey,
        signer_pubkey: session.signer_pubkey,
        user_pubkey: session.user_pubkey,
        relay_count: session.relays.len(),
        permissions_count: session.permissions.len(),
        auth_required: session.auth_required,
        authorized: session.authorized,
        expires_in_secs: session.expires_in_secs,
    }
}

pub fn bridge_source() -> &'static str {
    BRIDGE_SOURCE
}
