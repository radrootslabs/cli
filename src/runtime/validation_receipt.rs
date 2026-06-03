use std::collections::{BTreeMap, BTreeSet};

use radroots_events::kinds::{
    KIND_TRADE_VALIDATION_RECEIPT, KIND_WORKER_TRADE_TRANSITION_PROOF_RES,
};
use radroots_nostr::prelude::{
    RadrootsNostrEvent, RadrootsNostrEventId, RadrootsNostrFilter, RadrootsNostrKind,
    radroots_event_from_nostr, radroots_nostr_filter_tag,
};
#[cfg(feature = "sp1-verify")]
use radroots_sp1_host_trade::verify_order_acceptance_validation_receipt_inline_sp1_proof;
use radroots_sp1_host_trade::{
    RadrootsSp1TradeHostError, RadrootsSp1TradeProofMode, RadrootsSp1TradeProverBackend,
    RadrootsSp1TradeWorkerResultPayload, RadrootsSp1TradeWorkerResultStatus,
    RadrootsSp1TradeWorkerRole,
};
use radroots_trade::validation_receipt::{
    RadrootsTradeValidationReceipt, RadrootsValidationReceiptError,
    RadrootsValidationReceiptExpectedBinding, RadrootsValidationReceiptProofSystem,
    RadrootsValidationReceiptResult, RadrootsValidationReceiptTags, RadrootsValidationReceiptType,
    verify_validation_receipt_event,
};
use serde::{Deserialize, Serialize};

use crate::runtime::config::RuntimeConfig;
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayFetchError, DirectRelayFetchReceipt, fetch_events_from_relays,
};
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

#[derive(Debug, Deserialize)]
struct RawValidationReceiptWorkerResultPayload {
    cryptographic_proof_verified: bool,
    decision_event_id: Option<String>,
    event_set_root: Option<String>,
    listing_event_id: Option<String>,
    order_id: Option<String>,
    proof_generated: bool,
    proof_mode: String,
    proof_system: String,
    public_values_hash: String,
    prover_backend: String,
    receipt_kind: Option<u32>,
    receipt_event_id: String,
    reducer_output_root: Option<String>,
    request_event_id: Option<String>,
    sp1_execute_checked: bool,
    sp1_execute_public_values_hash: Option<String>,
    status: String,
    worker_role: Option<String>,
}

impl RawValidationReceiptWorkerResultPayload {
    fn typed(&self) -> Option<RadrootsSp1TradeWorkerResultPayload> {
        Some(RadrootsSp1TradeWorkerResultPayload {
            cryptographic_proof_verified: self.cryptographic_proof_verified,
            decision_event_id: self.decision_event_id.clone(),
            event_set_root: self.event_set_root.clone(),
            listing_event_id: self.listing_event_id.clone(),
            order_id: self.order_id.clone(),
            proof_generated: self.proof_generated,
            proof_mode: RadrootsSp1TradeProofMode::from_label(self.proof_mode.as_str())?,
            proof_system: RadrootsValidationReceiptProofSystem::from_label(
                self.proof_system.as_str(),
            )?,
            public_values_hash: self.public_values_hash.clone(),
            prover_backend: RadrootsSp1TradeProverBackend::from_label(
                self.prover_backend.as_str(),
            )?,
            receipt_event_id: self.receipt_event_id.clone(),
            receipt_kind: self.receipt_kind,
            reducer_output_root: self.reducer_output_root.clone(),
            request_event_id: self.request_event_id.clone(),
            sp1_execute_checked: self.sp1_execute_checked,
            sp1_execute_public_values_hash: self.sp1_execute_public_values_hash.clone(),
            status: match self.status.as_str() {
                "succeeded" => RadrootsSp1TradeWorkerResultStatus::Succeeded,
                _ => return None,
            },
            worker_role: match self.worker_role.as_deref() {
                Some("non_authoritative_prover") => {
                    Some(RadrootsSp1TradeWorkerRole::NonAuthoritativeProver)
                }
                Some(_) => return None,
                None => None,
            },
        })
    }
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
    let filter = match validation_receipt_order_filter(order_id) {
        Ok(filter) => filter,
        Err(reason) => return invalid_list_view(order_id.to_owned(), "invalid_order_id", reason),
    };
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(error) => return list_fetch_error_view(order_id, error),
    };
    list_from_fetch_receipt(config, order_id, receipt)
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
    let event_id = match RadrootsNostrEventId::parse(receipt_event_id) {
        Ok(event_id) => event_id,
        Err(error) => {
            return invalid_inspection_view(
                Some(receipt_event_id.to_owned()),
                "invalid_receipt_event_id",
                format!("invalid validation receipt event id `{receipt_event_id}`: {error}"),
            );
        }
    };
    let filter = RadrootsNostrFilter::new().id(event_id);
    let receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(receipt) => receipt,
        Err(error) => return inspection_fetch_error_view(receipt_event_id, error),
    };
    inspection_from_fetch_receipt(config, receipt_event_id, success_state, intent, receipt)
}

