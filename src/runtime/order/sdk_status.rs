use radroots_events::ids::RadrootsEventId;
use radroots_sdk::{
    OrderFulfillmentStatusKind, OrderPaymentHandoffKind, OrderPaymentStateKind,
    OrderSettlementStateKind, OrderStatusEligibility, OrderStatusEvidenceSummary, OrderStatusKind,
    OrderStatusNextActionKind, OrderStatusReceipt, SdkOrderStatusIssue,
};

use crate::view::runtime::{
    OrderIssueView, OrderStatusEligibilityView, OrderStatusEvidenceSummaryView,
    OrderStatusFulfillmentView, OrderStatusLifecycleCancellationView,
    OrderStatusLifecycleReceiptView, OrderStatusLifecycleView, OrderStatusPaymentView,
    OrderStatusSdkReceiptView, OrderStatusView,
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
    let fulfillment = sdk_order_status_fulfillment_view(&receipt, reducer_issues.as_slice());
    let lifecycle = sdk_order_status_lifecycle_view(&receipt, reducer_issues.as_slice());
    let payment = Some(sdk_order_status_payment_view(
        &receipt,
        reducer_issues.as_slice(),
    ));
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
        listing_addr: None,
        buyer_pubkey: None,
        seller_pubkey: None,
        economics: None,
        last_event_id: sdk_event_id_string(receipt.last_event_id.as_ref()),
        revision: None,
        inventory: None,
        fulfillment,
        lifecycle: Some(lifecycle),
        payment,
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
        payment_handoff: sdk_payment_handoff(receipt.payment_handoff).to_owned(),
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
        has_fulfillment: evidence.has_fulfillment,
        has_cancellation: evidence.has_cancellation,
        has_receipt: evidence.has_receipt,
        has_issues: evidence.has_issues,
    }
}

fn sdk_status_eligibility_view(eligibility: &OrderStatusEligibility) -> OrderStatusEligibilityView {
    OrderStatusEligibilityView {
        can_decide: eligibility.can_decide,
        can_propose_revision: eligibility.can_propose_revision,
        can_decide_revision: eligibility.can_decide_revision,
        can_cancel: eligibility.can_cancel,
        can_update_fulfillment: eligibility.can_update_fulfillment,
        can_record_receipt: eligibility.can_record_receipt,
    }
}

fn sdk_payment_handoff(kind: OrderPaymentHandoffKind) -> &'static str {
    match kind {
        OrderPaymentHandoffKind::NotReady => "not_ready",
        OrderPaymentHandoffKind::NotRequired => "not_required",
        OrderPaymentHandoffKind::InPersonOrOffPlatformPending => {
            "in_person_or_off_platform_pending"
        }
        OrderPaymentHandoffKind::InPersonOrOffPlatformRecorded => {
            "in_person_or_off_platform_recorded"
        }
        OrderPaymentHandoffKind::InPersonOrOffPlatformSettled => {
            "in_person_or_off_platform_settled"
        }
        OrderPaymentHandoffKind::Rejected => "rejected",
        OrderPaymentHandoffKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_status_next_action(kind: OrderStatusNextActionKind) -> &'static str {
    match kind {
        OrderStatusNextActionKind::NoLocalOrder => "no_local_order",
        OrderStatusNextActionKind::InspectEvidenceIssues => "inspect_evidence_issues",
        OrderStatusNextActionKind::AwaitSellerDecision => "await_seller_decision",
        OrderStatusNextActionKind::ArrangeInPersonOrOffPlatformPayment => {
            "arrange_in_person_or_off_platform_payment"
        }
        OrderStatusNextActionKind::DecideRevision => "decide_revision",
        OrderStatusNextActionKind::FulfillOrder => "fulfill_order",
        OrderStatusNextActionKind::RecordReceipt => "record_receipt",
        OrderStatusNextActionKind::Terminal => "terminal",
        _ => "unknown",
    }
}

