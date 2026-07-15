use radroots_event::ids::RadrootsEventId;
#[cfg(test)]
use radroots_event::ids::RadrootsOrderId;
use radroots_sdk::{
    SdkTradeStatusIssue, TradeStatusAmbiguityCandidate, TradeStatusEligibility,
    TradeStatusEvidenceSummary, TradeStatusKind, TradeStatusNextActionKind, TradeStatusReceipt,
    TradeStatusRequest, TradeValidationTrustDecision,
};
use radroots_trade::identity::RadrootsTradeLocator;

use crate::view::runtime::{
    OrderIssueView, OrderStatusAmbiguityCandidateView, OrderStatusEligibilityView,
    OrderStatusEvidenceSummaryView, OrderStatusLifecycleCancellationView, OrderStatusLifecycleView,
    OrderStatusSdkReceiptView, OrderStatusValidationTrustView, OrderStatusView,
    OrderTradeLocatorView,
};

use super::{ORDER_ACTOR_CONTEXT_SDK_LOCAL, ORDER_STATUS_SDK_SOURCE};

pub(super) fn sdk_order_status_view(receipt: TradeStatusReceipt) -> OrderStatusView {
    let state = sdk_order_status_state(receipt.status).to_owned();
    let reducer_issues = receipt
        .issues
        .iter()
        .map(sdk_order_status_issue_view)
        .collect::<Vec<_>>();
    let reason = sdk_order_status_reason(receipt.status, receipt.order_id.as_str());
    let lifecycle = sdk_order_status_lifecycle_view(&receipt, reducer_issues.as_slice());
    let sdk_receipt = Some(sdk_order_status_receipt_view(&receipt));
    let ambiguity_candidates = receipt
        .ambiguity_candidates
        .iter()
        .map(sdk_status_ambiguity_candidate_view)
        .collect::<Vec<_>>();
    let actions = ambiguity_candidates
        .iter()
        .map(|candidate| candidate.status_command.clone())
        .collect::<Vec<_>>();

    OrderStatusView {
        state,
        source: ORDER_STATUS_SDK_SOURCE.to_owned(),
        order_id: receipt.order_id.to_string(),
        locator: sdk_trade_locator_view(&receipt.locator),
        actor_context_source: ORDER_ACTOR_CONTEXT_SDK_LOCAL.to_owned(),
        request_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
        decision_event_id: sdk_event_id_string(receipt.decision_event_id.as_ref()),
        agreement_event_id: sdk_order_status_agreement_event_id(&receipt),
        listing_event_id: None,
        listing_addr: receipt.listing_addr.as_ref().map(ToString::to_string),
        buyer_pubkey: receipt.buyer_pubkey.as_ref().map(ToString::to_string),
        seller_pubkey: receipt.seller_pubkey.as_ref().map(ToString::to_string),
        economics: receipt.economics.clone(),
        last_event_id: sdk_event_id_string(receipt.last_event_id.as_ref()),
        inventory: None,
        lifecycle: Some(lifecycle),
        sdk_receipt,
        ambiguity_candidates,
        reducer_issues,
        target_transport_endpoints: Vec::new(),
        attempted_transport_endpoints: Vec::new(),
        failed_transport_targets: Vec::new(),
        fetched_count: 0,
        decoded_count: receipt.event_count,
        skipped_count: 0,
        reason,
        actions,
    }
}

fn sdk_trade_locator_view(locator: &RadrootsTradeLocator) -> OrderTradeLocatorView {
    OrderTradeLocatorView {
        trade_id: locator.trade_id.as_str().to_owned(),
        root_event_id: locator.root_event_id.as_ref().map(ToString::to_string),
        listing_addr: locator.listing_addr.as_ref().map(ToString::to_string),
        buyer_pubkey: locator.buyer_pubkey.as_ref().map(ToString::to_string),
        seller_pubkey: locator.seller_pubkey.as_ref().map(ToString::to_string),
    }
}

fn sdk_status_ambiguity_candidate_view(
    candidate: &TradeStatusAmbiguityCandidate,
) -> OrderStatusAmbiguityCandidateView {
    let status_selector = TradeStatusRequest::locator_selector(&candidate.locator);
    OrderStatusAmbiguityCandidateView {
        locator: sdk_trade_locator_view(&candidate.locator),
        status_command: format!("radroots trade status get {status_selector}"),
        status_selector,
    }
}

