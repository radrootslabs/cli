use radroots_sdk::{
    RadrootsSdkError, TradeValidationReceiptEvent, TradeValidationReceiptInspectReceipt,
    TradeValidationReceiptInspectRequest, TradeValidationReceiptInvalidCandidate,
    TradeValidationReceiptListReceipt, TradeValidationReceiptListRequest,
    TradeValidationReceiptRelayOutcomeKind, TradeValidationReceiptRelayOutcomeReceipt,
    TradeValidationReceiptTags, TradeValidationReceiptVerifyRequest,
    TradeValidationReceiptWorkerEvidence,
    TradeValidationReceiptWorkerEvidenceSelection as SdkWorkerEvidenceSelection,
};
use radroots_sp1_host_trade::RadrootsSp1TradeHostError;
use radroots_sp1_host_trade::verify_order_acceptance_validation_receipt_inline_sp1_proof;
use radroots_trade::validation_receipt::{
    RadrootsTradeCommitmentConfidence, RadrootsTradeValidationAuthority,
    RadrootsTradeValidationReceipt, RadrootsValidationReceiptProofSystem,
    RadrootsValidationReceiptResult, RadrootsValidationReceiptType,
};
use serde::Serialize;
use serde_json::Value;

use crate::runtime::config::RuntimeConfig;
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession};
use crate::view::runtime::{CommandDisposition, RelayFailureView};

#[derive(Debug, Clone)]
pub struct ValidationReceiptEventArgs {
    pub receipt_event_id: String,
}

