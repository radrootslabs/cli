use radroots_events::kinds::KIND_TRADE_VALIDATION_RECEIPT;
use radroots_nostr::prelude::{
    RadrootsNostrEvent, RadrootsNostrEventId, RadrootsNostrFilter, RadrootsNostrKind,
    radroots_event_from_nostr, radroots_nostr_filter_tag,
};
use radroots_trade::validation_receipt::{
    RadrootsTradeValidationReceipt, RadrootsValidationReceiptExpectedBinding,
    RadrootsValidationReceiptProof, RadrootsValidationReceiptProofSystem,
    RadrootsValidationReceiptResult, RadrootsValidationReceiptTags, RadrootsValidationReceiptType,
    verify_validation_receipt_event,
};
use serde::Serialize;

use crate::domain::runtime::{CommandDisposition, RelayFailureView};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayFetchError, DirectRelayFetchReceipt, fetch_events_from_relays,
};

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
    pub reason: String,
}

pub fn get(
    config: &RuntimeConfig,
    args: &ValidationReceiptEventArgs,
) -> ValidationReceiptInspectionView {
    inspect_event(config, &args.receipt_event_id, "valid")
}

pub fn verify(
    config: &RuntimeConfig,
    args: &ValidationReceiptEventArgs,
) -> ValidationReceiptInspectionView {
    inspect_event(config, &args.receipt_event_id, "verified")
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
    list_from_fetch_receipt(order_id, receipt)
}

fn inspect_event(
    config: &RuntimeConfig,
    receipt_event_id: &str,
    success_state: &str,
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
    inspection_from_fetch_receipt(receipt_event_id, success_state, receipt)
}

fn validation_receipt_order_filter(order_id: &str) -> Result<RadrootsNostrFilter, String> {
    let filter = RadrootsNostrFilter::new().kind(RadrootsNostrKind::Custom(
        KIND_TRADE_VALIDATION_RECEIPT as u16,
    ));
    radroots_nostr_filter_tag(filter, "d", vec![order_id.to_owned()])
        .map_err(|error| format!("build validation receipt order filter: {error}"))
}

fn inspection_from_fetch_receipt(
    receipt_event_id: &str,
    success_state: &str,
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
        success_state,
        event,
        target_relays,
        connected_relays,
        failed_relays,
    )
}

fn inspected_event_view(
    success_state: &str,
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
            let proof_verification = proof_verification_view(&verified.receipt.proof);
            let reason_code =
                (!failed_relays.is_empty()).then_some("relay_fetch_partial".to_owned());
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
            ValidationReceiptInspectionView {
                state: "invalid".to_owned(),
                resource: Some(validation_receipt_resource(&converted.id)),
                receipt_event_id: Some(converted.id.clone()),
                order_id: None,
                validation_state: "invalid".to_owned(),
                proof_verification: None,
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
    let mut receipts = Vec::new();
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
                receipts.push(summary_view(&converted, &verified.receipt, &verified.tags))
            }
            Err(error) => invalid_receipts.push(ValidationReceiptInvalidCandidateView {
                receipt_event_id: converted.id,
                kind: converted.kind,
                reason: error.to_string(),
            }),
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
        count: receipts.len(),
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
        reducer_output_root: tags.reducer_output_root.clone(),
        public_values_hash: tags.public_values_hash.clone(),
        proof_system: tags.proof_system.as_str().to_owned(),
        receipt_type: receipt_type_label(tags.receipt_type).to_owned(),
        root_event_id: tags.root_event_id.clone(),
        target_event_id: tags.target_event_id.clone(),
    }
}

fn proof_verification_view(
    proof: &RadrootsValidationReceiptProof,
) -> ValidationReceiptProofVerificationView {
    let cryptographic_proof_required = proof.system != RadrootsValidationReceiptProofSystem::None;
    let proof_material_present =
        proof.inline_proof_base64.is_some() || proof.proof_reference.is_some();
    let state = match proof.system {
        RadrootsValidationReceiptProofSystem::None => "deterministic_receipt_verified",
        _ if proof_material_present => "sp1_metadata_consistent",
        _ => "sp1_proof_material_missing",
    };
    let proof_metadata_binding = match proof.system {
        RadrootsValidationReceiptProofSystem::None => "not_required",
        _ if proof_material_present => "metadata_consistent",
        _ => "missing_proof_material",
    };
    ValidationReceiptProofVerificationView {
        state: state.to_owned(),
        verifier: "radroots_cli_validation_receipt_v1".to_owned(),
        proof_system: proof.system.as_str().to_owned(),
        public_values_hash_binding: "verified".to_owned(),
        proof_metadata_binding: proof_metadata_binding.to_owned(),
        cryptographic_proof_required,
        cryptographic_proof_verified: false,
        mode: proof.mode.clone(),
        program_hash: proof.program_hash.clone(),
        verifying_key_hash: proof.verifying_key_hash.clone(),
        proof_reference: proof.proof_reference.clone(),
        inline_proof_present: proof.inline_proof_base64.is_some(),
    }
}

fn validation_receipt_invalid_reason_code(
    error: &radroots_trade::validation_receipt::RadrootsValidationReceiptError,
) -> &'static str {
    use radroots_trade::validation_receipt::RadrootsValidationReceiptError;

    match error {
        RadrootsValidationReceiptError::InvalidProofMetadata("proof.material") => {
            "sp1_proof_material_missing"
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

fn summary_view(
    event: &radroots_events::RadrootsNostrEvent,
    receipt: &RadrootsTradeValidationReceipt,
    tags: &RadrootsValidationReceiptTags,
) -> ValidationReceiptSummaryView {
    let proof_verification = proof_verification_view(&receipt.proof);
    ValidationReceiptSummaryView {
        resource: validation_receipt_resource(&event.id),
        receipt_event_id: event.id.clone(),
        order_id: tags.order_id.clone(),
        author: event.author.clone(),
        created_at: event.created_at,
        receipt_type: receipt_type_label(receipt.receipt_type).to_owned(),
        result: receipt_result_label(receipt.result).to_owned(),
        proof_system: receipt.proof.system.as_str().to_owned(),
        proof_verification_state: proof_verification.state,
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
        RadrootsValidationReceiptProof, RadrootsValidationReceiptProofSystem,
        proof_verification_view, validation_receipt_invalid_reason_code,
    };
    use radroots_trade::validation_receipt::RadrootsValidationReceiptError;

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

    #[test]
    fn none_receipts_report_deterministic_verification_without_crypto_claim() {
        let view = proof_verification_view(&RadrootsValidationReceiptProof {
            inline_proof_base64: None,
            mode: None,
            program_hash: None,
            proof_reference: None,
            system: RadrootsValidationReceiptProofSystem::None,
            verifying_key_hash: None,
        });

        assert_eq!(view.state, "deterministic_receipt_verified");
        assert!(!view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
    }

    #[test]
    fn sp1_receipts_report_metadata_consistency_without_crypto_claim() {
        let view = proof_verification_view(&sp1_proof_with_material());

        assert_eq!(view.state, "sp1_metadata_consistent");
        assert!(view.cryptographic_proof_required);
        assert!(!view.cryptographic_proof_verified);
        assert_eq!(view.proof_metadata_binding, "metadata_consistent");
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