fn sdk_order_status_state(status: OrderStatusKind) -> &'static str {
    match status {
        OrderStatusKind::Missing => "missing",
        OrderStatusKind::Requested => "requested",
        OrderStatusKind::Accepted => "accepted",
        OrderStatusKind::Declined => "declined",
        OrderStatusKind::Cancelled => "cancelled",
        OrderStatusKind::Completed => "completed",
        OrderStatusKind::Disputed => "disputed",
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

fn sdk_order_status_fulfillment_view(
    receipt: &OrderStatusReceipt,
    issues: &[OrderIssueView],
) -> Option<OrderStatusFulfillmentView> {
    let fulfillment_issues = issues
        .iter()
        .filter(|issue| {
            issue.code.starts_with("fulfillment_") || issue.code == "forked_fulfillments"
        })
        .cloned()
        .collect::<Vec<_>>();
    if !fulfillment_issues.is_empty() {
        return Some(OrderStatusFulfillmentView {
            state: "invalid".to_owned(),
            event_id: sdk_event_id_string(receipt.fulfillment_event_id.as_ref()),
            root_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
            prev_event_id: sdk_event_id_string(receipt.decision_event_id.as_ref()),
            terminal: false,
            inventory_released: false,
            issues: fulfillment_issues,
        });
    }
    let fulfillment_status = receipt.fulfillment_status?;
    Some(OrderStatusFulfillmentView {
        state: sdk_fulfillment_status_state(fulfillment_status).to_owned(),
        event_id: sdk_event_id_string(receipt.fulfillment_event_id.as_ref()),
        root_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
        prev_event_id: sdk_event_id_string(receipt.decision_event_id.as_ref()),
        terminal: matches!(
            fulfillment_status,
            OrderFulfillmentStatusKind::Delivered | OrderFulfillmentStatusKind::SellerCancelled
        ),
        inventory_released: matches!(
            fulfillment_status,
            OrderFulfillmentStatusKind::SellerCancelled
        ),
        issues: Vec::new(),
    })
}

fn sdk_order_status_payment_view(
    receipt: &OrderStatusReceipt,
    issues: &[OrderIssueView],
) -> OrderStatusPaymentView {
    let payment_issues = issues
        .iter()
        .filter(|issue| issue.code.starts_with("payment_") || issue.code.starts_with("settlement_"))
        .cloned()
        .collect::<Vec<_>>();
    OrderStatusPaymentView {
        state: sdk_payment_state(receipt.payment_state).to_owned(),
        settlement_state: sdk_settlement_state(receipt.settlement_state).to_owned(),
        payment_event_id: None,
        settlement_event_id: None,
        agreement_event_id: sdk_order_status_agreement_event_id(receipt),
        quote_id: None,
        quote_version: None,
        economics_digest: None,
        amount: None,
        currency: None,
        method: None,
        reference: None,
        paid_at: None,
        reason: None,
        issues: payment_issues,
    }
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
    let receipt_view =
        receipt
            .receipt_event_id
            .as_ref()
            .map(|event_id| OrderStatusLifecycleReceiptView {
                event_id: event_id.to_string(),
                root_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
                prev_event_id: sdk_event_id_string(receipt.fulfillment_event_id.as_ref()),
                received: matches!(receipt.status, OrderStatusKind::Completed),
                issue: None,
                received_at: None,
            });

    OrderStatusLifecycleView {
        phase: sdk_order_status_lifecycle_phase(receipt).to_owned(),
        terminal: receipt.lifecycle_terminal,
        event_id: sdk_event_id_string(receipt.last_event_id.as_ref()),
        root_event_id: sdk_event_id_string(receipt.request_event_id.as_ref()),
        prev_event_id: None,
        cancellation,
        receipt: receipt_view,
        settlement_required: !matches!(
            receipt.settlement_state,
            OrderSettlementStateKind::NotRequired
        ),
        settlement_reason: None,
        issues: issues.to_vec(),
    }
}

fn sdk_order_status_lifecycle_phase(receipt: &OrderStatusReceipt) -> &'static str {
    match receipt.status {
        OrderStatusKind::Missing => "missing",
        OrderStatusKind::Requested => "requested",
        OrderStatusKind::Accepted => match receipt.fulfillment_status {
            Some(OrderFulfillmentStatusKind::Preparing)
            | Some(OrderFulfillmentStatusKind::OutForDelivery) => "fulfillment_in_progress",
            Some(
                OrderFulfillmentStatusKind::ReadyForPickup
                | OrderFulfillmentStatusKind::Delivered
                | OrderFulfillmentStatusKind::SellerCancelled,
            ) => "fulfilled",
            Some(OrderFulfillmentStatusKind::AcceptedNotFulfilled) | None => "accepted",
            Some(_) => "accepted",
        },
        OrderStatusKind::Declined => "declined",
        OrderStatusKind::Cancelled => "cancelled",
        OrderStatusKind::Completed => "completed",
        OrderStatusKind::Disputed => "disputed",
        OrderStatusKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_fulfillment_status_state(status: OrderFulfillmentStatusKind) -> &'static str {
    match status {
        OrderFulfillmentStatusKind::AcceptedNotFulfilled => "accepted_not_fulfilled",
        OrderFulfillmentStatusKind::Preparing => "preparing",
        OrderFulfillmentStatusKind::ReadyForPickup => "ready_for_pickup",
        OrderFulfillmentStatusKind::OutForDelivery => "out_for_delivery",
        OrderFulfillmentStatusKind::Delivered => "delivered",
        OrderFulfillmentStatusKind::SellerCancelled => "seller_cancelled",
        _ => "unknown",
    }
}

fn sdk_payment_state(state: OrderPaymentStateKind) -> &'static str {
    match state {
        OrderPaymentStateKind::NotRecorded => "not_recorded",
        OrderPaymentStateKind::Recorded => "recorded",
        OrderPaymentStateKind::Settled => "settled",
        OrderPaymentStateKind::Rejected => "rejected",
        OrderPaymentStateKind::Invalid => "invalid",
        _ => "unknown",
    }
}

fn sdk_settlement_state(state: OrderSettlementStateKind) -> &'static str {
    match state {
        OrderSettlementStateKind::NotRequired => "not_required",
        OrderSettlementStateKind::Pending => "pending",
        OrderSettlementStateKind::Accepted => "accepted",
        OrderSettlementStateKind::Rejected => "rejected",
        OrderSettlementStateKind::Invalid => "invalid",
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
