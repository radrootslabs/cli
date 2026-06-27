#![allow(dead_code)]

use serde::Serialize;
use serde_json::{Value, json};

pub const OUTPUT_SCHEMA_VERSION: &str = "radroots.cli.output.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeContext {
    pub request_id: String,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub output_format: OutputFormat,
    pub dry_run: bool,
    pub actor: Option<EnvelopeActor>,
}

impl EnvelopeContext {
    pub fn new(request_id: impl Into<String>, dry_run: bool) -> Self {
        Self {
            request_id: request_id.into(),
            correlation_id: None,
            idempotency_key: None,
            output_format: OutputFormat::Terminal,
            dry_run,
            actor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvelopeActor {
    pub account_id: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputEnvelope {
    pub schema_version: &'static str,
    pub operation_id: String,
    pub kind: String,
    pub status: OutputStatus,
    pub output_format: OutputFormat,
    pub request_id: String,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub dry_run: bool,
    pub actor: Option<EnvelopeActor>,
    pub resource: Option<OutputResource>,
    pub result: Value,
    pub reason_code: Option<String>,
    pub warnings: Vec<OutputWarning>,
    pub errors: Vec<OutputError>,
    pub next_actions: Vec<NextAction>,
}

impl OutputEnvelope {
    pub fn success(
        operation_id: impl Into<String>,
        result: Value,
        context: EnvelopeContext,
    ) -> Self {
        let operation_id = operation_id.into();
        let resource = output_resource_from_value(&result);
        let reason_code = output_reason_code_from_value(&result);
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            kind: operation_id.clone(),
            operation_id,
            status: OutputStatus::Ok,
            output_format: context.output_format,
            request_id: context.request_id,
            correlation_id: context.correlation_id,
            idempotency_key: context.idempotency_key,
            dry_run: context.dry_run,
            actor: context.actor,
            resource,
            result,
            reason_code,
            warnings: Vec::new(),
            errors: Vec::new(),
            next_actions: Vec::new(),
        }
    }

    pub fn failure(
        operation_id: impl Into<String>,
        error: OutputError,
        context: EnvelopeContext,
    ) -> Self {
        let operation_id = operation_id.into();
        let next_actions = next_actions_from_error_detail(&error);
        let resource = error.detail.as_ref().and_then(output_resource_from_value);
        let reason_code = Some(error.reason_code.clone());
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            kind: operation_id.clone(),
            operation_id,
            status: OutputStatus::Error,
            output_format: context.output_format,
            request_id: context.request_id,
            correlation_id: context.correlation_id,
            idempotency_key: context.idempotency_key,
            dry_run: context.dry_run,
            actor: context.actor,
            resource,
            result: Value::Null,
            reason_code,
            warnings: Vec::new(),
            errors: vec![error],
            next_actions,
        }
    }

    pub fn to_ndjson_frames(&self) -> Vec<NdjsonFrame> {
        let started = NdjsonFrame::new(
            self.operation_id.clone(),
            self.request_id.clone(),
            0,
            NdjsonFrameType::Started,
            json!({
                "state": "started",
                "status": self.status,
                "output_format": self.output_format,
                "dry_run": self.dry_run,
                "correlation_id": &self.correlation_id,
                "idempotency_key": &self.idempotency_key,
                "actor": &self.actor,
                "resource": &self.resource,
            }),
        );
        let mut terminal = NdjsonFrame::new(
            self.operation_id.clone(),
            self.request_id.clone(),
            1,
            if self.errors.is_empty() {
                NdjsonFrameType::Completed
            } else {
                NdjsonFrameType::Error
            },
            json!({
                "status": self.status,
                "reason_code": &self.reason_code,
                "output_format": self.output_format,
                "resource": &self.resource,
                "result": &self.result,
                "next_actions": &self.next_actions,
                "dry_run": self.dry_run,
                "correlation_id": &self.correlation_id,
                "idempotency_key": &self.idempotency_key,
                "actor": &self.actor,
            }),
        );
        terminal.warnings = self.warnings.clone();
        terminal.errors = self.errors.clone();
        vec![started, terminal]
    }
}

fn output_reason_code_from_value(value: &Value) -> Option<String> {
    value
        .get("reason_code")
        .and_then(Value::as_str)
        .filter(|reason_code| !reason_code.trim().is_empty())
        .map(str::to_owned)
}

fn output_resource_from_value(value: &Value) -> Option<OutputResource> {
    let object = value.as_object()?;
    if let Some(resource) = object.get("resource").and_then(declared_output_resource) {
        return Some(resource);
    }
    output_resource_from_fields(object).or_else(|| {
        let nested_fields = [
            "account",
            "resolved_account",
            "default_account",
            "bound_account",
            "farm",
            "listing",
            "basket",
            "quote",
            "trade",
        ];
        nested_fields
            .into_iter()
            .filter_map(|field| {
                object
                    .get(field)
                    .and_then(|value| nested_output_resource(field, value))
            })
            .next()
    })
}

fn nested_output_resource(field: &str, value: &Value) -> Option<OutputResource> {
    let mut resource = output_resource_from_value(value)?;
    if resource.kind == "resource" {
        resource.kind = match field {
            "resolved_account" | "default_account" | "bound_account" => "account",
            "order" => "trade",
            other => other,
        }
        .to_owned();
    }
    Some(resource)
}

fn declared_output_resource(value: &Value) -> Option<OutputResource> {
    let object = value.as_object()?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| !kind.trim().is_empty())?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())?;
    Some(OutputResource {
        kind: kind.to_owned(),
        id: id.to_owned(),
    })
}

