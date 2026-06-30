use radroots_events::ids::RadrootsEventId;
use radroots_sdk::{
    SdkTradeStatusIssue, TradeStatusEligibility, TradeStatusEvidenceSummary, TradeStatusKind,
    TradeStatusNextActionKind, TradeStatusReceipt,
};

use crate::view::runtime::{
    OrderIssueView, OrderStatusEligibilityView, OrderStatusEvidenceSummaryView,
    OrderStatusLifecycleCancellationView, OrderStatusLifecycleView, OrderStatusSdkReceiptView,
    OrderStatusView, OrderTradeLocatorView,
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

    OrderStatusView {
        state,
        source: ORDER_STATUS_SDK_SOURCE.to_owned(),
        order_id: receipt.order_id.to_string(),
        locator: sdk_trade_locator_view(&receipt),
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

fn sdk_trade_locator_view(receipt: &TradeStatusReceipt) -> OrderTradeLocatorView {
    OrderTradeLocatorView {
        trade_id: receipt.locator.trade_id.as_str().to_owned(),
        root_event_id: receipt
            .locator
            .root_event_id
            .as_ref()
            .map(ToString::to_string),
        listing_addr: receipt
            .locator
            .listing_addr
            .as_ref()
            .map(ToString::to_string),
        buyer_pubkey: receipt
            .locator
            .buyer_pubkey
            .as_ref()
            .map(ToString::to_string),
        seller_pubkey: receipt
            .locator
            .seller_pubkey
            .as_ref()
            .map(ToString::to_string),
    }
}

fn sdk_order_status_receipt_view(receipt: &TradeStatusReceipt) -> OrderStatusSdkReceiptView {
    OrderStatusSdkReceiptView {
        next_action: sdk_status_next_action(receipt.next_action).to_owned(),
        evidence: sdk_status_evidence_view(&receipt.evidence),
        eligibility: sdk_status_eligibility_view(&receipt.eligibility),
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
        has_pending_revision: evidence.has_pending_revision,
        has_cancellation: evidence.has_cancellation,
        has_issues: evidence.has_issues,
    }
}

fn sdk_status_eligibility_view(eligibility: &TradeStatusEligibility) -> OrderStatusEligibilityView {
    OrderStatusEligibilityView {
        can_decide: eligibility.can_decide,
        can_propose_revision: eligibility.can_propose_revision,
        can_decide_revision: eligibility.can_decide_revision,
        can_cancel: eligibility.can_cancel,
    }
}

fn sdk_status_next_action(kind: TradeStatusNextActionKind) -> &'static str {
    match kind {
        TradeStatusNextActionKind::NoLocalOrder => "no_local_order",
        TradeStatusNextActionKind::InspectEvidenceIssues => "inspect_evidence_issues",
        TradeStatusNextActionKind::AwaitSellerDecision => "await_seller_decision",
        TradeStatusNextActionKind::DecideRevision => "decide_revision",
        TradeStatusNextActionKind::AwaitRhiValidation => "await_rhi_validation",
        TradeStatusNextActionKind::Terminal => "terminal",
        _ => "unknown",
    }
}

fn sdk_order_status_state(status: TradeStatusKind) -> &'static str {
    match status {
        TradeStatusKind::Missing => "missing",
        TradeStatusKind::Ambiguous => "ambiguous",
        TradeStatusKind::Requested => "requested",
        TradeStatusKind::RevisionProposed => "revision_proposed",
        TradeStatusKind::AgreedPendingRhi => "pending_rhi",
        TradeStatusKind::Committed => "committed",
        TradeStatusKind::Declined => "declined",
        TradeStatusKind::Cancelled => "cancelled",
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
        TradeStatusKind::RevisionProposed => "revision_proposed",
        TradeStatusKind::AgreedPendingRhi => "pending_rhi",
        TradeStatusKind::Committed => "committed",
        TradeStatusKind::Declined => "declined",
        TradeStatusKind::Cancelled => "cancelled",
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