fn sdk_order_status_receipt_view(receipt: &TradeStatusReceipt) -> OrderStatusSdkReceiptView {
    OrderStatusSdkReceiptView {
        next_action: sdk_status_next_action(receipt.next_action).to_owned(),
        evidence: sdk_status_evidence_view(&receipt.evidence),
        eligibility: sdk_status_eligibility_view(&receipt.eligibility),
        validation_trust: receipt
            .validation_trust
            .as_ref()
            .map(sdk_status_validation_trust_view),
    }
}

fn sdk_status_evidence_view(
    evidence: &TradeStatusEvidenceSummary,
) -> OrderStatusEvidenceSummaryView {
    OrderStatusEvidenceSummaryView {
        event_count: evidence.event_count,
        limit_applied: evidence.limit_applied,
        has_request: evidence.has_request,
        has_decision: evidence.has_decision,
        has_agreement: evidence.has_agreement,
        has_validation_receipt: evidence.has_validation_receipt,
        has_cancellation: evidence.has_cancellation,
        has_issues: evidence.has_issues,
    }
}

fn sdk_status_validation_trust_view(
    decision: &TradeValidationTrustDecision,
) -> OrderStatusValidationTrustView {
    OrderStatusValidationTrustView {
        state: decision.state.as_str().to_owned(),
        validator_count: decision.validator_count,
        validator_set_addr: decision.validator_set_addr.clone(),
        validator_set_event_id: decision.validator_set_event_id.clone(),
        require_cryptographic_proof: decision.require_cryptographic_proof,
        receipt_event_id: decision.receipt_event_id.as_ref().map(ToString::to_string),
        receipt_author: decision.receipt_author.as_ref().map(ToString::to_string),
        result_event_id: decision.result_event_id.as_ref().map(ToString::to_string),
        result_author: decision.result_author.as_ref().map(ToString::to_string),
        proof_system: decision.proof_system.clone(),
        validation_authority: decision
            .validation_authority
            .map(|authority| authority.as_str().to_owned()),
        commitment_confidence: decision
            .commitment_confidence
            .map(|confidence| confidence.as_str().to_owned()),
        cryptographic_proof_required: decision.cryptographic_proof_required,
        cryptographic_proof_verified: decision.cryptographic_proof_verified,
        production_committed: decision.production_committed,
        reason_code: decision.reason_code.clone(),
        reason: decision.reason.clone(),
    }
}

fn sdk_status_eligibility_view(eligibility: &TradeStatusEligibility) -> OrderStatusEligibilityView {
    OrderStatusEligibilityView {
        can_decide: eligibility.can_decide,
        can_cancel: eligibility.can_cancel,
    }
}

fn sdk_status_next_action(kind: TradeStatusNextActionKind) -> &'static str {
    match kind {
        TradeStatusNextActionKind::NoLocalOrder => "no_local_order",
        TradeStatusNextActionKind::InspectEvidenceIssues => "inspect_evidence_issues",
        TradeStatusNextActionKind::AwaitSellerDecision => "await_seller_decision",
        TradeStatusNextActionKind::AwaitValidation => "await_validation",
        TradeStatusNextActionKind::Terminal => "terminal",
        _ => "unknown",
    }
}

