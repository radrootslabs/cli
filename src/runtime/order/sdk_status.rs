use radroots_events::ids::RadrootsEventId;
use radroots_sdk::{
    OrderStatusEligibility, OrderStatusEvidenceSummary, OrderStatusKind, OrderStatusNextActionKind,
    OrderStatusReceipt, SdkOrderStatusIssue,
};

use crate::view::runtime::{
    OrderIssueView, OrderStatusEligibilityView, OrderStatusEvidenceSummaryView,
    OrderStatusLifecycleCancellationView, OrderStatusLifecycleView, OrderStatusSdkReceiptView,
    OrderStatusView,
};

use super::{ORDER_ACTOR_CONTEXT_SDK_LOCAL, ORDER_STATUS_SDK_SOURCE};

pub(super) fn sdk_order_status_view(receipt: OrderStatusReceipt) -> OrderStatusView {
    let state = sdk_order_status_state(receipt.status).to_owned();
    let reducer_issues = receipt
        .issues
        .iter()
        .map(sdk_order_status_issue_view)
        .collect::<Vec<_>>();
    let reason = sdk_order_status_reason(receipt.status, receipt.order_id.as_str());
    let lifecycle = sdk_order_status_lifecycle_view(&receipt, reducer_issues.as_slice());
    let sdk_receipt = Some(sdk_order_status_receipt_view(&receipt));

    OrderStatusView {
        state,
        source: ORDER_STATUS_SDK_SOURCE.to_owned(),
        order_id: receipt.order_id.to_string(),
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
        revision: None,
        inventory: None,
        lifecycle: Some(lifecycle),
        sdk_receipt,
        reducer_issues,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        failed_relays: Vec::new(),
        fetched_count: 0,
        decoded_count: receipt.event_count,
        skipped_count: 0,
        reason,
        actions: Vec::new(),
    }
}

fn sdk_order_status_receipt_view(receipt: &OrderStatusReceipt) -> OrderStatusSdkReceiptView {
    OrderStatusSdkReceiptView {
        next_action: sdk_status_next_action(receipt.next_action).to_owned(),
        evidence: sdk_status_evidence_view(&receipt.evidence),
        eligibility: sdk_status_eligibility_view(&receipt.eligibility),
    }
}

fn sdk_status_evidence_view(
    evidence: &OrderStatusEvidenceSummary,
) -> OrderStatusEvidenceSummaryView {
    OrderStatusEvidenceSummaryView {
        event_count: evidence.event_count,
        limit_applied: evidence.limit_applied,
        has_request: evidence.has_request,
        has_decision: evidence.has_decision,
        has_agreement: evidence.has_agreement,
        has_pending_revision: evidence.has_pending_revision,
        has_cancellation: evidence.has_cancellation,
        has_issues: evidence.has_issues,
    }
}

fn sdk_status_eligibility_view(eligibility: &OrderStatusEligibility) -> OrderStatusEligibilityView {
    OrderStatusEligibilityView {
        can_decide: eligibility.can_decide,
        can_propose_revision: eligibility.can_propose_revision,
        can_decide_revision: eligibility.can_decide_revision,
        can_cancel: eligibility.can_cancel,
    }
}

fn sdk_status_next_action(kind: OrderStatusNextActionKind) -> &'static str {
    match kind {
        OrderStatusNextActionKind::NoLocalOrder => "no_local_order",
        OrderStatusNextActionKind::InspectEvidenceIssues => "inspect_evidence_issues",
        OrderStatusNextActionKind::AwaitSellerDecision => "await_seller_decision",
        OrderStatusNextActionKind::DecideRevision => "decide_revision",
        OrderStatusNextActionKind::AwaitRhiValidation => "await_rhi_validation",
        OrderStatusNextActionKind::Terminal => "terminal",
        _ => "unknown",
    }
}

fn sdk_order_status_state(status: OrderStatusKind) -> &'static str {
    match status {
        OrderStatusKind::Missing => "missing",
        OrderStatusKind::Requested => "requested",
        OrderStatusKind::RevisionProposed => "revision_proposed",
        OrderStatusKind::AgreedPendingRhi => "pending_rhi",
        OrderStatusKind::Committed => "committed",
        OrderStatusKind::Declined => "declined",
        OrderStatusKind::Cancelled => "cancelled",
        OrderStatusKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_order_status_reason(status: OrderStatusKind, order_id: &str) -> Option<String> {
    match status {
        OrderStatusKind::Missing => Some(format!("no local SDK order events matched `{order_id}`")),
        OrderStatusKind::Invalid => Some(format!(
            "local SDK order events for `{order_id}` failed reducer validation"
        )),
        _ => None,
    }
}

fn sdk_order_status_agreement_event_id(receipt: &OrderStatusReceipt) -> Option<String> {
    sdk_event_id_string(receipt.agreement_event_id.as_ref())
}

fn sdk_order_status_lifecycle_view(
    receipt: &OrderStatusReceipt,
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

fn sdk_order_status_lifecycle_phase(receipt: &OrderStatusReceipt) -> &'static str {
    match receipt.status {
        OrderStatusKind::Missing => "missing",
        OrderStatusKind::Requested => "requested",
        OrderStatusKind::RevisionProposed => "revision_proposed",
        OrderStatusKind::AgreedPendingRhi => "pending_rhi",
        OrderStatusKind::Committed => "committed",
        OrderStatusKind::Declined => "declined",
        OrderStatusKind::Cancelled => "cancelled",
        OrderStatusKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_order_status_issue_view(issue: &SdkOrderStatusIssue) -> OrderIssueView {
    let code = issue.code();
    OrderIssueView {
        code: code.clone(),
        field: "sdk_order_status".to_owned(),
        message: format!("SDK order status reported `{code}`"),
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