fn output_resource_from_fields(object: &serde_json::Map<String, Value>) -> Option<OutputResource> {
    [
        ("account_id", "account"),
        ("id", "resource"),
        ("farm_id", "farm"),
        ("seller_account_id", "account"),
        ("buyer_account_id", "account"),
        ("listing_id", "listing"),
        ("listing_address", "listing"),
        ("listing_addr", "listing"),
        ("basket_id", "basket"),
        ("order_id", "trade"),
    ]
    .into_iter()
    .find_map(|(field, kind)| {
        object
            .get(field)
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(|id| OutputResource {
                kind: kind.to_owned(),
                id: id.to_owned(),
            })
    })
}

pub fn next_actions_from_result_value(result: &Value) -> Vec<NextAction> {
    next_actions_from_actions_value(result.get("actions"))
}

fn next_actions_from_error_detail(error: &OutputError) -> Vec<NextAction> {
    next_actions_from_actions_value(
        error
            .detail
            .as_ref()
            .and_then(|detail| detail.get("actions")),
    )
}

fn next_actions_from_actions_value(actions_value: Option<&Value>) -> Vec<NextAction> {
    actions_value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(next_action_from_action_string)
        .fold(Vec::<NextAction>::new(), |mut actions, action| {
            if !actions.contains(&action) {
                actions.push(action);
            }
            actions
        })
}

fn next_action_from_action_string(action: &str) -> Option<NextAction> {
    let action = action.trim();
    if action
        == "configure RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE or RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_SECRET_ID"
    {
        return Some(NextAction {
            kind: NextActionKind::OperatorConfig,
            label: "configure radrootsd proxy token source".to_owned(),
            command: None,
            description: Some(action.to_owned()),
            env_var: Some("RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE".to_owned()),
            config_key: None,
        });
    }
    if action == "configure signer.remote_nip46 signer_session_ref" {
        return Some(NextAction {
            kind: NextActionKind::OperatorConfig,
            label: "configure signer session binding".to_owned(),
            command: None,
            description: Some(action.to_owned()),
            env_var: None,
            config_key: Some("signer.remote_nip46.signer_session_ref".to_owned()),
        });
    }
    let command = action.trim().strip_prefix("run ").unwrap_or(action).trim();
    if !command.starts_with("radroots ") {
        return None;
    }
    Some(NextAction {
        kind: NextActionKind::CliCommand,
        label: next_action_label(command),
        command: Some(command.to_owned()),
        description: None,
        env_var: None,
        config_key: None,
    })
}