fn sdk_order_status_state(status: TradeStatusKind) -> &'static str {
    match status {
        TradeStatusKind::Missing => "missing",
        TradeStatusKind::Ambiguous => "ambiguous",
        TradeStatusKind::Requested => "requested",
        TradeStatusKind::AgreedPendingValidation => "pending_validation",
        TradeStatusKind::Committed => "committed",
        TradeStatusKind::Declined => "declined",
        TradeStatusKind::Cancelled => "cancelled",
        TradeStatusKind::ValidationExpired => "validation_expired",
        TradeStatusKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_order_status_reason(status: TradeStatusKind, order_id: &str) -> Option<String> {
    match status {
        TradeStatusKind::Missing => Some(format!("no local SDK trade events matched `{order_id}`")),
        TradeStatusKind::Ambiguous => Some(format!(
            "local SDK trade events for `{order_id}` matched multiple roots"
        )),
        TradeStatusKind::Invalid => Some(format!(
            "local SDK trade events for `{order_id}` failed reducer validation"
        )),
        _ => None,
    }
}

fn sdk_order_status_agreement_event_id(receipt: &TradeStatusReceipt) -> Option<String> {
    sdk_event_id_string(receipt.agreement_event_id.as_ref())
}

fn sdk_order_status_lifecycle_view(
    receipt: &TradeStatusReceipt,
    issues: &[OrderIssueView],
) -> OrderStatusLifecycleView {
    let cancellation = receipt.cancellation_event_id.as_ref().map(|event_id| {
        OrderStatusLifecycleCancellationView {
            event_id: event_id.to_string(),
            root_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
            prev_event_id: sdk_event_id_string(receipt.decision_event_id.as_ref()),
            reason: None,
        }
    });
    OrderStatusLifecycleView {
        phase: sdk_order_status_lifecycle_phase(receipt).to_owned(),
        terminal: receipt.lifecycle_terminal,
        event_id: sdk_event_id_string(receipt.last_event_id.as_ref()),
        root_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
        prev_event_id: None,
        cancellation,
        issues: issues.to_vec(),
    }
}

fn sdk_order_status_lifecycle_phase(receipt: &TradeStatusReceipt) -> &'static str {
    match receipt.status {
        TradeStatusKind::Missing => "missing",
        TradeStatusKind::Ambiguous => "ambiguous",
        TradeStatusKind::Requested => "requested",
        TradeStatusKind::AgreedPendingValidation => "pending_validation",
        TradeStatusKind::Committed => "committed",
        TradeStatusKind::Declined => "declined",
        TradeStatusKind::Cancelled => "cancelled",
        TradeStatusKind::ValidationExpired => "validation_expired",
        TradeStatusKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_order_status_issue_view(issue: &SdkTradeStatusIssue) -> OrderIssueView {
    let code = issue.code();
    OrderIssueView {
        code: code.clone(),
        field: "sdk_order_status".to_owned(),
        message: format!("SDK trade status reported `{code}`"),
        event_ids: issue
            .event_ids
            .iter()
            .map(RadrootsEventId::to_string)
            .collect(),
    }
}

fn sdk_event_id_string(event_id: Option<&RadrootsEventId>) -> Option<String> {
    event_id.map(RadrootsEventId::to_string)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sdk_order_status_view_exposes_ambiguity_candidates() {
        let order_id = RadrootsOrderId::parse("order-1").expect("order id");
        let root_event_id = RadrootsEventId::parse(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .expect("root event id");
        let receipt = TradeStatusReceipt {
            locator: RadrootsTradeLocator::from_order_id(order_id.clone()),
            order_id,
            root_event_id: None,
            ambiguity_candidates: vec![TradeStatusAmbiguityCandidate {
                locator: RadrootsTradeLocator::from_order_id(
                    RadrootsOrderId::parse("order-1").expect("candidate order id"),
                )
                .with_root_event_id(root_event_id.clone()),
            }],
            source: radroots_sdk::SdkTradeStatusSource::LocalOnly,
            found: false,
            event_count: 2,
            limit_applied: 500,
            status: TradeStatusKind::Ambiguous,
            lifecycle_terminal: false,
            listing_addr: None,
            buyer_pubkey: None,
            seller_pubkey: None,
            economics: None,
            evidence: TradeStatusEvidenceSummary {
                event_count: 2,
                limit_applied: 500,
                has_request: true,
                has_decision: false,
                has_agreement: false,
                has_validation_receipt: false,
                has_cancellation: false,
                has_issues: false,
            },
            validation_trust: None,
            online_evidence: None,
            eligibility: TradeStatusEligibility {
                can_decide: false,
                can_cancel: false,
            },
            next_action: TradeStatusNextActionKind::InspectEvidenceIssues,
            event_ids: vec![root_event_id.clone()],
            request_event_id: None,
            decision_event_id: None,
            agreement_event_id: None,
            rhi_receipt_event_id: None,
            cancellation_event_id: None,
            last_event_id: None,
            issues: Vec::new(),
        };

        let view_json = serde_json::to_value(sdk_order_status_view(receipt)).expect("view json");

        assert_eq!(
            view_json["ambiguity_candidates"][0]["status_selector"],
            json!(format!("order-1@{}", root_event_id.as_str()))
        );
        assert_eq!(
            view_json["actions"][0],
            json!(format!(
                "radroots trade status get order-1@{}",
                root_event_id.as_str()
            ))
        );
    }
}
