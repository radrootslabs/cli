use serde::Serialize;
use serde_json::Value;

use crate::runtime::config::RuntimeConfig;
use crate::view::runtime::{CommandDisposition, TransportTargetFailureView};

#[derive(Debug, Clone)]
pub struct ValidationReceiptEventArgs {
    pub receipt_event_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptInspectionView {
    pub state: String,
    pub resource: Option<ValidationReceiptResourceView>,
    pub receipt_event_id: Option<String>,
    pub validation_state: String,
    pub proof_verification: Option<Value>,
    pub receipt: Option<Value>,
    pub receipt_tags: Option<Value>,
    pub event: Option<Value>,
    pub target_transport_endpoints: Vec<String>,
    pub attempted_transport_endpoints: Vec<String>,
    pub failed_transport_targets: Vec<TransportTargetFailureView>,
    pub reason_code: Option<String>,
    pub reason: Option<String>,
    pub sdk_error: Option<Value>,
    pub actions: Vec<String>,
}

impl ValidationReceiptInspectionView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "valid" | "verified" => CommandDisposition::Success,
            "missing" => CommandDisposition::NotFound,
            "invalid" => CommandDisposition::ValidationFailed,
            "unconfigured" => CommandDisposition::Unconfigured,
            "network_unavailable" => CommandDisposition::ExternalUnavailable,
            "unsupported" => CommandDisposition::Unsupported,
            _ => CommandDisposition::InternalError,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptResourceView {
    pub kind: String,
    pub id: String,
}

pub fn get(
    _config: &RuntimeConfig,
    args: &ValidationReceiptEventArgs,
) -> ValidationReceiptInspectionView {
    unsupported_validation_receipt_view(args, "inspect")
}

pub fn verify(
    _config: &RuntimeConfig,
    args: &ValidationReceiptEventArgs,
) -> ValidationReceiptInspectionView {
    unsupported_validation_receipt_view(args, "verify")
}

fn unsupported_validation_receipt_view(
    args: &ValidationReceiptEventArgs,
    intent: &str,
) -> ValidationReceiptInspectionView {
    let receipt_event_id = args.receipt_event_id.trim().to_owned();
    ValidationReceiptInspectionView {
        state: "unsupported".to_owned(),
        resource: Some(ValidationReceiptResourceView {
            kind: "validation_receipt".to_owned(),
            id: receipt_event_id.clone(),
        }),
        receipt_event_id: Some(receipt_event_id),
        validation_state: "not_applicable".to_owned(),
        proof_verification: None,
        receipt: None,
        receipt_tags: None,
        event: None,
        target_transport_endpoints: Vec::new(),
        attempted_transport_endpoints: Vec::new(),
        failed_transport_targets: Vec::new(),
        reason_code: Some("validation_receipts_not_agreement_authority".to_owned()),
        reason: Some(format!(
            "validation receipt {intent} is not part of the release-product V1 trade agreement authority"
        )),
        sdk_error: None,
        actions: vec![
            "radroots trade get <trade-id>".to_owned(),
            "radroots trade evidence inspect <trade-id>".to_owned(),
        ],
    }
}