fn validation_receipt_order_filter(order_id: &str) -> Result<RadrootsNostrFilter, String> {
    let filter = RadrootsNostrFilter::new().kind(RadrootsNostrKind::Custom(
        KIND_TRADE_VALIDATION_RECEIPT as u16,
    ));
    radroots_nostr_filter_tag(filter, "d", vec![order_id.to_owned()])
        .map_err(|error| format!("build validation receipt order filter: {error}"))
}

fn inspection_from_fetch_receipt(
    config: &RuntimeConfig,
    receipt_event_id: &str,
    success_state: &str,
    intent: ValidationReceiptCommandIntent,
    fetch_receipt: DirectRelayFetchReceipt,
) -> ValidationReceiptInspectionView {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        mut events,
    } = fetch_receipt;
    events.sort_by_key(|event| event.created_at.as_secs());
    let Some(event) = events.into_iter().next() else {
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
            failed_relays: relay_failures(failed_relays),
            reason_code: Some("validation_receipt_not_found".to_owned()),
            reason: Some(format!(
                "validation receipt event `{receipt_event_id}` was not found on configured relays"
            )),
            actions: Vec::new(),
        };
    };
    inspected_event_view(
        config,
        success_state,
        intent,
        event,
        target_relays,
        connected_relays,
        failed_relays,
    )
}

