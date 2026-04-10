#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::config::{
    CapabilityBindingTargetKind, HyfConfig, INFERENCE_HYF_STDIO_CAPABILITY, RuntimeConfig,
};

const HYF_STATUS_TIMEOUT: Duration = Duration::from_secs(1);
const HYF_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const HYF_STATUS_REQUEST_ID: &str = "cli-doctor-hyf-status";
const HYF_CAPABILITIES_REQUEST_ID: &str = "cli-runtime-hyf-capabilities";
const HYF_SOURCE: &str = "hyf status control request · local first";
const HYF_PROTOCOL_VERSION: u64 = 1;
const HYF_CONSUMER: &str = "radroots-cli";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyfStatusView {
    pub executable: String,
    pub state: String,
    pub source: String,
    pub reason: Option<String>,
    pub protocol_version: Option<u64>,
    pub deterministic_available: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyfClient {
    executable: PathBuf,
}

impl HyfClient {
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }

    pub fn executable(&self) -> &Path {
        self.executable.as_path()
    }

    pub fn status(&self) -> Result<HyfSuccess<HyfStatusOutput>, HyfClientError> {
        self.call(
            HYF_STATUS_REQUEST_ID,
            Some(HYF_STATUS_REQUEST_ID),
            "sys.status",
            None,
            &HyfEmptyInput::default(),
        )
    }

    pub fn capabilities(&self) -> Result<HyfSuccess<HyfCapabilitiesOutput>, HyfClientError> {
        self.call(
            HYF_CAPABILITIES_REQUEST_ID,
            None,
            "sys.capabilities",
            None,
            &HyfEmptyInput::default(),
        )
    }

    pub fn query_rewrite(
        &self,
        request_id: &str,
        trace_id: Option<&str>,
        context: &HyfRequestContext,
        request: &HyfQueryRewriteRequest,
    ) -> Result<HyfSuccess<HyfQueryRewriteOutput>, HyfClientError> {
        self.call(
            request_id,
            trace_id,
            "query_rewrite",
            Some(context),
            request,
        )
    }

    pub fn semantic_rank(
        &self,
        request_id: &str,
        trace_id: Option<&str>,
        context: &HyfRequestContext,
        request: &HyfSemanticRankRequest,
    ) -> Result<HyfSuccess<HyfSemanticRankOutput>, HyfClientError> {
        self.call(
            request_id,
            trace_id,
            "semantic_rank",
            Some(context),
            request,
        )
    }

    pub fn explain_result(
        &self,
        request_id: &str,
        trace_id: Option<&str>,
        context: &HyfRequestContext,
        request: &HyfExplainResultRequest,
    ) -> Result<HyfSuccess<HyfExplainResultOutput>, HyfClientError> {
        self.call(
            request_id,
            trace_id,
            "explain_result",
            Some(context),
            request,
        )
    }

    fn call<TRequest, TResponse>(
        &self,
        request_id: &str,
        trace_id: Option<&str>,
        capability: &str,
        context: Option<&HyfRequestContext>,
        input: &TRequest,
    ) -> Result<HyfSuccess<TResponse>, HyfClientError>
    where
        TRequest: Serialize,
        TResponse: for<'de> Deserialize<'de>,
    {
        let request = serialize_request(request_id, trace_id, capability, context, input)
            .map_err(HyfClientError::SerializeRequest)?;

        let output = self.run_request(request.as_str())?;
        let stdout = String::from_utf8(output.stdout).map_err(HyfClientError::InvalidUtf8)?;
        let response: HyfWireResponse<TResponse> =
            serde_json::from_str(stdout.as_str()).map_err(HyfClientError::InvalidJson)?;

        if !response.ok {
            return Err(HyfClientError::RemoteError {
                code: response.error.as_ref().and_then(|error| error.code.clone()),
                message: response
                    .error
                    .as_ref()
                    .and_then(|error| error.message.clone()),
            });
        }

        let Some(output) = response.output else {
            return Err(HyfClientError::InvalidResponse(
                "hyf response omitted output for a successful request".to_owned(),
            ));
        };

        Ok(HyfSuccess {
            version: response.version,
            request_id: response.request_id,
            trace_id: response.trace_id,
            output,
            meta: response.meta,
        })
    }

    fn run_request(&self, request: &str) -> Result<Output, HyfClientError> {
        let mut child = Command::new(&self.executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => HyfClientError::NotFound,
                _ => HyfClientError::Start(error),
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{request}").map_err(HyfClientError::Write)?;
        }

        let output = collect_output_with_timeout(child)?;
        if !output.status.success() {
            return Err(HyfClientError::NonZeroExit {
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(output)
    }
}

fn serialize_request<TRequest: Serialize>(
    request_id: &str,
    trace_id: Option<&str>,
    capability: &str,
    context: Option<&HyfRequestContext>,
    input: &TRequest,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&HyfRequestEnvelope {
        version: HYF_PROTOCOL_VERSION,
        request_id,
        trace_id,
        capability,
        context,
        input,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyfSuccess<T> {
    pub version: u64,
    pub request_id: String,
    pub trace_id: Option<String>,
    pub output: T,
    pub meta: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum HyfClientError {
    #[error("hyf executable was not found")]
    NotFound,
    #[error("failed to start hyf request: {0}")]
    Start(std::io::Error),
    #[error("failed to write hyf request stdin: {0}")]
    Write(std::io::Error),
    #[error("failed to wait on hyf request: {0}")]
    Wait(std::io::Error),
    #[error("failed to read hyf request output: {0}")]
    Read(std::io::Error),
    #[error("hyf request timed out after {0}ms")]
    Timeout(u128),
    #[error("hyf request exited unsuccessfully")]
    NonZeroExit { status: Option<i32>, stderr: String },
    #[error("failed to serialize hyf request: {0}")]
    SerializeRequest(serde_json::Error),
    #[error("hyf response was not valid UTF-8: {0}")]
    InvalidUtf8(std::string::FromUtf8Error),
    #[error("hyf response was not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error("{0}")]
    InvalidResponse(String),
    #[error("hyf request returned a remote error")]
    RemoteError {
        code: Option<String>,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct HyfRequestContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_mode_preference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<HyfRequestScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_provenance: Option<bool>,
}

impl HyfRequestContext {
    pub fn deterministic_cli() -> Self {
        Self {
            consumer: Some(HYF_CONSUMER.to_owned()),
            execution_mode_preference: Some("deterministic".to_owned()),
            scope: None,
            return_provenance: None,
        }
    }

    pub fn with_return_provenance(mut self, return_provenance: bool) -> Self {
        self.return_provenance = Some(return_provenance);
        self
    }

    pub fn with_listing_scope(mut self, listing_ids: Vec<String>) -> Self {
        self.scope = if listing_ids.is_empty() {
            None
        } else {
            Some(HyfRequestScope { listing_ids })
        };
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct HyfRequestScope {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listing_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HyfQueryRewriteRequest {
    pub query: String,
}

impl HyfQueryRewriteRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HyfSemanticCandidate {
    pub id: String,
    pub title: String,
    pub farm: String,
    pub delivery: String,
    pub distance_km: f64,
    pub freshness_minutes: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HyfSemanticRankRequest {
    pub query: String,
    pub candidates: Vec<HyfSemanticCandidate>,
}

impl HyfSemanticRankRequest {
    pub fn new(query: impl Into<String>, candidates: Vec<HyfSemanticCandidate>) -> Self {
        Self {
            query: query.into(),
            candidates,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HyfExplainResultRequest {
    pub query: String,
    pub candidate: HyfSemanticCandidate,
}

impl HyfExplainResultRequest {
    pub fn new(query: impl Into<String>, candidate: HyfSemanticCandidate) -> Self {
        Self {
            query: query.into(),
            candidate,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfBuildIdentity {
    pub protocol_version: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfExecutionModes {
    pub deterministic: bool,
    #[serde(default)]
    pub assisted: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfStatusOutput {
    pub build_identity: HyfBuildIdentity,
    pub enabled_execution_modes: HyfExecutionModes,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfRequestContextContract {
    pub accepted_features: Vec<String>,
    pub effective_features: Vec<String>,
    pub unsupported_field_behavior: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfBusinessCapability {
    pub id: String,
    pub kind: String,
    pub deterministic_execution: String,
    pub implementation_status: String,
    pub callable: bool,
    pub implemented: bool,
    pub assisted_execution: String,
    pub assisted_backend_available: bool,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct HyfCapabilitiesOutput {
    pub control_routes: Vec<String>,
    pub business_capabilities: Vec<HyfBusinessCapability>,
    pub assisted_backend_capabilities: Vec<Value>,
    pub request_context_contract: HyfRequestContextContract,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfExtractedFilters {
    pub local_intent: bool,
    pub fulfillment: String,
    pub time_window: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfQueryRewriteOutput {
    pub original_text: String,
    pub normalized_text: String,
    pub rewritten_text: String,
    pub query_terms: Vec<String>,
    pub normalization_signals: Vec<String>,
    pub ranking_hints: Vec<String>,
    pub extracted_filters: HyfExtractedFilters,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfScoredCandidate {
    pub id: String,
    pub heuristic_score: i64,
    pub matched_terms: Vec<String>,
    pub reasons: Vec<String>,
    pub delivery_alignment: String,
    pub distance_band: String,
    pub freshness_band: String,
    pub scope_match: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfSemanticRankOutput {
    pub ranked_ids: Vec<String>,
    pub reasons: BTreeMap<String, Vec<String>>,
    pub scored_candidates: Vec<HyfScoredCandidate>,
    pub ranking_hints: Vec<String>,
    pub extracted_filters: HyfExtractedFilters,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfSignalAssessment {
    pub delivery_alignment: String,
    pub distance_band: String,
    pub freshness_band: String,
    pub scope_match: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HyfExplainResultOutput {
    pub result_id: String,
    pub explanation_kind: String,
    pub summary: String,
    pub score: i64,
    pub reasons: Vec<String>,
    pub matched_terms: Vec<String>,
    pub ranking_hints: Vec<String>,
    pub extracted_filters: HyfExtractedFilters,
    pub signal_assessment: HyfSignalAssessment,
}

#[derive(Debug, Clone, Serialize, Default)]
struct HyfEmptyInput {}

#[derive(Debug, Serialize)]
struct HyfRequestEnvelope<'a, T> {
    version: u64,
    request_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<&'a str>,
    capability: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<&'a HyfRequestContext>,
    input: &'a T,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct HyfWireResponse<T> {
    version: u64,
    request_id: String,
    #[serde(default)]
    trace_id: Option<String>,
    ok: bool,
    #[serde(default)]
    output: Option<T>,
    #[serde(default)]
    meta: Option<Value>,
    #[serde(default)]
    error: Option<HyfWireError>,
}

#[derive(Debug, Clone, Deserialize)]
struct HyfWireError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

pub fn resolve_runtime_client(config: &RuntimeConfig) -> Result<HyfClient, HyfStatusView> {
    if !config.hyf.enabled {
        return Err(disabled_status(config.hyf.executable.display().to_string()));
    }

    let Some(binding) = config.capability_binding(INFERENCE_HYF_STDIO_CAPABILITY) else {
        return resolve_client(&config.hyf);
    };

    match binding.target_kind {
        CapabilityBindingTargetKind::ExplicitEndpoint => resolve_client(&HyfConfig {
            enabled: true,
            executable: binding.target.clone().into(),
        }),
        CapabilityBindingTargetKind::ManagedInstance => Err(unavailable_status(
            config.hyf.executable.display().to_string(),
            format!(
                "configured hyf binding target `{}` uses unsupported target_kind `managed_instance`; use `explicit_endpoint` for `inference.hyf_stdio`",
                binding.target
            ),
            None,
            None,
        )),
    }
}

pub fn resolve_ready_runtime_client(config: &RuntimeConfig) -> Result<HyfClient, HyfStatusView> {
    let client = resolve_runtime_client(config)?;
    let status = resolve_status_for_client(&client);
    if status.state == "ready" {
        Ok(client)
    } else {
        Err(status)
    }
}

pub fn resolve_runtime_status(config: &RuntimeConfig) -> HyfStatusView {
    match resolve_runtime_client(config) {
        Ok(client) => resolve_status_for_client(&client),
        Err(view) => view,
    }
}

pub fn resolve_status(config: &HyfConfig) -> HyfStatusView {
    match resolve_client(config) {
        Ok(client) => resolve_status_for_client(&client),
        Err(view) => view,
    }
}

fn resolve_client(config: &HyfConfig) -> Result<HyfClient, HyfStatusView> {
    let executable = config.executable.display().to_string();
    if !config.enabled {
        return Err(disabled_status(executable));
    }

    if config.executable.as_os_str().is_empty() {
        return Err(unavailable_status(
            executable,
            "hyf executable path is not configured".to_owned(),
            None,
            None,
        ));
    }

    Ok(HyfClient::new(config.executable.clone()))
}

fn resolve_status_for_client(client: &HyfClient) -> HyfStatusView {
    let executable = client.executable().display().to_string();
    let response = match client.status() {
        Ok(response) => response,
        Err(HyfClientError::NotFound) => {
            return unavailable_status(
                executable,
                format!(
                    "hyf executable was not found at {}",
                    client.executable().display()
                ),
                None,
                None,
            );
        }
        Err(HyfClientError::Start(error)) => {
            return unavailable_status(
                executable,
                format!(
                    "failed to start hyf control request at {}: {error}",
                    client.executable().display()
                ),
                None,
                None,
            );
        }
        Err(HyfClientError::Write(error)) => {
            return unavailable_status(
                executable,
                format!("failed to write hyf control request stdin: {error}"),
                None,
                None,
            );
        }
        Err(HyfClientError::Timeout(timeout_ms)) => {
            return unavailable_status(
                executable,
                format!("hyf status control request timed out after {timeout_ms}ms"),
                None,
                None,
            );
        }
        Err(HyfClientError::Wait(error)) | Err(HyfClientError::Read(error)) => {
            return unavailable_status(
                executable,
                format!("failed to capture hyf status control output: {error}"),
                None,
                None,
            );
        }
        Err(HyfClientError::NonZeroExit { status, stderr }) => {
            return unavailable_status(
                executable,
                format_nonzero_exit("hyf status control request", status, stderr.as_str()),
                None,
                None,
            );
        }
        Err(HyfClientError::InvalidUtf8(error)) => {
            return unavailable_status(
                executable,
                format!("hyf status output was not valid UTF-8: {error}"),
                None,
                None,
            );
        }
        Err(HyfClientError::InvalidJson(error)) => {
            return unavailable_status(
                executable,
                format!("hyf status output was not valid JSON: {error}"),
                None,
                None,
            );
        }
        Err(HyfClientError::RemoteError { code, .. }) => {
            let reason = code
                .map(|code| format!("hyf status control request returned error code {code}"))
                .unwrap_or_else(|| {
                    "hyf status control request returned an invalid error response".to_owned()
                });
            return unavailable_status(executable, reason, None, None);
        }
        Err(HyfClientError::SerializeRequest(_) | HyfClientError::InvalidResponse(_)) => {
            return unavailable_status(
                executable,
                "hyf status control request returned an invalid error response".to_owned(),
                None,
                None,
            );
        }
    };

    let protocol_version = Some(response.output.build_identity.protocol_version);
    let deterministic_available = Some(response.output.enabled_execution_modes.deterministic);

    if response.version != HYF_PROTOCOL_VERSION {
        return unavailable_status(
            executable,
            format!(
                "hyf status response version {:?} is incompatible with cli expected {}",
                Some(response.version),
                HYF_PROTOCOL_VERSION
            ),
            protocol_version,
            deterministic_available,
        );
    }

    if response.request_id != HYF_STATUS_REQUEST_ID {
        return unavailable_status(
            executable,
            "hyf status response did not preserve the control request id".to_owned(),
            protocol_version,
            deterministic_available,
        );
    }

    if protocol_version != Some(HYF_PROTOCOL_VERSION) {
        return unavailable_status(
            executable,
            format!(
                "hyf protocol version {:?} is incompatible with cli expected {}",
                protocol_version, HYF_PROTOCOL_VERSION
            ),
            protocol_version,
            deterministic_available,
        );
    }

    if deterministic_available != Some(true) {
        return unavailable_status(
            executable,
            "hyf deterministic execution is unavailable".to_owned(),
            protocol_version,
            deterministic_available,
        );
    }

    HyfStatusView {
        executable,
        state: "ready".to_owned(),
        source: HYF_SOURCE.to_owned(),
        reason: Some("healthy · protocol 1 · deterministic available".to_owned()),
        protocol_version,
        deterministic_available,
    }
}

fn collect_output_with_timeout(mut child: Child) -> Result<Output, HyfClientError> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return collect_output(child, status),
            Ok(None) => {
                if started_at.elapsed() >= HYF_STATUS_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(HyfClientError::Timeout(HYF_STATUS_TIMEOUT.as_millis()));
                }
                thread::sleep(HYF_STATUS_POLL_INTERVAL);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HyfClientError::Wait(error));
            }
        }
    }
}

fn collect_output(mut child: Child, status: ExitStatus) -> Result<Output, HyfClientError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .map_err(HyfClientError::Read)?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)
            .map_err(HyfClientError::Read)?;
    }

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn disabled_status(executable: String) -> HyfStatusView {
    HyfStatusView {
        executable,
        state: "disabled".to_owned(),
        source: HYF_SOURCE.to_owned(),
        reason: Some("disabled by config".to_owned()),
        protocol_version: None,
        deterministic_available: None,
    }
}

fn unavailable_status(
    executable: String,
    reason: String,
    protocol_version: Option<u64>,
    deterministic_available: Option<bool>,
) -> HyfStatusView {
    HyfStatusView {
        executable,
        state: "unavailable".to_owned(),
        source: HYF_SOURCE.to_owned(),
        reason: Some(reason),
        protocol_version,
        deterministic_available,
    }
}

fn format_nonzero_exit(request_label: &str, status: Option<i32>, stderr: &str) -> String {
    match status {
        Some(code) if stderr.is_empty() => {
            format!("{request_label} exited with status code {code}")
        }
        Some(code) => {
            format!("{request_label} exited with status code {code}: {stderr}")
        }
        None if stderr.is_empty() => format!("{request_label} terminated by signal"),
        None => format!("{request_label} terminated by signal: {stderr}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HYF_PROTOCOL_VERSION, HyfClient, HyfEmptyInput, HyfExplainResultRequest,
        HyfQueryRewriteRequest, HyfRequestContext, HyfSemanticCandidate, HyfSemanticRankRequest,
        resolve_status,
    };
    use crate::runtime::config::HyfConfig;
    use serde::Serialize;
    use serde_json::Value;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn hyf_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn disabled_hyf_reports_disabled_state_without_spawning() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let view = resolve_status(&HyfConfig {
            enabled: false,
            executable: "hyfd".into(),
        });
        assert_eq!(view.state, "disabled");
        assert_eq!(view.reason.as_deref(), Some("disabled by config"));
    }

    #[test]
    fn healthy_hyf_status_reports_ready() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let dir = tempdir().expect("tempdir");
        let executable = write_response_script(
            dir.path(),
            format!(
                "{{\"version\":{HYF_PROTOCOL_VERSION},\"request_id\":\"cli-doctor-hyf-status\",\"trace_id\":\"cli-doctor-hyf-status\",\"ok\":true,\"output\":{{\"build_identity\":{{\"protocol_version\":{HYF_PROTOCOL_VERSION}}},\"enabled_execution_modes\":{{\"deterministic\":true}}}}}}"
            )
            .as_str(),
        );

        let view = resolve_status(&HyfConfig {
            enabled: true,
            executable,
        });
        assert_eq!(view.state, "ready", "reason: {:?}", view.reason);
        assert_eq!(view.protocol_version, Some(HYF_PROTOCOL_VERSION));
        assert_eq!(view.deterministic_available, Some(true));
    }

    #[test]
    fn incompatible_hyf_status_reports_unavailable() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let dir = tempdir().expect("tempdir");
        let executable = write_response_script(
            dir.path(),
            "{\"version\":1,\"request_id\":\"cli-doctor-hyf-status\",\"trace_id\":\"cli-doctor-hyf-status\",\"ok\":true,\"output\":{\"build_identity\":{\"protocol_version\":2},\"enabled_execution_modes\":{\"deterministic\":true}}}",
        );

        let view = resolve_status(&HyfConfig {
            enabled: true,
            executable,
        });
        assert_eq!(view.state, "unavailable", "reason: {:?}", view.reason);
        assert!(
            view.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("incompatible"))
        );
    }

    #[test]
    fn capabilities_request_uses_typed_client() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let dir = tempdir().expect("tempdir");
        let executable = write_response_script(
            dir.path(),
            "{\"version\":1,\"request_id\":\"cli-runtime-hyf-capabilities\",\"ok\":true,\"output\":{\"control_routes\":[\"sys.status\",\"sys.capabilities\"],\"business_capabilities\":[{\"id\":\"query_rewrite\",\"kind\":\"business\",\"deterministic_execution\":\"enabled\",\"implementation_status\":\"implemented\",\"callable\":true,\"implemented\":true,\"assisted_execution\":\"unavailable\",\"assisted_backend_available\":false}],\"assisted_backend_capabilities\":[],\"request_context_contract\":{\"accepted_features\":[\"consumer\",\"execution_mode_preference\"],\"effective_features\":[\"execution_mode_preference\"],\"unsupported_field_behavior\":\"reject\"}}}",
        );

        let request = request_json(
            "cli-runtime-hyf-capabilities",
            None,
            "sys.capabilities",
            None,
            &HyfEmptyInput::default(),
        );
        let response = HyfClient::new(executable)
            .capabilities()
            .expect("capabilities");

        assert_eq!(request["capability"], "sys.capabilities");
        assert_eq!(request["input"], serde_json::json!({}));
        assert!(request.get("context").is_none());
        assert_eq!(
            response.output.control_routes,
            vec!["sys.status", "sys.capabilities"]
        );
        assert_eq!(response.output.business_capabilities[0].id, "query_rewrite");
    }

    #[test]
    fn query_rewrite_request_round_trips_typed_output() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let dir = tempdir().expect("tempdir");
        let executable = write_response_script(
            dir.path(),
            "{\"version\":1,\"request_id\":\"rewrite-test-1\",\"trace_id\":\"trace-rewrite-test-1\",\"ok\":true,\"output\":{\"original_text\":\"apples near me with weekend pickup\",\"normalized_text\":\"apples near me with weekend pickup\",\"rewritten_text\":\"apples\",\"query_terms\":[\"apples\"],\"normalization_signals\":[\"local_intent_detected\"],\"ranking_hints\":[\"prefer_local_results\"],\"extracted_filters\":{\"local_intent\":true,\"fulfillment\":\"pickup\",\"time_window\":\"weekend\"}},\"meta\":{\"execution_mode\":\"deterministic\",\"backend\":\"heuristic\"}}",
        );
        let context = HyfRequestContext::deterministic_cli().with_return_provenance(true);
        let request = request_json(
            "rewrite-test-1",
            Some("trace-rewrite-test-1"),
            "query_rewrite",
            Some(&context),
            &HyfQueryRewriteRequest::new("apples near me with weekend pickup"),
        );
        let client = HyfClient::new(executable);
        let response = client
            .query_rewrite(
                "rewrite-test-1",
                Some("trace-rewrite-test-1"),
                &context,
                &HyfQueryRewriteRequest::new("apples near me with weekend pickup"),
            )
            .expect("query rewrite");
        assert_eq!(request["capability"], "query_rewrite");
        assert_eq!(
            request["context"]["execution_mode_preference"],
            "deterministic"
        );
        assert_eq!(request["context"]["consumer"], "radroots-cli");
        assert_eq!(request["context"]["return_provenance"], true);
        assert_eq!(
            request["input"]["query"],
            "apples near me with weekend pickup"
        );
        assert_eq!(response.output.rewritten_text, "apples");
        assert_eq!(response.output.query_terms, vec!["apples"]);
        assert_eq!(
            response.meta,
            Some(serde_json::json!({"execution_mode":"deterministic","backend":"heuristic"}))
        );
    }

    #[test]
    fn semantic_rank_request_round_trips_typed_output() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let dir = tempdir().expect("tempdir");
        let executable = write_response_script(
            dir.path(),
            "{\"version\":1,\"request_id\":\"rank-test-1\",\"ok\":true,\"output\":{\"ranked_ids\":[\"listing_local_1\",\"listing_regional_1\"],\"reasons\":{\"listing_local_1\":[\"apples match\",\"pickup match\"],\"listing_regional_1\":[\"delivery mismatch\"]},\"scored_candidates\":[{\"id\":\"listing_local_1\",\"heuristic_score\":14,\"matched_terms\":[\"apples\"],\"reasons\":[\"apples match\",\"pickup match\"],\"delivery_alignment\":\"match\",\"distance_band\":\"closer\",\"freshness_band\":\"fresher\",\"scope_match\":true}],\"ranking_hints\":[\"prefer_local_results\"],\"extracted_filters\":{\"local_intent\":true,\"fulfillment\":\"pickup\",\"time_window\":\"weekend\"}},\"meta\":{\"execution_mode\":\"deterministic\",\"backend\":\"heuristic\"}}",
        );
        let context = HyfRequestContext::deterministic_cli()
            .with_listing_scope(vec!["listing_local_1".to_owned()]);
        let request = request_json(
            "rank-test-1",
            None,
            "semantic_rank",
            Some(&context),
            &HyfSemanticRankRequest::new(
                "apples near me with weekend pickup",
                vec![sample_candidate("listing_local_1")],
            ),
        );
        let client = HyfClient::new(executable);
        let response = client
            .semantic_rank(
                "rank-test-1",
                None,
                &context,
                &HyfSemanticRankRequest::new(
                    "apples near me with weekend pickup",
                    vec![sample_candidate("listing_local_1")],
                ),
            )
            .expect("semantic rank");

        assert_eq!(request["capability"], "semantic_rank");
        assert_eq!(
            request["context"]["scope"]["listing_ids"],
            serde_json::json!(["listing_local_1"])
        );
        assert_eq!(request["input"]["candidates"][0]["id"], "listing_local_1");
        assert_eq!(response.output.ranked_ids[0], "listing_local_1");
        assert_eq!(response.output.scored_candidates[0].heuristic_score, 14);
    }

    #[test]
    fn explain_result_request_round_trips_typed_output() {
        let _guard = hyf_test_lock().lock().expect("hyf test lock");
        let dir = tempdir().expect("tempdir");
        let executable = write_response_script(
            dir.path(),
            "{\"version\":1,\"request_id\":\"explain-test-1\",\"trace_id\":\"trace-explain-test-1\",\"ok\":true,\"output\":{\"result_id\":\"listing_local_1\",\"explanation_kind\":\"deterministic\",\"summary\":\"Result listing_local_1 was ranked using deterministic heuristic signals: apples match and pickup match.\",\"score\":14,\"reasons\":[\"apples match\",\"pickup match\"],\"matched_terms\":[\"apples\"],\"ranking_hints\":[\"prefer_local_results\"],\"extracted_filters\":{\"local_intent\":true,\"fulfillment\":\"pickup\",\"time_window\":\"weekend\"},\"signal_assessment\":{\"delivery_alignment\":\"match\",\"distance_band\":\"closer\",\"freshness_band\":\"fresher\",\"scope_match\":true}},\"meta\":{\"execution_mode\":\"deterministic\",\"backend\":\"heuristic\"}}",
        );
        let context = HyfRequestContext::deterministic_cli().with_return_provenance(true);
        let request = request_json(
            "explain-test-1",
            Some("trace-explain-test-1"),
            "explain_result",
            Some(&context),
            &HyfExplainResultRequest::new(
                "apples near me with weekend pickup",
                sample_candidate("listing_local_1"),
            ),
        );
        let client = HyfClient::new(executable);
        let response = client
            .explain_result(
                "explain-test-1",
                Some("trace-explain-test-1"),
                &context,
                &HyfExplainResultRequest::new(
                    "apples near me with weekend pickup",
                    sample_candidate("listing_local_1"),
                ),
            )
            .expect("explain result");

        assert_eq!(request["capability"], "explain_result");
        assert_eq!(request["context"]["return_provenance"], true);
        assert_eq!(request["input"]["candidate"]["id"], "listing_local_1");
        assert_eq!(response.output.result_id, "listing_local_1");
        assert_eq!(
            response.output.signal_assessment.delivery_alignment,
            "match"
        );
    }

    fn sample_candidate(id: &str) -> HyfSemanticCandidate {
        HyfSemanticCandidate {
            id: id.to_owned(),
            title: "Organic apples".to_owned(),
            farm: "Local Orchard".to_owned(),
            delivery: "pickup".to_owned(),
            distance_km: 4.1,
            freshness_minutes: 3,
        }
    }

    fn request_json<T: Serialize>(
        request_id: &str,
        trace_id: Option<&str>,
        capability: &str,
        context: Option<&HyfRequestContext>,
        input: &T,
    ) -> Value {
        let raw = super::serialize_request(request_id, trace_id, capability, context, input)
            .expect("serialize request");
        serde_json::from_str(raw.as_str()).expect("request json")
    }

    fn write_response_script(dir: &Path, response: &str) -> PathBuf {
        write_script(
            dir,
            format!("#!/bin/sh\nread -r _request || exit 64\ncat <<'JSON'\n{response}\nJSON\n")
                .as_str(),
        )
    }
    fn write_script(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("fake-hyfd");
        fs::write(&path, script).expect("write fake hyfd");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod fake hyfd");
        path
    }
}
