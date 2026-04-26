#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;

pub const OUTPUT_SCHEMA_VERSION: &str = "radroots.cli.output.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeContext {
    pub request_id: String,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub dry_run: bool,
    pub actor: Option<EnvelopeActor>,
}

impl EnvelopeContext {
    pub fn new(request_id: impl Into<String>, dry_run: bool) -> Self {
        Self {
            request_id: request_id.into(),
            correlation_id: None,
            idempotency_key: None,
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
    pub request_id: String,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub dry_run: bool,
    pub actor: Option<EnvelopeActor>,
    pub result: Value,
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
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            kind: operation_id.clone(),
            operation_id,
            request_id: context.request_id,
            correlation_id: context.correlation_id,
            idempotency_key: context.idempotency_key,
            dry_run: context.dry_run,
            actor: context.actor,
            result,
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
        Self {
            schema_version: OUTPUT_SCHEMA_VERSION,
            kind: operation_id.clone(),
            operation_id,
            request_id: context.request_id,
            correlation_id: context.correlation_id,
            idempotency_key: context.idempotency_key,
            dry_run: context.dry_run,
            actor: context.actor,
            result: Value::Null,
            warnings: Vec::new(),
            errors: vec![error],
            next_actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputError {
    pub code: String,
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
        Self {
            code: code.into(),
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
    UnavailableOrUnconfigured,
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
            Self::UnavailableOrUnconfigured => 3,
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
    pub label: String,
    pub command: String,
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
        CliExitCode, EnvelopeContext, NdjsonFrame, NdjsonFrameType, OUTPUT_SCHEMA_VERSION,
        OutputEnvelope, OutputError,
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
        assert_eq!(value["request_id"], "req_test");
        assert_eq!(value["correlation_id"], "corr_test");
        assert_eq!(value["idempotency_key"], "idem_test");
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["result"]["listing_id"], "listing_test");
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
            "order.submit",
            error,
            EnvelopeContext::new("req_order", false),
        );
        let value = serde_json::to_value(envelope).expect("serialize envelope");

        assert_eq!(value["schema_version"], OUTPUT_SCHEMA_VERSION);
        assert_eq!(value["operation_id"], "order.submit");
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["errors"][0]["code"], "approval_required");
        assert_eq!(value["errors"][0]["exit_code"], 6);
    }

    #[test]
    fn ndjson_frames_serialize_one_json_object_per_line() {
        let frames = [
            NdjsonFrame::new(
                "order.event.watch",
                "req_watch",
                0,
                NdjsonFrameType::Started,
                json!({ "state": "started" }),
            ),
            NdjsonFrame::new(
                "order.event.watch",
                "req_watch",
                1,
                NdjsonFrameType::Event,
                json!({ "state": "submitted" }),
            ),
            NdjsonFrame::new(
                "order.event.watch",
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
            assert_eq!(value["operation_id"], "order.event.watch");
            assert!(value["frame_type"].is_string());
        }
    }

    #[test]
    fn exit_code_contract_matches_handoff_range() {
        assert_eq!(CliExitCode::Success.code(), 0);
        assert_eq!(CliExitCode::InvalidInput.code(), 2);
        assert_eq!(CliExitCode::ApprovalRequiredOrDenied.code(), 6);
        assert_eq!(CliExitCode::UnsafeOperationRefused.code(), 11);
    }
}