#[derive(Debug, Clone)]
pub struct ValidationReceiptListArgs {
    pub order_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptInspectionView {
    pub state: String,
    pub resource: Option<ValidationReceiptResourceView>,
    pub receipt_event_id: Option<String>,
    pub order_id: Option<String>,
    pub validation_state: String,
    pub proof_verification: Option<ValidationReceiptProofVerificationView>,
    pub receipt: Option<RadrootsTradeValidationReceipt>,
    pub receipt_tags: Option<ValidationReceiptTagsView>,
    pub event: Option<ValidationReceiptEventView>,
    pub target_relays: Vec<String>,
    pub connected_relays: Vec<String>,
    pub failed_relays: Vec<RelayFailureView>,
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
            _ => CommandDisposition::InternalError,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptListView {
    pub state: String,
    pub order_id: String,
    pub count: usize,
    pub valid_count: usize,
    pub invalid_count: usize,
    pub receipts: Vec<ValidationReceiptSummaryView>,
    pub invalid_receipts: Vec<ValidationReceiptInvalidCandidateView>,
    pub target_relays: Vec<String>,
    pub connected_relays: Vec<String>,
    pub failed_relays: Vec<RelayFailureView>,
    pub reason_code: Option<String>,
    pub reason: Option<String>,
    pub sdk_error: Option<Value>,
    pub actions: Vec<String>,
}

impl ValidationReceiptListView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "listed" | "empty" | "partial" => CommandDisposition::Success,
            "invalid" => CommandDisposition::ValidationFailed,
            "unconfigured" => CommandDisposition::Unconfigured,
            "network_unavailable" => CommandDisposition::ExternalUnavailable,
            _ => CommandDisposition::InternalError,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptResourceView {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptEventView {
    pub id: String,
    pub author: String,
    pub created_at: u32,
    pub kind: u32,
    pub sig: String,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptTagsView {
    pub order_id: String,
    pub event_set_root: String,
    pub listing_event_id: String,
    pub reducer_output_root: String,
    pub public_values_hash: String,
    pub proof_system: String,
    pub receipt_type: String,
    pub root_event_id: String,
    pub target_event_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptProofVerificationView {
    pub state: String,
    pub verifier: String,
    pub proof_system: String,
    pub validation_authority: Option<String>,
    pub commitment_confidence: Option<String>,
    pub production_verification: bool,
    pub public_values_hash_binding: String,
    pub proof_metadata_binding: String,
    pub cryptographic_proof_required: bool,
    pub cryptographic_proof_verified: bool,
    pub mode: Option<String>,
    pub program_hash: Option<String>,
    pub verifying_key_hash: Option<String>,
    pub proof_reference: Option<String>,
    pub inline_proof_present: bool,
    pub worker_evidence: Option<ValidationReceiptWorkerEvidenceView>,
    pub untrusted_worker_evidence: Option<ValidationReceiptWorkerEvidenceView>,
    pub reason_code: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptWorkerEvidenceView {
    pub result_event_id: String,
    pub author: String,
    pub validation_authority: Option<String>,
    pub commitment_confidence: Option<String>,
    pub status: String,
    pub prover_backend: String,
    pub proof_mode: String,
    pub proof_system: String,
    pub proof_generated: bool,
    pub sp1_execute_checked: bool,
    pub sp1_execute_public_values_hash: Option<String>,
    pub cryptographic_proof_verified: bool,
    pub public_values_hash: String,
}

#[derive(Clone, Debug, Default)]
struct ValidationReceiptWorkerEvidenceSelection {
    trusted: Option<ValidationReceiptWorkerEvidenceView>,
    untrusted: Option<ValidationReceiptWorkerEvidenceView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptSummaryView {
    pub resource: ValidationReceiptResourceView,
    pub receipt_event_id: String,
    pub order_id: String,
    pub author: String,
    pub created_at: u32,
    pub receipt_type: String,
    pub result: String,
    pub proof_system: String,
    pub proof_verification_state: String,
    pub validation_authority: Option<String>,
    pub commitment_confidence: Option<String>,
    pub production_verification: bool,
    pub event_set_root: String,
    pub reducer_output_root: String,
    pub public_values_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReceiptInvalidCandidateView {
    pub receipt_event_id: String,
    pub kind: u32,
    pub reason_code: String,
    pub reason: String,
    pub proof_verification: Option<ValidationReceiptProofVerificationView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidationReceiptCommandIntent {
    Inspect,
    Verify,
}

pub fn get(
    config: &RuntimeConfig,
    args: &ValidationReceiptEventArgs,
) -> ValidationReceiptInspectionView {
    inspect_event(
        config,
        &args.receipt_event_id,
        "valid",
        ValidationReceiptCommandIntent::Inspect,
    )
}

pub fn verify(
    config: &RuntimeConfig,
    args: &ValidationReceiptEventArgs,
) -> ValidationReceiptInspectionView {
    inspect_event(
        config,
        &args.receipt_event_id,
        "verified",
        ValidationReceiptCommandIntent::Verify,
    )
}

pub fn list(config: &RuntimeConfig, args: &ValidationReceiptListArgs) -> ValidationReceiptListView {
    let order_id = args.order_id.trim();
    if order_id.is_empty() {
        return invalid_list_view(
            args.order_id.clone(),
            "invalid_order_id",
            "validation receipt list requires non-empty `order_id`",
        );
    }
    let request = match TradeValidationReceiptListRequest::parse(order_id).and_then(|request| {
        request.try_with_trusted_worker_pubkeys(
            config.rhi.trusted_worker_pubkeys.iter().map(String::as_str),
        )
    }) {
        Ok(request) => request,
        Err(error) => {
            return list_sdk_error_view(order_id, CliSdkAdapterError::Sdk(error));
        }
    };
    let session = match CliSdkSession::connect(config) {
        Ok(session) => session,
        Err(error) => return list_sdk_error_view(order_id, error),
    };
    match session.block_on(session.sdk().trades().validation_receipts().list(request)) {
        Ok(receipt) => list_from_sdk_receipt(receipt),
        Err(error) => list_sdk_error_view(order_id, CliSdkAdapterError::Sdk(error)),
    }
}

fn inspect_event(
    config: &RuntimeConfig,
    receipt_event_id: &str,
    success_state: &str,
    intent: ValidationReceiptCommandIntent,
) -> ValidationReceiptInspectionView {
    let receipt_event_id = receipt_event_id.trim();
    if receipt_event_id.is_empty() {
        return invalid_inspection_view(
            None,
            "invalid_receipt_event_id",
            "validation receipt command requires non-empty `receipt_event_id`",
        );
    }
    let trusted_worker_pubkeys = config.rhi.trusted_worker_pubkeys.iter().map(String::as_str);
    let session = match CliSdkSession::connect(config) {
        Ok(session) => session,
        Err(error) => return inspection_sdk_error_view(receipt_event_id, error),
    };
    match intent {
        ValidationReceiptCommandIntent::Inspect => {
            let request = match TradeValidationReceiptInspectRequest::parse(receipt_event_id)
                .and_then(|request| request.try_with_trusted_worker_pubkeys(trusted_worker_pubkeys))
            {
                Ok(request) => request,
                Err(error) => {
                    return inspection_sdk_error_view(
                        receipt_event_id,
                        CliSdkAdapterError::Sdk(error),
                    );
                }
            };
            match session.block_on(
                session
                    .sdk()
                    .trades()
                    .validation_receipts()
                    .inspect(request),
            ) {
                Ok(receipt) => {
                    inspection_from_sdk_receipt(receipt_event_id, success_state, intent, receipt)
                }
                Err(error) => {
                    inspection_sdk_error_view(receipt_event_id, CliSdkAdapterError::Sdk(error))
                }
            }
        }
        ValidationReceiptCommandIntent::Verify => {
            let request = match TradeValidationReceiptVerifyRequest::parse(receipt_event_id)
                .and_then(|request| request.try_with_trusted_worker_pubkeys(trusted_worker_pubkeys))
            {
                Ok(request) => request,
                Err(error) => {
                    return inspection_sdk_error_view(
                        receipt_event_id,
                        CliSdkAdapterError::Sdk(error),
                    );
                }
            };
            match session.block_on(session.sdk().trades().validation_receipts().verify(request)) {
                Ok(receipt) => {
                    inspection_from_sdk_receipt(receipt_event_id, success_state, intent, receipt)
                }
                Err(error) => {
                    inspection_sdk_error_view(receipt_event_id, CliSdkAdapterError::Sdk(error))
                }
            }
        }
    }
}

fn inspection_from_sdk_receipt(
    receipt_event_id: &str,
    success_state: &str,
    intent: ValidationReceiptCommandIntent,
    sdk_receipt: TradeValidationReceiptInspectReceipt,
) -> ValidationReceiptInspectionView {
    let target_relays = sdk_receipt.relay_targets;
    let connected_relays = connected_relays(&sdk_receipt.relay_evidence.relays);
    let failed_relays = sdk_relay_failures(&sdk_receipt.relay_evidence.relays);
    let reason_code = (!failed_relays.is_empty()).then_some("relay_fetch_partial".to_owned());
    if let Some(invalid) = sdk_receipt.invalid_receipt {
        return invalid_inspected_event_view(
            invalid,
            target_relays,
            connected_relays,
            failed_relays,
        );
    }
    let Some(receipt) = sdk_receipt.receipt else {
        return ValidationReceiptInspectionView {
            state: "missing".to_owned(),
            resource: Some(validation_receipt_resource(receipt_event_id)),
            receipt_event_id: Some(receipt_event_id.to_owned()),
            order_id: None,
            validation_state: "missing".to_owned(),
            proof_verification: None,
            receipt: None,
            receipt_tags: None,
            event: None,
            target_relays,
            connected_relays,
            failed_relays,
            reason_code: Some("validation_receipt_not_found".to_owned()),
            reason: Some(format!(
                "validation receipt event `{receipt_event_id}` was not found on configured Nostr relays"
            )),
            sdk_error: None,
            actions: Vec::new(),
        };
    };
    inspected_event_view(
        receipt,
        success_state,
        intent,
        target_relays,
        connected_relays,
        failed_relays,
        reason_code,
    )
}

fn inspected_event_view(
    sdk_receipt: TradeValidationReceiptEvent,
    success_state: &str,
    intent: ValidationReceiptCommandIntent,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<RelayFailureView>,
    relay_reason_code: Option<String>,
) -> ValidationReceiptInspectionView {
    let event_id = sdk_receipt.event.id.clone();
    let order_id = sdk_receipt.tags.order_id.clone();
    let proof_verification = proof_verification_view_for_receipt(
        &sdk_receipt.receipt,
        sdk_worker_evidence_selection(sdk_receipt.worker_evidence),
    );
    let accepted = match intent {
        ValidationReceiptCommandIntent::Inspect => {
            !proof_state_is_invalid(proof_verification.state.as_str())
        }
        ValidationReceiptCommandIntent::Verify => {
            proof_verification.production_verification
                && proof_state_is_verification_success(proof_verification.state.as_str())
        }
    };
    if !accepted {
        return ValidationReceiptInspectionView {
            state: "invalid".to_owned(),
            resource: Some(validation_receipt_resource(&event_id)),
            receipt_event_id: Some(event_id),
            order_id: Some(order_id),
            validation_state: "invalid".to_owned(),
            proof_verification: Some(proof_verification.clone()),
            receipt: Some(sdk_receipt.receipt),
            receipt_tags: Some(tags_view(&sdk_receipt.tags)),
            event: Some(event_view(sdk_receipt.event)),
            target_relays,
            connected_relays,
            failed_relays,
            reason_code: proof_verification.reason_code.clone(),
            reason: proof_verification.reason.clone(),
            sdk_error: None,
            actions: Vec::new(),
        };
    }
    ValidationReceiptInspectionView {
        state: success_state.to_owned(),
        resource: Some(validation_receipt_resource(&event_id)),
        receipt_event_id: Some(event_id),
        order_id: Some(order_id),
        validation_state: "valid".to_owned(),
        proof_verification: Some(proof_verification),
        receipt: Some(sdk_receipt.receipt),
        receipt_tags: Some(tags_view(&sdk_receipt.tags)),
        event: Some(event_view(sdk_receipt.event)),
        target_relays,
        connected_relays,
        failed_relays,
        reason_code: relay_reason_code,
        reason: None,
        sdk_error: None,
        actions: Vec::new(),
    }
}

fn invalid_inspected_event_view(
    invalid: TradeValidationReceiptInvalidCandidate,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<RelayFailureView>,
) -> ValidationReceiptInspectionView {
    ValidationReceiptInspectionView {
        state: "invalid".to_owned(),
        resource: Some(validation_receipt_resource(&invalid.event.id)),
        receipt_event_id: Some(invalid.event.id.clone()),
        order_id: None,
        validation_state: "invalid".to_owned(),
        proof_verification: None,
        receipt: None,
        receipt_tags: None,
        event: Some(event_view(invalid.event)),
        target_relays,
        connected_relays,
        failed_relays,
        reason_code: Some(invalid.reason_code),
        reason: Some(invalid.reason),
        sdk_error: None,
        actions: Vec::new(),
    }
}

fn list_from_sdk_receipt(
    sdk_receipt: TradeValidationReceiptListReceipt,
) -> ValidationReceiptListView {
    let target_relays = sdk_receipt.relay_targets;
    let connected_relays = connected_relays(&sdk_receipt.relay_evidence.relays);
    let failed_relays = sdk_relay_failures(&sdk_receipt.relay_evidence.relays);
    let mut invalid_receipts = sdk_receipt
        .invalid_receipts
        .into_iter()
        .map(invalid_candidate_view)
        .collect::<Vec<_>>();
    let mut receipts = Vec::new();
    for sdk_event in sdk_receipt.receipts {
        let proof_verification = proof_verification_view_for_receipt(
            &sdk_event.receipt,
            sdk_worker_evidence_selection(sdk_event.worker_evidence),
        );
        if proof_state_is_invalid(proof_verification.state.as_str()) {
            invalid_receipts.push(ValidationReceiptInvalidCandidateView {
                receipt_event_id: sdk_event.event.id,
                kind: sdk_event.event.kind,
                reason_code: proof_verification
                    .reason_code
                    .clone()
                    .unwrap_or_else(|| proof_verification.state.clone()),
                reason: proof_verification.reason.clone().unwrap_or_else(|| {
                    "validation receipt proof material did not verify".to_owned()
                }),
                proof_verification: Some(proof_verification),
            });
        } else {
            receipts.push(summary_view(
                &sdk_event.event,
                &sdk_event.receipt,
                &sdk_event.tags,
                &proof_verification,
            ));
        }
    }

    let valid_count = receipts.len();
    let invalid_count = invalid_receipts.len();
    let state = if valid_count > 0 && invalid_count > 0 {
        "partial"
    } else if valid_count > 0 {
        "listed"
    } else if invalid_count > 0 {
        "invalid"
    } else {
        "empty"
    };
    let reason_code = if invalid_count > 0 {
        Some("validation_receipt_candidates_invalid".to_owned())
    } else if !failed_relays.is_empty() {
        Some("relay_fetch_partial".to_owned())
    } else {
        None
    };
    let reason = match state {
        "invalid" => Some(format!(
            "found {invalid_count} invalid validation receipt candidate(s) and no valid receipts"
        )),
        "partial" => Some(format!(
            "found {valid_count} valid receipt(s) and {invalid_count} invalid candidate(s)"
        )),
        _ => None,
    };

    ValidationReceiptListView {
        state: state.to_owned(),
        order_id: sdk_receipt.order_id.as_str().to_owned(),
        count: valid_count + invalid_count,
        valid_count,
        invalid_count,
        receipts,
        invalid_receipts,
        target_relays,
        connected_relays,
        failed_relays,
        reason_code,
        reason,
        sdk_error: None,
        actions: Vec::new(),
    }
}

fn inspection_sdk_error_view(
    receipt_event_id: &str,
    error: CliSdkAdapterError,
) -> ValidationReceiptInspectionView {
    let mapped = validation_receipt_sdk_error_parts(error);
    ValidationReceiptInspectionView {
        state: mapped.state,
        resource: Some(validation_receipt_resource(receipt_event_id)),
        receipt_event_id: Some(receipt_event_id.to_owned()),
        order_id: None,
        validation_state: "unverified".to_owned(),
        proof_verification: None,
        receipt: None,
        receipt_tags: None,
        event: None,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        reason_code: Some(mapped.reason_code),
        reason: Some(mapped.reason),
        sdk_error: mapped.sdk_error,
        actions: mapped.actions,
    }
}

fn list_sdk_error_view(order_id: &str, error: CliSdkAdapterError) -> ValidationReceiptListView {
    let mapped = validation_receipt_sdk_error_parts(error);
    ValidationReceiptListView {
        state: mapped.state,
        order_id: order_id.to_owned(),
        count: 0,
        valid_count: 0,
        invalid_count: 0,
        receipts: Vec::new(),
        invalid_receipts: Vec::new(),
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        reason_code: Some(mapped.reason_code),
        reason: Some(mapped.reason),
        sdk_error: mapped.sdk_error,
        actions: mapped.actions,
    }
}

struct ValidationReceiptSdkErrorParts {
    state: String,
    reason_code: String,
    reason: String,
    sdk_error: Option<Value>,
    actions: Vec<String>,
}

fn validation_receipt_sdk_error_parts(error: CliSdkAdapterError) -> ValidationReceiptSdkErrorParts {
    match error {
        CliSdkAdapterError::Sdk(error) => sdk_error_parts(error),
        CliSdkAdapterError::Runtime(error) => ValidationReceiptSdkErrorParts {
            state: "network_unavailable".to_owned(),
            reason_code: "sdk_runtime_failed".to_owned(),
            reason: error.to_string(),
            sdk_error: None,
            actions: Vec::new(),
        },
    }
}

fn sdk_error_parts(error: RadrootsSdkError) -> ValidationReceiptSdkErrorParts {
    let state = match error.code() {
        "empty_target_relays" => "unconfigured",
        _ => match error.class() {
            radroots_sdk::RadrootsSdkErrorClass::Configuration
            | radroots_sdk::RadrootsSdkErrorClass::Unsupported => "unconfigured",
            radroots_sdk::RadrootsSdkErrorClass::Request => "invalid",
            radroots_sdk::RadrootsSdkErrorClass::Transport
            | radroots_sdk::RadrootsSdkErrorClass::Storage
            | radroots_sdk::RadrootsSdkErrorClass::Clock
            | radroots_sdk::RadrootsSdkErrorClass::Authorization
            | radroots_sdk::RadrootsSdkErrorClass::LocalMutation => "network_unavailable",
            _ => "network_unavailable",
        },
    };
    let actions = if error.code() == "empty_target_relays" {
        vec![
            "radroots transport profile set --kind nostr --nostr-relay wss://relay.example.com"
                .to_owned(),
        ]
    } else {
        Vec::new()
    };
    ValidationReceiptSdkErrorParts {
        state: state.to_owned(),
        reason_code: error.code().to_owned(),
        reason: error.to_string(),
        sdk_error: Some(error.detail_json()),
        actions,
    }
}

fn invalid_inspection_view(
    receipt_event_id: Option<String>,
    reason_code: &str,
    reason: impl Into<String>,
) -> ValidationReceiptInspectionView {
    ValidationReceiptInspectionView {
        state: "invalid".to_owned(),
        resource: receipt_event_id.as_deref().map(validation_receipt_resource),
        receipt_event_id,
        order_id: None,
        validation_state: "invalid".to_owned(),
        proof_verification: None,
        receipt: None,
        receipt_tags: None,
        event: None,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        reason_code: Some(reason_code.to_owned()),
        reason: Some(reason.into()),
        sdk_error: None,
        actions: Vec::new(),
    }
}

fn invalid_list_view(
    order_id: String,
    reason_code: &str,
    reason: impl Into<String>,
) -> ValidationReceiptListView {
    ValidationReceiptListView {
        state: "invalid".to_owned(),
        order_id,
        count: 0,
        valid_count: 0,
        invalid_count: 0,
        receipts: Vec::new(),
        invalid_receipts: Vec::new(),
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        reason_code: Some(reason_code.to_owned()),
        reason: Some(reason.into()),
        sdk_error: None,
        actions: Vec::new(),
    }
}

fn validation_receipt_resource(id: &str) -> ValidationReceiptResourceView {
    ValidationReceiptResourceView {
        kind: "validation_receipt".to_owned(),
        id: id.to_owned(),
    }
}

fn event_view(event: radroots_events::RadrootsNostrEvent) -> ValidationReceiptEventView {
    ValidationReceiptEventView {
        id: event.id,
        author: event.author,
        created_at: event.created_at,
        kind: event.kind,
        sig: event.sig,
        tags: event.tags,
        content: event.content,
    }
}

fn tags_view(tags: &TradeValidationReceiptTags) -> ValidationReceiptTagsView {
    ValidationReceiptTagsView {
        order_id: tags.order_id.clone(),
        event_set_root: tags.event_set_root.clone(),
        listing_event_id: tags.listing_event_id.clone(),
        reducer_output_root: tags.reducer_output_root.clone(),
        public_values_hash: tags.public_values_hash.clone(),
        proof_system: tags.proof_system.clone(),
        receipt_type: tags.receipt_type.clone(),
        root_event_id: tags.root_event_id.clone(),
        target_event_id: tags.target_event_id.clone(),
    }
}

struct ValidationReceiptTrustSummary {
    state: &'static str,
    validation_authority: Option<String>,
    commitment_confidence: Option<String>,
    production_verification: bool,
    reason_code: Option<&'static str>,
    reason: Option<&'static str>,
}

fn local_only_trust_summary() -> ValidationReceiptTrustSummary {
    ValidationReceiptTrustSummary {
        state: "local_only_deterministic_receipt",
        validation_authority: Some(
            RadrootsTradeValidationAuthority::DevDeterministicOnly
                .as_str()
                .to_owned(),
        ),
        commitment_confidence: Some(
            RadrootsTradeCommitmentConfidence::LocalOnly
                .as_str()
                .to_owned(),
        ),
        production_verification: false,
        reason_code: None,
        reason: None,
    }
}

fn sp1_execute_checked_trust_summary() -> ValidationReceiptTrustSummary {
    ValidationReceiptTrustSummary {
        state: "sp1_execute_checked",
        validation_authority: None,
        commitment_confidence: None,
        production_verification: false,
        reason_code: Some("validation_receipt_trust_metadata_missing"),
        reason: Some(
            "trusted worker evidence reports SP1 execution but omits validation authority or commitment confidence",
        ),
    }
}

fn missing_trust_metadata_summary() -> ValidationReceiptTrustSummary {
    ValidationReceiptTrustSummary {
        state: "worker_evidence_trust_metadata_missing",
        validation_authority: None,
        commitment_confidence: None,
        production_verification: false,
        reason_code: Some("validation_receipt_trust_metadata_missing"),
        reason: Some("trusted worker evidence omits validation authority or commitment confidence"),
    }
}

fn mismatched_trust_metadata_summary() -> ValidationReceiptTrustSummary {
    ValidationReceiptTrustSummary {
        state: "worker_evidence_trust_metadata_mismatch",
        validation_authority: None,
        commitment_confidence: None,
        production_verification: false,
        reason_code: Some("validation_receipt_trust_metadata_mismatch"),
        reason: Some(
            "trusted worker evidence validation authority does not match commitment confidence",
        ),
    }
}

fn invalid_worker_evidence_summary() -> ValidationReceiptTrustSummary {
    ValidationReceiptTrustSummary {
        state: "validation_receipt_worker_evidence_invalid",
        validation_authority: None,
        commitment_confidence: Some(
            RadrootsTradeCommitmentConfidence::Invalid
                .as_str()
                .to_owned(),
        ),
        production_verification: false,
        reason_code: Some("validation_receipt_worker_evidence_invalid"),
        reason: Some("trusted worker evidence marks the validation receipt invalid"),
    }
}

fn trusted_worker_summary(
    state: &'static str,
    authority: &str,
    confidence: &str,
    production_verification: bool,
) -> ValidationReceiptTrustSummary {
    ValidationReceiptTrustSummary {
        state,
        validation_authority: Some(authority.to_owned()),
        commitment_confidence: Some(confidence.to_owned()),
        production_verification,
        reason_code: None,
        reason: None,
    }
}

fn none_proof_trust_summary(
    worker_evidence: &ValidationReceiptWorkerEvidenceSelection,
) -> ValidationReceiptTrustSummary {
    let Some(evidence) = worker_evidence.trusted.as_ref() else {
        return local_only_trust_summary();
    };
    let authority = evidence.validation_authority.as_deref();
    let confidence = evidence.commitment_confidence.as_deref();
    if authority.is_none() && confidence.is_none() {
        return if evidence.sp1_execute_checked {
            sp1_execute_checked_trust_summary()
        } else {
            local_only_trust_summary()
        };
    }
    let (Some(authority), Some(confidence)) = (authority, confidence) else {
        return missing_trust_metadata_summary();
    };
    match (authority, confidence) {
        ("dev_deterministic_only", "local_only") => local_only_trust_summary(),
        ("trusted_rhi_service_key", "pending_rhi") => {
            trusted_worker_summary("pending_rhi_validation", authority, confidence, false)
        }
        ("trusted_rhi_service_key", "committed_by_trusted_service") => {
            trusted_worker_summary("trusted_service_validated", authority, confidence, false)
        }
        ("cryptographic_proof_verified", "committed_by_cryptographic_proof") => {
            trusted_worker_summary("cryptographic_proof_verified", authority, confidence, true)
        }
        ("trusted_service_and_proof_verified", "committed_by_trusted_service_and_proof") => {
            trusted_worker_summary(
                "trusted_service_and_proof_verified",
                authority,
                confidence,
                true,
            )
        }
        (_, "invalid") => invalid_worker_evidence_summary(),
        _ => mismatched_trust_metadata_summary(),
    }
}

fn verified_sp1_trust_summary(
    worker_evidence: &ValidationReceiptWorkerEvidenceSelection,
) -> ValidationReceiptTrustSummary {
    if let Some(evidence) = worker_evidence.trusted.as_ref()
        && evidence.validation_authority.as_deref() == Some("trusted_service_and_proof_verified")
        && evidence.commitment_confidence.as_deref()
            == Some("committed_by_trusted_service_and_proof")
    {
        return trusted_worker_summary(
            "trusted_service_and_proof_verified",
            "trusted_service_and_proof_verified",
            "committed_by_trusted_service_and_proof",
            true,
        );
    }
    trusted_worker_summary(
        "sp1_inline_proof_verified",
        "cryptographic_proof_verified",
        "committed_by_cryptographic_proof",
        true,
    )
}

fn proof_verification_view_for_receipt(
    receipt: &RadrootsTradeValidationReceipt,
    worker_evidence: ValidationReceiptWorkerEvidenceSelection,
) -> ValidationReceiptProofVerificationView {
    let proof = &receipt.proof;
    let cryptographic_proof_required = proof.system != RadrootsValidationReceiptProofSystem::None;
    if proof.system == RadrootsValidationReceiptProofSystem::None {
        let trust = none_proof_trust_summary(&worker_evidence);
        return ValidationReceiptProofVerificationView {
            state: trust.state.to_owned(),
            verifier: "radroots_cli_validation_receipt_v1".to_owned(),
            proof_system: proof.system.as_str().to_owned(),
            validation_authority: trust.validation_authority,
            commitment_confidence: trust.commitment_confidence,
            production_verification: trust.production_verification,
            public_values_hash_binding: "verified".to_owned(),
            proof_metadata_binding: "not_required".to_owned(),
            cryptographic_proof_required,
            cryptographic_proof_verified: false,
            mode: proof.mode.clone(),
            program_hash: proof.program_hash.clone(),
            verifying_key_hash: proof.verifying_key_hash.clone(),
            proof_reference: proof.proof_reference.clone(),
            inline_proof_present: proof.inline_proof_base64.is_some(),
            worker_evidence: worker_evidence.trusted,
            untrusted_worker_evidence: worker_evidence.untrusted,
            reason_code: trust.reason_code.map(str::to_owned),
            reason: trust.reason.map(str::to_owned),
        };
    }
    if proof.proof_reference.is_some() {
        return sp1_unverified_proof_view(
            receipt,
            worker_evidence,
            "sp1_reference_unresolved",
            "unverified",
            "reference_unresolved",
            Some("sp1_reference_unresolved"),
            Some("SP1 proof reference resolution is not implemented by this CLI"),
        );
    }
    if proof.inline_proof_base64.is_none() {
        return sp1_unverified_proof_view(
            receipt,
            worker_evidence,
            "sp1_proof_material_missing",
            "unverified",
            "missing_proof_material",
            Some("sp1_proof_material_missing"),
            Some("SP1 proof material is missing"),
        );
    }
    if proof.system != RadrootsValidationReceiptProofSystem::Sp1Core {
        return sp1_unverified_proof_view(
            receipt,
            worker_evidence,
            "sp1_metadata_consistent_unverified",
            "unverified",
            "metadata_consistent_unverified",
            Some("sp1_inline_proof_verification_unsupported"),
            Some("only inline sp1_core proof verification is active in this CLI"),
        );
    }

    match verify_inline_sp1_receipt(receipt) {
        Ok(()) => {
            let trust = verified_sp1_trust_summary(&worker_evidence);
            ValidationReceiptProofVerificationView {
                state: trust.state.to_owned(),
                verifier: "radroots_cli_validation_receipt_v1".to_owned(),
                proof_system: proof.system.as_str().to_owned(),
                validation_authority: trust.validation_authority,
                commitment_confidence: trust.commitment_confidence,
                production_verification: trust.production_verification,
                public_values_hash_binding: "verified".to_owned(),
                proof_metadata_binding: "verified".to_owned(),
                cryptographic_proof_required,
                cryptographic_proof_verified: true,
                mode: proof.mode.clone(),
                program_hash: proof.program_hash.clone(),
                verifying_key_hash: proof.verifying_key_hash.clone(),
                proof_reference: proof.proof_reference.clone(),
                inline_proof_present: proof.inline_proof_base64.is_some(),
                worker_evidence: worker_evidence.trusted,
                untrusted_worker_evidence: worker_evidence.untrusted,
                reason_code: trust.reason_code.map(str::to_owned),
                reason: trust.reason.map(str::to_owned),
            }
        }
        Err(error) => {
            let mapped = proof_state_from_sp1_error(&error);
            let reason = error.to_string();
            sp1_unverified_proof_view(
                receipt,
                worker_evidence,
                mapped.state,
                mapped.public_values_hash_binding,
                mapped.proof_metadata_binding,
                Some(mapped.reason_code),
                Some(reason.as_str()),
            )
        }
    }
}

fn sp1_unverified_proof_view(
    receipt: &RadrootsTradeValidationReceipt,
    worker_evidence: ValidationReceiptWorkerEvidenceSelection,
    state: &str,
    public_values_hash_binding: &str,
    proof_metadata_binding: &str,
    reason_code: Option<&str>,
    reason: Option<&str>,
) -> ValidationReceiptProofVerificationView {
    let proof = &receipt.proof;
    ValidationReceiptProofVerificationView {
        state: state.to_owned(),
        verifier: "radroots_cli_validation_receipt_v1".to_owned(),
        proof_system: proof.system.as_str().to_owned(),
        validation_authority: None,
        commitment_confidence: None,
        production_verification: false,
        public_values_hash_binding: public_values_hash_binding.to_owned(),
        proof_metadata_binding: proof_metadata_binding.to_owned(),
        cryptographic_proof_required: proof.system != RadrootsValidationReceiptProofSystem::None,
        cryptographic_proof_verified: false,
        mode: proof.mode.clone(),
        program_hash: proof.program_hash.clone(),
        verifying_key_hash: proof.verifying_key_hash.clone(),
        proof_reference: proof.proof_reference.clone(),
        inline_proof_present: proof.inline_proof_base64.is_some(),
        worker_evidence: worker_evidence.trusted,
        untrusted_worker_evidence: worker_evidence.untrusted,
        reason_code: reason_code.map(str::to_owned),
        reason: reason.map(str::to_owned),
    }
}

#[cfg(test)]
fn validation_receipt_invalid_reason_code(
    error: &radroots_trade::validation_receipt::RadrootsValidationReceiptError,
) -> &'static str {
    use radroots_trade::validation_receipt::RadrootsValidationReceiptError;

    match error {
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.material")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.material_missing") => {
            "sp1_proof_material_missing"
        }
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.material_conflict") => {
            "sp1_proof_material_conflict"
        }
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.inline_proof_base64") => {
            "sp1_inline_proof_invalid"
        }
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.proof_reference") => {
            "sp1_proof_reference_invalid"
        }
        RadrootsValidationReceiptError::TagMismatch("public_values_hash") => {
            "public_values_hash_mismatch"
        }
        RadrootsValidationReceiptError::ExpectedBindingMismatch("public_values_hash") => {
            "public_values_hash_mismatch"
        }
        RadrootsValidationReceiptError::ExpectedBindingMismatch("program_hash") => {
            "sp1_program_hash_mismatch"
        }
        RadrootsValidationReceiptError::ExpectedBindingMismatch("verifying_key_hash") => {
            "sp1_verifying_key_hash_mismatch"
        }
        _ => "validation_receipt_invalid",
    }
}

struct MappedSp1ProofError {
    state: &'static str,
    public_values_hash_binding: &'static str,
    proof_metadata_binding: &'static str,
    reason_code: &'static str,
}

fn proof_state_from_sp1_error(error: &RadrootsSp1TradeHostError) -> MappedSp1ProofError {
    match error {
        RadrootsSp1TradeHostError::Sp1ProofReferenceUnresolved => MappedSp1ProofError {
            state: "sp1_reference_unresolved",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "reference_unresolved",
            reason_code: "sp1_reference_unresolved",
        },
        RadrootsSp1TradeHostError::MissingProofMaterial => MappedSp1ProofError {
            state: "sp1_proof_material_missing",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "missing_proof_material",
            reason_code: "sp1_proof_material_missing",
        },
        RadrootsSp1TradeHostError::ProofMaterialConflict => MappedSp1ProofError {
            state: "sp1_proof_material_conflict",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "conflicting_proof_material",
            reason_code: "sp1_proof_material_conflict",
        },
        RadrootsSp1TradeHostError::PublicValuesHashMismatch
        | RadrootsSp1TradeHostError::Sp1PublicValuesMismatch
        | RadrootsSp1TradeHostError::ValidationReceiptBindingMismatch(_) => MappedSp1ProofError {
            state: "sp1_public_values_mismatch",
            public_values_hash_binding: "mismatch",
            proof_metadata_binding: "verified",
            reason_code: "sp1_public_values_mismatch",
        },
        RadrootsSp1TradeHostError::Sp1ProgramHashMismatch
        | RadrootsSp1TradeHostError::MissingSp1ProgramHash => MappedSp1ProofError {
            state: "sp1_program_hash_mismatch",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "mismatch",
            reason_code: "sp1_program_hash_mismatch",
        },
        RadrootsSp1TradeHostError::Sp1VerifyingKeyHashMismatch
        | RadrootsSp1TradeHostError::MissingVerifyingKeyHash => MappedSp1ProofError {
            state: "sp1_verifying_key_hash_mismatch",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "mismatch",
            reason_code: "sp1_verifying_key_hash_mismatch",
        },
        RadrootsSp1TradeHostError::Sp1ProofVerifierUnavailable => MappedSp1ProofError {
            state: "sp1_verifier_unavailable",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "verifier_unavailable",
            reason_code: "sp1_verifier_unavailable",
        },
        RadrootsSp1TradeHostError::Sp1SetupFailed(_) => MappedSp1ProofError {
            state: "sp1_verifier_setup_failed",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "verifier_setup_failed",
            reason_code: "sp1_verifier_setup_failed",
        },
        _ => MappedSp1ProofError {
            state: "sp1_proof_invalid",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "invalid",
            reason_code: "sp1_proof_invalid",
        },
    }
}

fn verify_inline_sp1_receipt(
    receipt: &RadrootsTradeValidationReceipt,
) -> Result<(), RadrootsSp1TradeHostError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| RadrootsSp1TradeHostError::Sp1SetupFailed(error.to_string()))?;
    runtime
        .block_on(verify_order_acceptance_validation_receipt_inline_sp1_proof(
            receipt,
        ))
        .map(|_| ())
}

fn proof_state_is_invalid(state: &str) -> bool {
    matches!(
        state,
        "sp1_proof_material_missing"
            | "sp1_proof_material_conflict"
            | "sp1_public_values_mismatch"
            | "sp1_program_hash_mismatch"
            | "sp1_verifying_key_hash_mismatch"
            | "sp1_proof_invalid"
            | "validation_receipt_worker_evidence_invalid"
            | "worker_evidence_trust_metadata_mismatch"
    )
}

fn proof_state_is_verification_success(state: &str) -> bool {
    matches!(
        state,
        "trusted_service_validated"
            | "cryptographic_proof_verified"
            | "trusted_service_and_proof_verified"
            | "sp1_inline_proof_verified"
    )
}

fn invalid_candidate_view(
    candidate: TradeValidationReceiptInvalidCandidate,
) -> ValidationReceiptInvalidCandidateView {
    ValidationReceiptInvalidCandidateView {
        receipt_event_id: candidate.event.id,
        kind: candidate.event.kind,
        reason_code: candidate.reason_code,
        reason: candidate.reason,
        proof_verification: None,
    }
}

fn sdk_worker_evidence_selection(
    selection: SdkWorkerEvidenceSelection,
) -> ValidationReceiptWorkerEvidenceSelection {
    ValidationReceiptWorkerEvidenceSelection {
        trusted: selection.trusted.map(worker_evidence_view),
        untrusted: selection.untrusted.map(worker_evidence_view),
    }
}

fn worker_evidence_view(
    evidence: TradeValidationReceiptWorkerEvidence,
) -> ValidationReceiptWorkerEvidenceView {
    ValidationReceiptWorkerEvidenceView {
        result_event_id: evidence.result_event_id.as_str().to_owned(),
        author: evidence.author.as_str().to_owned(),
        validation_authority: evidence
            .validation_authority
            .map(|authority| authority.as_str().to_owned()),
        commitment_confidence: evidence
            .commitment_confidence
            .map(|confidence| confidence.as_str().to_owned()),
        status: evidence.status,
        prover_backend: evidence.prover_backend,
        proof_mode: evidence.proof_mode,
        proof_system: evidence.proof_system,
        proof_generated: evidence.proof_generated,
        sp1_execute_checked: evidence.sp1_execute_checked,
        sp1_execute_public_values_hash: evidence.sp1_execute_public_values_hash,
        cryptographic_proof_verified: evidence.cryptographic_proof_verified,
        public_values_hash: evidence.public_values_hash,
    }
}

fn connected_relays(relays: &[TradeValidationReceiptRelayOutcomeReceipt]) -> Vec<String> {
    relays
        .iter()
        .filter(|relay| relay.outcome_kind == TradeValidationReceiptRelayOutcomeKind::Eose)
        .map(|relay| relay.relay_url.clone())
        .collect()
}

fn sdk_relay_failures(
    relays: &[TradeValidationReceiptRelayOutcomeReceipt],
) -> Vec<RelayFailureView> {
    relays
        .iter()
        .filter(|relay| relay.outcome_kind != TradeValidationReceiptRelayOutcomeKind::Eose)
        .map(|relay| RelayFailureView {
            relay: relay.relay_url.clone(),
            reason: relay
                .message
                .clone()
                .unwrap_or_else(|| sdk_relay_outcome_kind(relay.outcome_kind).to_owned()),
        })
        .collect()
}

fn sdk_relay_outcome_kind(kind: TradeValidationReceiptRelayOutcomeKind) -> &'static str {
    match kind {
        TradeValidationReceiptRelayOutcomeKind::Eose => "eose",
        TradeValidationReceiptRelayOutcomeKind::Closed => "closed",
        TradeValidationReceiptRelayOutcomeKind::Notice => "notice",
        _ => "unknown",
    }
}

fn summary_view(
    event: &radroots_events::RadrootsNostrEvent,
    receipt: &RadrootsTradeValidationReceipt,
    tags: &TradeValidationReceiptTags,
    proof_verification: &ValidationReceiptProofVerificationView,
) -> ValidationReceiptSummaryView {
    ValidationReceiptSummaryView {
        resource: validation_receipt_resource(&event.id),
        receipt_event_id: event.id.clone(),
        order_id: tags.order_id.clone(),
        author: event.author.clone(),
        created_at: event.created_at,
        receipt_type: receipt_type_label(receipt.receipt_type).to_owned(),
        result: receipt_result_label(receipt.result).to_owned(),
        proof_system: receipt.proof.system.as_str().to_owned(),
        proof_verification_state: proof_verification.state.clone(),
        validation_authority: proof_verification.validation_authority.clone(),
        commitment_confidence: proof_verification.commitment_confidence.clone(),
        production_verification: proof_verification.production_verification,
        event_set_root: receipt.event_set_root.clone(),
        reducer_output_root: receipt.new_state_root.clone(),
        public_values_hash: receipt.public_values_hash.clone(),
    }
}

fn receipt_type_label(value: RadrootsValidationReceiptType) -> &'static str {
    value.as_str()
}