fn next_action_label(command: &str) -> String {
    let parts = command.split_whitespace().collect::<Vec<_>>();
    let mut index = usize::from(parts.first().is_some_and(|part| *part == "radroots"));
    let mut labels = Vec::new();
    while index < parts.len() {
        let part = parts[index];
        if part.starts_with("--") {
            index += 1;
            if matches!(
                part,
                "--format"
                    | "--account-id"
                    | "--relay"
                    | "--publish-transport"
                    | "--idempotency-key"
                    | "--correlation-id"
                    | "--approval-token"
            ) && index < parts.len()
            {
                index += 1;
            }
            continue;
        }
        labels.push(part);
        index += 1;
    }
    if labels.is_empty() {
        "radroots".to_owned()
    } else {
        labels.join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Terminal,
    Json,
    Ndjson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputResource {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputError {
    pub code: String,
    pub reason_code: String,
    pub message: String,
    pub exit_code: u8,
    pub detail: Option<Value>,
}

impl OutputError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        exit_code: CliExitCode,
    ) -> Self {
        let code = code.into();
        Self {
            reason_code: code.clone(),
            code,
            message: message.into(),
            exit_code: exit_code.code(),
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliExitCode {
    Success,
    InternalError,
    InvalidInput,
    RuntimeUnavailable,
    NotFound,
    AuthorizationFailed,
    ApprovalRequiredOrDenied,
    SignerUnavailable,
    SyncOrNetworkFailure,
    Conflict,
    ValidationFailed,
    UnsafeOperationRefused,
}

impl CliExitCode {
    pub fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::InternalError => 1,
            Self::InvalidInput => 2,
            Self::RuntimeUnavailable => 3,
            Self::NotFound => 4,
            Self::AuthorizationFailed => 5,
            Self::ApprovalRequiredOrDenied => 6,
            Self::SignerUnavailable => 7,
            Self::SyncOrNetworkFailure => 8,
            Self::Conflict => 9,
            Self::ValidationFailed => 10,
            Self::UnsafeOperationRefused => 11,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextAction {
    pub kind: NextActionKind,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActionKind {
    CliCommand,
    OperatorConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NdjsonFrameType {
    Started,
    Event,
    Progress,
    Warning,
    Error,
    Completed,
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NdjsonFrame {
    pub schema_version: &'static str,
    pub operation_id: String,
    pub kind: String,
    pub request_id: String,
    pub sequence: u64,
    pub frame_type: NdjsonFrameType,
    pub payload: Value,
    pub warnings: Vec<OutputWarning>,
    pub errors: Vec<OutputError>,
}

impl NdjsonFrame {
    pub fn new(
        operation_id: impl Into<String>,
        request_id: impl Into<String>,
        sequence: u64,
        frame_type: NdjsonFrameType,
        payload: Value,
    ) -> Self {
        let operation_id = operation_id.into();
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            kind: operation_id.clone(),
            operation_id,
            request_id: request_id.into(),
            sequence,
            frame_type,
            payload,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        CliExitCode, EnvelopeContext, NdjsonFrame, NdjsonFrameType, NextActionKind,
        OUTPUT_SCHEMA_VERSION, OutputEnvelope, OutputError,
    };

    #[test]
    fn success_envelope_serializes_required_fields() {
        let mut context = EnvelopeContext::new("req_test", true);
        context.correlation_id = Some("corr_test".to_owned());
        context.idempotency_key = Some("idem_test".to_owned());
        let envelope = OutputEnvelope::success(
            "listing.publish",
            json!({ "listing_id": "listing_test" }),
            context,
        );
        let value = serde_json::to_value(envelope).expect("serialize envelope");

        assert_eq!(value["schema_version"], OUTPUT_SCHEMA_VERSION);
        assert_eq!(value["operation_id"], "listing.publish");
        assert_eq!(value["kind"], "listing.publish");
        assert_eq!(value["status"], "ok");
        assert_eq!(value["output_format"], "terminal");
        assert_eq!(value["request_id"], "req_test");
        assert_eq!(value["correlation_id"], "corr_test");
        assert_eq!(value["idempotency_key"], "idem_test");
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["resource"]["kind"], "listing");
        assert_eq!(value["resource"]["id"], "listing_test");
        assert_eq!(value["result"]["listing_id"], "listing_test");
        assert_eq!(value["reason_code"], Value::Null);
        assert_eq!(value["warnings"].as_array().unwrap().len(), 0);
        assert_eq!(value["errors"].as_array().unwrap().len(), 0);
        assert_eq!(value["next_actions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn failure_envelope_carries_structured_error_and_exit_code() {
        let error = OutputError::new(
            "approval_required",
            "operation requires approval token",
            CliExitCode::ApprovalRequiredOrDenied,
        );
        let envelope = OutputEnvelope::failure(
            "trade.submit",
            error,
            EnvelopeContext::new("req_order", false),
        );
        let value = serde_json::to_value(envelope).expect("serialize envelope");

        assert_eq!(value["schema_version"], OUTPUT_SCHEMA_VERSION);
        assert_eq!(value["operation_id"], "trade.submit");
        assert_eq!(value["status"], "error");
        assert_eq!(value["reason_code"], "approval_required");
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "approval_required");
        assert_eq!(value["errors"][0]["reason_code"], "approval_required");
        assert_eq!(value["errors"][0]["exit_code"], 6);
    }

    #[test]
    fn failure_envelope_derives_next_actions_from_error_detail() {
        let mut error = OutputError::new(
            "not_found",
            "order draft was not found",
            CliExitCode::NotFound,
        );
        error.detail = Some(json!({
            "actions": [
                "radroots trade list",
                "run radroots basket create"
            ]
        }));
        let envelope = OutputEnvelope::failure(
            "trade.submit",
            error,
            EnvelopeContext::new("req_order", true),
        );

        assert_eq!(envelope.next_actions.len(), 2);
        assert_eq!(envelope.next_actions[0].kind, NextActionKind::CliCommand);
        assert_eq!(envelope.next_actions[0].label, "trade list");
        assert_eq!(
            envelope.next_actions[0].command.as_deref(),
            Some("radroots trade list")
        );
        assert_eq!(envelope.next_actions[1].kind, NextActionKind::CliCommand);
        assert_eq!(envelope.next_actions[1].label, "basket create");
        assert_eq!(
            envelope.next_actions[1].command.as_deref(),
            Some("radroots basket create")
        );
    }

    #[test]
    fn failure_envelope_derives_operator_config_next_actions() {
        let mut error = OutputError::new(
            "operation_unavailable",
            "publish transport needs operator configuration",
            CliExitCode::RuntimeUnavailable,
        );
        error.detail = Some(json!({
            "actions": [
                "configure RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE or RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_SECRET_ID",
                "configure signer.remote_nip46 signer_session_ref",
                "configure RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE or RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_SECRET_ID"
            ]
        }));
        let envelope = OutputEnvelope::failure(
            "config.get",
            error,
            EnvelopeContext::new("req_config", false),
        );
        let value = serde_json::to_value(&envelope).expect("serialize envelope");

        assert_eq!(envelope.next_actions.len(), 2);
        assert_eq!(
            envelope.next_actions[0].kind,
            NextActionKind::OperatorConfig
        );
        assert_eq!(
            envelope.next_actions[0].label,
            "configure radrootsd proxy token source"
        );
        assert_eq!(envelope.next_actions[0].command, None);
        assert_eq!(
            envelope.next_actions[0].env_var.as_deref(),
            Some("RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE")
        );
        assert_eq!(
            envelope.next_actions[1].kind,
            NextActionKind::OperatorConfig
        );
        assert_eq!(
            envelope.next_actions[1].label,
            "configure signer session binding"
        );
        assert_eq!(envelope.next_actions[1].command, None);
        assert_eq!(
            envelope.next_actions[1].config_key.as_deref(),
            Some("signer.remote_nip46.signer_session_ref")
        );
        assert_eq!(value["next_actions"][0]["kind"], "operator_config");
        assert_eq!(value["next_actions"][0]["command"], Value::Null);
        assert_eq!(value["next_actions"][1]["kind"], "operator_config");
        assert_eq!(value["next_actions"][1]["command"], Value::Null);
    }

    #[test]
    fn ndjson_frames_serialize_one_json_object_per_line() {
        let frames = [
            NdjsonFrame::new(
                "sync.watch",
                "req_watch",
                0,
                NdjsonFrameType::Started,
                json!({ "state": "started" }),
            ),
            NdjsonFrame::new(
                "sync.watch",
                "req_watch",
                1,
                NdjsonFrameType::Event,
                json!({ "state": "submitted" }),
            ),
            NdjsonFrame::new(
                "sync.watch",
                "req_watch",
                2,
                NdjsonFrameType::Completed,
                json!({ "state": "complete" }),
            ),
        ];
        let rendered = frames
            .iter()
            .map(|frame| serde_json::to_string(frame).expect("serialize frame"))
            .collect::<Vec<_>>()
            .join("\n");

        for line in rendered.lines() {
            let value: Value = serde_json::from_str(line).expect("line is json");
            assert_eq!(value["schema_version"], OUTPUT_SCHEMA_VERSION);
            assert_eq!(value["operation_id"], "sync.watch");
            assert!(value["frame_type"].is_string());
        }
    }

    #[test]
    fn ndjson_terminal_frame_carries_status_reason_and_resource() {
        let mut error = OutputError::new(
            "not_implemented",
            "operation is not implemented",
            CliExitCode::RuntimeUnavailable,
        );
        error.detail = Some(json!({
            "order_id": "ord_test",
        }));
        let envelope = OutputEnvelope::failure(
            "test.operation",
            error,
            EnvelopeContext::new("req_test", false),
        );
        let frames = envelope.to_ndjson_frames();

        assert_eq!(frames[0].payload["status"], "error");
        assert_eq!(frames[0].payload["output_format"], "terminal");
        assert_eq!(frames[1].payload["status"], "error");
        assert_eq!(frames[1].payload["reason_code"], "not_implemented");
        assert_eq!(frames[1].payload["resource"]["kind"], "trade");
        assert_eq!(frames[1].payload["resource"]["id"], "ord_test");
        assert_eq!(frames[1].errors[0].reason_code, "not_implemented");
    }

    #[test]
    fn exit_code_contract_matches_handoff_range() {
        assert_eq!(CliExitCode::Success.code(), 0);
        assert_eq!(CliExitCode::InvalidInput.code(), 2);
        assert_eq!(CliExitCode::ApprovalRequiredOrDenied.code(), 6);
        assert_eq!(CliExitCode::UnsafeOperationRefused.code(), 11);
    }
}