fn inspected_event_view(
    config: &RuntimeConfig,
    success_state: &str,
    intent: ValidationReceiptCommandIntent,
    event: RadrootsNostrEvent,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<DirectRelayFailure>,
) -> ValidationReceiptInspectionView {
    let converted = radroots_event_from_nostr(&event);
    match verify_validation_receipt_event(
        &converted,
        RadrootsValidationReceiptExpectedBinding::default(),
    ) {
        Ok(verified) => {
            let event_id = converted.id.clone();
            let order_id = verified.tags.order_id.clone();
            let proof_verification =
                proof_verification_view(config, &event_id, &verified.receipt, &verified.tags);
            let reason_code =
                (!failed_relays.is_empty()).then_some("relay_fetch_partial".to_owned());
            let accepted = match intent {
                ValidationReceiptCommandIntent::Inspect => {
                    !proof_state_is_invalid(proof_verification.state.as_str())
                }
                ValidationReceiptCommandIntent::Verify => {
                    proof_state_is_verification_success(proof_verification.state.as_str())
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
                    receipt: Some(verified.receipt),
                    receipt_tags: Some(tags_view(&verified.tags)),
                    event: Some(event_view(converted)),
                    target_relays,
                    connected_relays,
                    failed_relays: relay_failures(failed_relays),
                    reason_code: proof_verification.reason_code.clone(),
                    reason: proof_verification.reason.clone(),
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
                receipt: Some(verified.receipt),
                receipt_tags: Some(tags_view(&verified.tags)),
                event: Some(event_view(converted)),
                target_relays,
                connected_relays,
                failed_relays: relay_failures(failed_relays),
                reason_code,
                reason: None,
                actions: Vec::new(),
            }
        }
        Err(error) => {
            let reason_code = validation_receipt_invalid_reason_code(&error);
            let proof_verification = invalid_proof_verification_view(&error);
            ValidationReceiptInspectionView {
                state: "invalid".to_owned(),
                resource: Some(validation_receipt_resource(&converted.id)),
                receipt_event_id: Some(converted.id.clone()),
                order_id: None,
                validation_state: "invalid".to_owned(),
                proof_verification,
                receipt: None,
                receipt_tags: None,
                event: Some(event_view(converted)),
                target_relays,
                connected_relays,
                failed_relays: relay_failures(failed_relays),
                reason_code: Some(reason_code.to_owned()),
                reason: Some(error.to_string()),
                actions: Vec::new(),
            }
        }
    }
}

fn list_from_fetch_receipt(
    config: &RuntimeConfig,
    order_id: &str,
    fetch_receipt: DirectRelayFetchReceipt,
) -> ValidationReceiptListView {
    let DirectRelayFetchReceipt {
        target_relays,
        connected_relays,
        failed_relays,
        mut events,
    } = fetch_receipt;
    events.sort_by(|left, right| {
        left.created_at
            .as_secs()
            .cmp(&right.created_at.as_secs())
            .then_with(|| left.id.to_hex().cmp(&right.id.to_hex()))
    });
    let mut verified_receipts = Vec::new();
    let mut invalid_receipts = Vec::new();

    for event in events {
        let converted = radroots_event_from_nostr(&event);
        match verify_validation_receipt_event(
            &converted,
            RadrootsValidationReceiptExpectedBinding {
                order_id: Some(order_id),
                ..RadrootsValidationReceiptExpectedBinding::default()
            },
        ) {
            Ok(verified) => {
                verified_receipts.push((converted, verified.receipt, verified.tags));
            }
            Err(error) => {
                let reason_code = validation_receipt_invalid_reason_code(&error);
                invalid_receipts.push(ValidationReceiptInvalidCandidateView {
                    receipt_event_id: converted.id,
                    kind: converted.kind,
                    reason_code: reason_code.to_owned(),
                    reason: error.to_string(),
                    proof_verification: invalid_proof_verification_view(&error),
                });
            }
        }
    }

    let evidence_bindings = verified_receipts
        .iter()
        .map(|(event, receipt, tags)| WorkerEvidenceReceiptBinding {
            receipt_event_id: event.id.as_str(),
            receipt,
            tags,
        })
        .collect::<Vec<_>>();
    let mut worker_evidence = worker_evidence_for_receipts(config, &evidence_bindings);
    let mut receipts = Vec::new();
    for (event, receipt, tags) in verified_receipts {
        let proof_verification = proof_verification_view_for_receipt(
            &receipt,
            worker_evidence
                .remove(event.id.as_str())
                .unwrap_or_default(),
        );
        if proof_state_is_invalid(proof_verification.state.as_str()) {
            invalid_receipts.push(ValidationReceiptInvalidCandidateView {
                receipt_event_id: event.id,
                kind: event.kind,
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
            receipts.push(summary_view(&event, &receipt, &tags, &proof_verification));
        }
    }

    let failed_relays = relay_failures(failed_relays);
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
        order_id: order_id.to_owned(),
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
        actions: Vec::new(),
    }
}

fn inspection_fetch_error_view(
    receipt_event_id: &str,
    error: DirectRelayFetchError,
) -> ValidationReceiptInspectionView {
    let (state, reason_code, reason, target_relays, connected_relays, failed_relays, actions) =
        fetch_error_parts(error);
    ValidationReceiptInspectionView {
        state,
        resource: Some(validation_receipt_resource(receipt_event_id)),
        receipt_event_id: Some(receipt_event_id.to_owned()),
        order_id: None,
        validation_state: "unverified".to_owned(),
        proof_verification: None,
        receipt: None,
        receipt_tags: None,
        event: None,
        target_relays,
        connected_relays,
        failed_relays,
        reason_code: Some(reason_code),
        reason: Some(reason),
        actions,
    }
}

fn list_fetch_error_view(
    order_id: &str,
    error: DirectRelayFetchError,
) -> ValidationReceiptListView {
    let (state, reason_code, reason, target_relays, connected_relays, failed_relays, actions) =
        fetch_error_parts(error);
    ValidationReceiptListView {
        state,
        order_id: order_id.to_owned(),
        count: 0,
        valid_count: 0,
        invalid_count: 0,
        receipts: Vec::new(),
        invalid_receipts: Vec::new(),
        target_relays,
        connected_relays,
        failed_relays,
        reason_code: Some(reason_code),
        reason: Some(reason),
        actions,
    }
}

fn fetch_error_parts(
    error: DirectRelayFetchError,
) -> (
    String,
    String,
    String,
    Vec<String>,
    Vec<String>,
    Vec<RelayFailureView>,
    Vec<String>,
) {
    match error {
        DirectRelayFetchError::MissingRelays => (
            "unconfigured".to_owned(),
            "relay_unconfigured".to_owned(),
            "validation receipt commands require at least one configured relay".to_owned(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![
                "radroots --relay wss://relay.example.com validation receipt list --order-id <order-id>"
                    .to_owned(),
            ],
        ),
        DirectRelayFetchError::Connect {
            reason,
            target_relays,
            failed_relays,
        } => (
            "network_unavailable".to_owned(),
            "relay_fetch_failed".to_owned(),
            reason,
            target_relays,
            Vec::new(),
            relay_failures(failed_relays),
            Vec::new(),
        ),
        DirectRelayFetchError::RelayConfig { relay, source } => (
            "network_unavailable".to_owned(),
            "relay_config_failed".to_owned(),
            format!("failed to configure relay `{relay}` for validation receipt fetch: {source}"),
            vec![relay.clone()],
            Vec::new(),
            vec![RelayFailureView {
                relay,
                reason: source.to_string(),
            }],
            Vec::new(),
        ),
        DirectRelayFetchError::Fetch(source) => (
            "network_unavailable".to_owned(),
            "relay_fetch_failed".to_owned(),
            source.to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        DirectRelayFetchError::Runtime(reason) => (
            "network_unavailable".to_owned(),
            "relay_fetch_runtime_failed".to_owned(),
            reason,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
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

fn tags_view(tags: &RadrootsValidationReceiptTags) -> ValidationReceiptTagsView {
    ValidationReceiptTagsView {
        order_id: tags.order_id.clone(),
        event_set_root: tags.event_set_root.clone(),
        listing_event_id: tags.listing_event_id.clone(),
        reducer_output_root: tags.reducer_output_root.clone(),
        public_values_hash: tags.public_values_hash.clone(),
        proof_system: tags.proof_system.as_str().to_owned(),
        receipt_type: receipt_type_label(tags.receipt_type).to_owned(),
        root_event_id: tags.root_event_id.clone(),
        target_event_id: tags.target_event_id.clone(),
    }
}

fn proof_verification_view(
    config: &RuntimeConfig,
    receipt_event_id: &str,
    receipt: &RadrootsTradeValidationReceipt,
    tags: &RadrootsValidationReceiptTags,
) -> ValidationReceiptProofVerificationView {
    let worker_evidence = worker_evidence_for_receipt(config, receipt_event_id, receipt, tags);
    proof_verification_view_for_receipt(receipt, worker_evidence)
}

fn proof_verification_view_for_receipt(
    receipt: &RadrootsTradeValidationReceipt,
    worker_evidence: ValidationReceiptWorkerEvidenceSelection,
) -> ValidationReceiptProofVerificationView {
    let proof = &receipt.proof;
    let cryptographic_proof_required = proof.system != RadrootsValidationReceiptProofSystem::None;
    if proof.system == RadrootsValidationReceiptProofSystem::None {
        let state = if worker_evidence
            .trusted
            .as_ref()
            .is_some_and(|evidence| evidence.sp1_execute_checked)
        {
            "sp1_execute_checked"
        } else {
            "deterministic_receipt_verified"
        };
        return ValidationReceiptProofVerificationView {
            state: state.to_owned(),
            verifier: "radroots_cli_validation_receipt_v1".to_owned(),
            proof_system: proof.system.as_str().to_owned(),
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
            reason_code: None,
            reason: None,
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
        Ok(()) => ValidationReceiptProofVerificationView {
            state: "sp1_inline_proof_verified".to_owned(),
            verifier: "radroots_cli_validation_receipt_v1".to_owned(),
            proof_system: proof.system.as_str().to_owned(),
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
            reason_code: None,
            reason: None,
        },
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

fn validation_receipt_invalid_reason_code(error: &RadrootsValidationReceiptError) -> &'static str {
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

fn invalid_proof_verification_view(
    error: &RadrootsValidationReceiptError,
) -> Option<ValidationReceiptProofVerificationView> {
    let reason_code = validation_receipt_invalid_reason_code(error);
    let (state, public_values_hash_binding, proof_metadata_binding) = match error {
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.material")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.material_missing") => (
            "sp1_proof_material_missing",
            "unverified",
            "missing_proof_material",
        ),
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.material_conflict") => (
            "sp1_proof_material_conflict",
            "unverified",
            "conflicting_proof_material",
        ),
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.inline_proof_base64")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.proof_reference")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.mode")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.program_hash")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.verifying_key_hash")
        | RadrootsValidationReceiptError::InvalidProofMetadata("proof.system")
        | RadrootsValidationReceiptError::TagMismatch("proof_system")
        | RadrootsValidationReceiptError::ExpectedBindingMismatch("proof_system") => {
            ("sp1_proof_invalid", "unverified", "invalid")
        }
        RadrootsValidationReceiptError::TagMismatch("public_values_hash")
        | RadrootsValidationReceiptError::ExpectedBindingMismatch("public_values_hash") => (
            "sp1_public_values_mismatch",
            "mismatch",
            "metadata_consistent",
        ),
        RadrootsValidationReceiptError::ExpectedBindingMismatch("program_hash") => {
            ("sp1_program_hash_mismatch", "unverified", "mismatch")
        }
        RadrootsValidationReceiptError::ExpectedBindingMismatch("verifying_key_hash") => {
            ("sp1_verifying_key_hash_mismatch", "unverified", "mismatch")
        }
        _ => return None,
    };

    Some(ValidationReceiptProofVerificationView {
        state: state.to_owned(),
        verifier: "radroots_cli_validation_receipt_v1".to_owned(),
        proof_system: "unknown".to_owned(),
        public_values_hash_binding: public_values_hash_binding.to_owned(),
        proof_metadata_binding: proof_metadata_binding.to_owned(),
        cryptographic_proof_required: true,
        cryptographic_proof_verified: false,
        mode: None,
        program_hash: None,
        verifying_key_hash: None,
        proof_reference: None,
        inline_proof_present: false,
        worker_evidence: None,
        untrusted_worker_evidence: None,
        reason_code: Some(reason_code.to_owned()),
        reason: Some(error.to_string()),
    })
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
        _ => MappedSp1ProofError {
            state: "sp1_proof_invalid",
            public_values_hash_binding: "unverified",
            proof_metadata_binding: "invalid",
            reason_code: "sp1_proof_invalid",
        },
    }
}

#[cfg(feature = "sp1-verify")]
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

#[cfg(not(feature = "sp1-verify"))]
fn verify_inline_sp1_receipt(
    _receipt: &RadrootsTradeValidationReceipt,
) -> Result<(), RadrootsSp1TradeHostError> {
    Err(RadrootsSp1TradeHostError::Sp1ProofVerificationFailed(
        "SP1 inline proof verification is disabled for this build".to_owned(),
    ))
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
    )
}

fn proof_state_is_verification_success(state: &str) -> bool {
    matches!(
        state,
        "deterministic_receipt_verified" | "sp1_execute_checked" | "sp1_inline_proof_verified"
    )
}

fn validation_receipt_worker_result_filter(
    receipt_event_ids: Vec<String>,
) -> Result<RadrootsNostrFilter, String> {
    let filter = RadrootsNostrFilter::new().kind(RadrootsNostrKind::Custom(
        KIND_WORKER_TRADE_TRANSITION_PROOF_RES as u16,
    ));
    radroots_nostr_filter_tag(filter, "e", receipt_event_ids)
        .map_err(|error| format!("build validation receipt worker result filter: {error}"))
}

struct WorkerEvidenceReceiptBinding<'a> {
    receipt_event_id: &'a str,
    receipt: &'a RadrootsTradeValidationReceipt,
    tags: &'a RadrootsValidationReceiptTags,
}

fn worker_evidence_for_receipt(
    config: &RuntimeConfig,
    receipt_event_id: &str,
    receipt: &RadrootsTradeValidationReceipt,
    tags: &RadrootsValidationReceiptTags,
) -> ValidationReceiptWorkerEvidenceSelection {
    let bindings = [WorkerEvidenceReceiptBinding {
        receipt_event_id,
        receipt,
        tags,
    }];
    worker_evidence_for_receipts(config, &bindings)
        .remove(receipt_event_id)
        .unwrap_or_default()
}

fn worker_evidence_for_receipts(
    config: &RuntimeConfig,
    bindings: &[WorkerEvidenceReceiptBinding<'_>],
) -> BTreeMap<String, ValidationReceiptWorkerEvidenceSelection> {
    if config.rhi.trusted_worker_pubkeys.is_empty() || bindings.is_empty() {
        return BTreeMap::new();
    }
    let receipt_event_ids = bindings
        .iter()
        .map(|binding| binding.receipt_event_id.to_owned())
        .collect::<Vec<_>>();
    let filter = match validation_receipt_worker_result_filter(receipt_event_ids) {
        Ok(filter) => filter,
        Err(_) => return BTreeMap::new(),
    };
    let fetch_receipt = match fetch_events_from_relays(&config.relay.urls, filter) {
        Ok(fetch_receipt) => fetch_receipt,
        Err(_) => return BTreeMap::new(),
    };
    let binding_by_receipt_id = bindings
        .iter()
        .map(|binding| (binding.receipt_event_id, binding))
        .collect::<BTreeMap<_, _>>();
    let trusted_pubkeys = config
        .rhi
        .trusted_worker_pubkeys
        .iter()
        .map(|pubkey| pubkey.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut by_receipt =
        BTreeMap::<String, Vec<(u64, String, bool, ValidationReceiptWorkerEvidenceView)>>::new();

    for event in fetch_receipt.events {
        let payload =
            match serde_json::from_str::<RawValidationReceiptWorkerResultPayload>(&event.content) {
                Ok(payload) => payload,
                Err(_) => continue,
            };
        let Some(binding) = binding_by_receipt_id.get(payload.receipt_event_id.as_str()) else {
            continue;
        };
        let converted = radroots_event_from_nostr(&event);
        let author = converted.author.to_ascii_lowercase();
        let trusted_author = trusted_pubkeys.contains(author.as_str());
        let typed_payload = payload.typed();
        let bound = typed_payload
            .as_ref()
            .is_some_and(|payload| worker_payload_binds_receipt(payload, binding));
        let trusted = trusted_author && bound;
        let receipt_event_id = payload.receipt_event_id.clone();
        let result_event_id = event.id.to_hex();
        let view = ValidationReceiptWorkerEvidenceView {
            result_event_id: result_event_id.clone(),
            author,
            status: payload.status,
            prover_backend: payload.prover_backend,
            proof_mode: payload.proof_mode,
            proof_system: payload.proof_system,
            proof_generated: payload.proof_generated,
            sp1_execute_checked: payload.sp1_execute_checked,
            sp1_execute_public_values_hash: payload.sp1_execute_public_values_hash,
            cryptographic_proof_verified: payload.cryptographic_proof_verified,
            public_values_hash: payload.public_values_hash,
        };
        by_receipt.entry(receipt_event_id).or_default().push((
            event.created_at.as_secs(),
            result_event_id,
            trusted,
            view,
        ));
    }

    by_receipt
        .into_iter()
        .map(|(receipt_event_id, mut candidates)| {
            candidates.sort_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| left.1.cmp(&right.1))
                    .then_with(|| left.2.cmp(&right.2))
            });
            let mut selection = ValidationReceiptWorkerEvidenceSelection::default();
            for (_, _, trusted, view) in candidates.into_iter().rev() {
                if trusted && selection.trusted.is_none() {
                    selection.trusted = Some(view);
                } else if !trusted && selection.untrusted.is_none() {
                    selection.untrusted = Some(view);
                }
                if selection.trusted.is_some() && selection.untrusted.is_some() {
                    break;
                }
            }
            (receipt_event_id, selection)
        })
        .collect()
}

fn worker_payload_binds_receipt(
    payload: &RadrootsSp1TradeWorkerResultPayload,
    binding: &WorkerEvidenceReceiptBinding<'_>,
) -> bool {
    let receipt = binding.receipt;
    let tags = binding.tags;
    payload.status == RadrootsSp1TradeWorkerResultStatus::Succeeded
        && payload.worker_role == Some(RadrootsSp1TradeWorkerRole::NonAuthoritativeProver)
        && payload.receipt_kind == Some(KIND_TRADE_VALIDATION_RECEIPT)
        && payload.receipt_event_id == binding.receipt_event_id
        && payload.order_id.as_deref() == Some(tags.order_id.as_str())
        && payload.listing_event_id.as_deref() == Some(tags.listing_event_id.as_str())
        && payload.event_set_root.as_deref() == Some(tags.event_set_root.as_str())
        && payload.reducer_output_root.as_deref() == Some(tags.reducer_output_root.as_str())
        && payload.request_event_id.as_deref() == Some(tags.root_event_id.as_str())
        && payload.decision_event_id.as_deref() == Some(tags.target_event_id.as_str())
        && payload.public_values_hash == receipt.public_values_hash
        && payload.proof_system == receipt.proof.system
        && payload.proof_mode.mode_label().unwrap_or("none")
            == receipt.proof.mode.as_deref().unwrap_or("none")
        && payload.proof_generated
            == (receipt.proof.system != RadrootsValidationReceiptProofSystem::None)
        && payload.cryptographic_proof_verified == payload.proof_generated
        && payload.sp1_execute_checked
        && payload.sp1_execute_public_values_hash.as_deref()
            == Some(receipt.public_values_hash.as_str())
}

fn summary_view(
    event: &radroots_events::RadrootsNostrEvent,
    receipt: &RadrootsTradeValidationReceipt,
    tags: &RadrootsValidationReceiptTags,
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

fn relay_failures(failures: Vec<DirectRelayFailure>) -> Vec<RelayFailureView> {
    failures
        .into_iter()
        .map(|failure| RelayFailureView {
            relay: failure.relay,
            reason: failure.reason,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        RawValidationReceiptWorkerResultPayload, ValidationReceiptWorkerEvidenceSelection,
        ValidationReceiptWorkerEvidenceView, WorkerEvidenceReceiptBinding,
        proof_verification_view_for_receipt, validation_receipt_invalid_reason_code,
        worker_payload_binds_receipt,
    };
    use radroots_events::kinds::KIND_TRADE_VALIDATION_RECEIPT;
    use radroots_trade::validation_receipt::{
        RadrootsTradeValidationReceipt, RadrootsValidationReceiptError,
        RadrootsValidationReceiptProof, RadrootsValidationReceiptProofSystem,
        RadrootsValidationReceiptResult, RadrootsValidationReceiptStatement,
        RadrootsValidationReceiptTags, RadrootsValidationReceiptType, VALIDATION_RECEIPT_DOMAIN,
        VALIDATION_RECEIPT_VERSION,
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

    fn receipt_tags() -> RadrootsValidationReceiptTags {
        RadrootsValidationReceiptTags {
            event_set_root: "0x2222222222222222222222222222222222222222222222222222222222222222"
                .to_owned(),
            listing_event_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            order_id: "order-1".to_owned(),
            proof_system: RadrootsValidationReceiptProofSystem::None,
            public_values_hash:
                "0x5555555555555555555555555555555555555555555555555555555555555555".to_owned(),
            receipt_type: RadrootsValidationReceiptType::TradeTransition,
            reducer_output_root:
                "0x3333333333333333333333333333333333333333333333333333333333333333".to_owned(),
            root_event_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_owned(),
            target_event_id: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_owned(),
        }
    }

    fn worker_result_payload(listing_event_id: &str) -> RawValidationReceiptWorkerResultPayload {
        RawValidationReceiptWorkerResultPayload {
            cryptographic_proof_verified: false,
            decision_event_id: Some(
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned(),
            ),
            event_set_root: Some(
                "0x2222222222222222222222222222222222222222222222222222222222222222".to_owned(),
            ),
            listing_event_id: Some(listing_event_id.to_owned()),
            order_id: Some("order-1".to_owned()),
            proof_generated: false,
            proof_mode: "none".to_owned(),
            proof_system: "none".to_owned(),
            public_values_hash:
                "0x5555555555555555555555555555555555555555555555555555555555555555".to_owned(),
            prover_backend: "local_execute".to_owned(),
            receipt_kind: Some(KIND_TRADE_VALIDATION_RECEIPT),
            receipt_event_id: "receipt-1".to_owned(),
            reducer_output_root: Some(
                "0x3333333333333333333333333333333333333333333333333333333333333333".to_owned(),
            ),
            request_event_id: Some(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
            ),
            sp1_execute_checked: true,
            sp1_execute_public_values_hash: Some(
                "0x5555555555555555555555555555555555555555555555555555555555555555".to_owned(),
            ),
            status: "succeeded".to_owned(),
            worker_role: Some("non_authoritative_prover".to_owned()),
        }
    }

    #[test]
    fn worker_evidence_binds_distinct_listing_request_and_decision_ids() {
        let receipt = deterministic_receipt();
        let tags = receipt_tags();
        let binding = WorkerEvidenceReceiptBinding {
            receipt_event_id: "receipt-1",
            receipt: &receipt,
            tags: &tags,
        };

        assert!(worker_payload_binds_receipt(
            &worker_result_payload(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .typed()
            .expect("typed payload"),
            &binding
        ));
    }

    #[test]
    fn worker_evidence_rejects_listing_id_mismatch() {
        let receipt = deterministic_receipt();
        let tags = receipt_tags();
        let binding = WorkerEvidenceReceiptBinding {
            receipt_event_id: "receipt-1",
            receipt: &receipt,
            tags: &tags,
        };

        assert!(!worker_payload_binds_receipt(
            &worker_result_payload(
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
            )
            .typed()
            .expect("typed payload"),
            &binding
        ));
    }

    #[test]
    fn worker_evidence_unknown_typed_values_are_not_trusted() {
        let mut payload = worker_result_payload(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        payload.prover_backend = "future_backend".to_owned();
        assert!(payload.typed().is_none());
    }

    #[test]
    fn none_receipts_report_deterministic_verification_without_crypto_claim() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection::default(),
        );

        assert_eq!(view.state, "deterministic_receipt_verified");
        assert!(!view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
    }

    #[test]
    fn none_receipts_surface_advisory_sp1_execute_evidence() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection {
                trusted: Some(ValidationReceiptWorkerEvidenceView {
                    result_event_id: "result-1".to_owned(),
                    author: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_owned(),
                    status: "succeeded".to_owned(),
                    prover_backend: "local_execute".to_owned(),
                    proof_mode: "none".to_owned(),
                    proof_system: "none".to_owned(),
                    proof_generated: false,
                    sp1_execute_checked: true,
                    sp1_execute_public_values_hash: Some(
                        "0x5555555555555555555555555555555555555555555555555555555555555555"
                            .to_owned(),
                    ),
                    cryptographic_proof_verified: false,
                    public_values_hash:
                        "0x5555555555555555555555555555555555555555555555555555555555555555"
                            .to_owned(),
                }),
                untrusted: None,
            },
        );

        assert_eq!(view.state, "sp1_execute_checked");
        assert!(!view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
    }

    #[test]
    fn untrusted_worker_evidence_does_not_upgrade_deterministic_receipts() {
        let view = proof_verification_view_for_receipt(
            &deterministic_receipt(),
            ValidationReceiptWorkerEvidenceSelection {
                trusted: None,
                untrusted: Some(ValidationReceiptWorkerEvidenceView {
                    result_event_id: "result-1".to_owned(),
                    author: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_owned(),
                    status: "succeeded".to_owned(),
                    prover_backend: "local_execute".to_owned(),
                    proof_mode: "none".to_owned(),
                    proof_system: "none".to_owned(),
                    proof_generated: false,
                    sp1_execute_checked: true,
                    sp1_execute_public_values_hash: Some(
                        "0x5555555555555555555555555555555555555555555555555555555555555555"
                            .to_owned(),
                    ),
                    cryptographic_proof_verified: false,
                    public_values_hash:
                        "0x5555555555555555555555555555555555555555555555555555555555555555"
                            .to_owned(),
                }),
            },
        );

        assert_eq!(view.state, "deterministic_receipt_verified");
        assert!(view.worker_evidence.is_none());
        assert!(view.untrusted_worker_evidence.is_some());
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