fn receipt_result_label(value: RadrootsValidationReceiptResult) -> &'static str {
    match value {
        RadrootsValidationReceiptResult::Valid => "valid",
        RadrootsValidationReceiptResult::Invalid => "invalid",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ValidationReceiptWorkerEvidenceSelection, ValidationReceiptWorkerEvidenceView,
        proof_state_from_sp1_error, proof_state_is_invalid, proof_state_is_verification_success,
        proof_verification_view_for_receipt, validation_receipt_invalid_reason_code,
    };
    use radroots_sp1_host_trade::RadrootsSp1TradeHostError;
    use radroots_trade::validation_receipt::{
        RadrootsTradeValidationReceipt, RadrootsValidationReceiptError,
        RadrootsValidationReceiptProof, RadrootsValidationReceiptProofSystem,
        RadrootsValidationReceiptResult, RadrootsValidationReceiptStatement,
        RadrootsValidationReceiptType, VALIDATION_RECEIPT_DOMAIN, VALIDATION_RECEIPT_VERSION,
    };

    fn sp1_proof_with_material() -> RadrootsValidationReceiptProof {
        RadrootsValidationReceiptProof {
            inline_proof_base64: Some("cHJvb2Y=".to_owned()),
            mode: Some("core".to_owned()),
            program_hash: Some(
                "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            ),
            proof_reference: None,
            system: RadrootsValidationReceiptProofSystem::Sp1Core,
            verifying_key_hash: Some(
                "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
            ),
        }
    }

    fn receipt_with_proof(proof: RadrootsValidationReceiptProof) -> RadrootsTradeValidationReceipt {
        RadrootsTradeValidationReceipt {
            changed_records_root:
                "0x1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            domain: VALIDATION_RECEIPT_DOMAIN.to_owned(),
            error_bitmap: "0x00000000000000000000000000000000".to_owned(),
            event_set_root: "0x2222222222222222222222222222222222222222222222222222222222222222"
                .to_owned(),
            new_state_root: "0x3333333333333333333333333333333333333333333333333333333333333333"
                .to_owned(),
            previous_state_root:
                "0x4444444444444444444444444444444444444444444444444444444444444444".to_owned(),
            proof,
            public_values_hash:
                "0x5555555555555555555555555555555555555555555555555555555555555555".to_owned(),
            receipt_type: RadrootsValidationReceiptType::TradeTransition,
            result: RadrootsValidationReceiptResult::Valid,
            statement: RadrootsValidationReceiptStatement {
                listing_event_id:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                root_event_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    .to_owned(),
                target_event_id: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    .to_owned(),
                statement_type: RadrootsValidationReceiptType::TradeTransition,
            },
            version: VALIDATION_RECEIPT_VERSION,
        }
    }

    fn deterministic_receipt() -> RadrootsTradeValidationReceipt {
        receipt_with_proof(RadrootsValidationReceiptProof {
            inline_proof_base64: None,
            mode: None,
            program_hash: None,
            proof_reference: None,
            system: RadrootsValidationReceiptProofSystem::None,
            verifying_key_hash: None,
        })
    }

    fn worker_evidence(
        validation_authority: Option<&str>,
        commitment_confidence: Option<&str>,
        sp1_execute_checked: bool,
    ) -> ValidationReceiptWorkerEvidenceView {
        ValidationReceiptWorkerEvidenceView {
            result_event_id: "result-1".to_owned(),
            author: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            validation_authority: validation_authority.map(str::to_owned),
            commitment_confidence: commitment_confidence.map(str::to_owned),
            status: "succeeded".to_owned(),
            prover_backend: "local_execute".to_owned(),
            proof_mode: "none".to_owned(),
            proof_system: "none".to_owned(),
            proof_generated: false,
            sp1_execute_checked,
            sp1_execute_public_values_hash: sp1_execute_checked.then(|| {
                "0x5555555555555555555555555555555555555555555555555555555555555555".to_owned()
            }),
            cryptographic_proof_verified: false,
            public_values_hash:
                "0x5555555555555555555555555555555555555555555555555555555555555555".to_owned(),
        }
    }

    #[test]
    fn none_receipts_report_local_only_without_crypto_claim() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection::default(),
        );

        assert_eq!(view.state, "local_only_deterministic_receipt");
        assert_eq!(
            view.validation_authority.as_deref(),
            Some("dev_deterministic_only")
        );
        assert_eq!(view.commitment_confidence.as_deref(), Some("local_only"));
        assert!(!view.production_verification);
        assert!(!view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
    }

    #[test]
    fn none_receipts_surface_advisory_sp1_execute_evidence() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection {
                trusted: Some(worker_evidence(None, None, true)),
                untrusted: None,
            },
        );

        assert_eq!(view.state, "sp1_execute_checked");
        assert_eq!(
            view.reason_code.as_deref(),
            Some("validation_receipt_trust_metadata_missing")
        );
        assert!(!view.production_verification);
        assert!(!view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
    }

    #[test]
    fn none_receipts_surface_trusted_service_confidence_without_production_verification() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection {
                trusted: Some(worker_evidence(
                    Some("trusted_rhi_service_key"),
                    Some("committed_by_trusted_service"),
                    false,
                )),
                untrusted: None,
            },
        );

        assert_eq!(view.state, "trusted_service_validated");
        assert_eq!(
            view.validation_authority.as_deref(),
            Some("trusted_rhi_service_key")
        );
        assert_eq!(
            view.commitment_confidence.as_deref(),
            Some("committed_by_trusted_service")
        );
        assert!(!view.production_verification);
        assert!(proof_state_is_verification_success(view.state.as_str()));
    }

    #[test]
    fn invalid_worker_evidence_marks_receipt_invalid() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection {
                trusted: Some(worker_evidence(
                    Some("trusted_rhi_service_key"),
                    Some("invalid"),
                    false,
                )),
                untrusted: None,
            },
        );

        assert_eq!(view.state, "validation_receipt_worker_evidence_invalid");
        assert!(proof_state_is_invalid(view.state.as_str()));
        assert!(!view.production_verification);
    }

    #[test]
    fn untrusted_worker_evidence_does_not_upgrade_deterministic_receipts() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection {
                trusted: None,
                untrusted: Some(worker_evidence(
                    Some("trusted_rhi_service_key"),
                    Some("committed_by_trusted_service"),
                    true,
                )),
            },
        );

        assert_eq!(view.state, "local_only_deterministic_receipt");
        assert!(!view.production_verification);
        assert!(view.worker_evidence.is_none());
        assert!(view.untrusted_worker_evidence.is_some());
    }

    #[test]
    fn validation_success_labels_exclude_local_only_and_sp1_execute_checked() {
        assert!(!proof_state_is_verification_success(
            "local_only_deterministic_receipt"
        ));
        assert!(!proof_state_is_verification_success("sp1_execute_checked"));
        assert!(proof_state_is_verification_success(
            "trusted_service_validated"
        ));
        assert!(proof_state_is_verification_success(
            "sp1_inline_proof_verified"
        ));
        assert!(proof_state_is_verification_success(
            "trusted_service_and_proof_verified"
        ));
    }

    #[test]
    fn sp1_receipts_with_references_report_unresolved_without_crypto_claim() {
        let mut receipt = receipt_with_proof(sp1_proof_with_material());
        receipt.proof.inline_proof_base64 = None;
        receipt.proof.proof_reference = Some(format!("radroots-proof://sha256/{}", "1".repeat(64)));

        let view = proof_verification_view_for_receipt(
            &receipt,
            ValidationReceiptWorkerEvidenceSelection::default(),
        );

        assert_eq!(view.state, "sp1_reference_unresolved");
        assert!(view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
        assert_eq!(view.proof_metadata_binding, "reference_unresolved");
    }

    #[cfg(feature = "sp1-verify")]
    #[test]
    fn invalid_inline_sp1_material_reports_invalid_proof_state() {
        let view = proof_verification_view_for_receipt(
            &receipt_with_proof(sp1_proof_with_material()),
            ValidationReceiptWorkerEvidenceSelection::default(),
        );

        assert_eq!(view.state, "sp1_proof_invalid");
        assert!(view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
        assert_eq!(view.reason_code.as_deref(), Some("sp1_proof_invalid"));
    }

    #[cfg(not(feature = "sp1-verify"))]
    #[test]
    fn inline_sp1_material_reports_unavailable_verifier_without_sp1_verify_feature() {
        let view = proof_verification_view_for_receipt(
            &receipt_with_proof(sp1_proof_with_material()),
            ValidationReceiptWorkerEvidenceSelection::default(),
        );

        assert_eq!(view.state, "sp1_verifier_unavailable");
        assert!(view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
        assert_eq!(view.public_values_hash_binding, "unverified");
        assert_eq!(view.proof_metadata_binding, "verifier_unavailable");
        assert_eq!(
            view.reason_code.as_deref(),
            Some("sp1_verifier_unavailable")
        );
        assert!(!proof_state_is_invalid(view.state.as_str()));
    }

    #[test]
    fn sp1_setup_failed_reports_verifier_setup_failure_without_invalid_proof_state() {
        let mapped = proof_state_from_sp1_error(&RadrootsSp1TradeHostError::Sp1SetupFailed(
            "runtime unavailable".to_owned(),
        ));

        assert_eq!(mapped.state, "sp1_verifier_setup_failed");
        assert_eq!(mapped.public_values_hash_binding, "unverified");
        assert_eq!(mapped.proof_metadata_binding, "verifier_setup_failed");
        assert_eq!(mapped.reason_code, "sp1_verifier_setup_failed");
        assert!(!proof_state_is_invalid(mapped.state));
    }

    #[test]
    fn invalid_receipt_errors_get_specific_reason_codes() {
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::InvalidProofMetadata("proof.material")
            ),
            "sp1_proof_material_missing"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::InvalidProofMetadata("proof.material_missing")
            ),
            "sp1_proof_material_missing"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::InvalidProofMetadata("proof.material_conflict")
            ),
            "sp1_proof_material_conflict"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(&RadrootsValidationReceiptError::TagMismatch(
                "public_values_hash"
            )),
            "public_values_hash_mismatch"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::InvalidProofMetadata("proof.inline_proof_base64")
            ),
            "sp1_inline_proof_invalid"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::InvalidProofMetadata("proof.proof_reference")
            ),
            "sp1_proof_reference_invalid"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::ExpectedBindingMismatch("program_hash")
            ),
            "sp1_program_hash_mismatch"
        );
        assert_eq!(
            validation_receipt_invalid_reason_code(
                &RadrootsValidationReceiptError::ExpectedBindingMismatch("verifying_key_hash")
            ),
            "sp1_verifying_key_hash_mismatch"
        );
    }
}
