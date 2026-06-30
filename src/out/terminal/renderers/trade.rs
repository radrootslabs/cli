use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};
use crate::out::terminal::values::{ready_blocked, relay_summary, yes_no};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("trade.submit", &TRADE_RENDERER)
        .register("trade.get", &TRADE_RENDERER)
        .register("trade.list", &TRADE_RENDERER)
        .register("trade.app.list", &TRADE_RENDERER)
        .register("trade.app.export", &TRADE_RENDERER)
        .register("trade.rebind", &TRADE_RENDERER)
        .register("trade.accept", &TRADE_RENDERER)
        .register("trade.decline", &TRADE_RENDERER)
        .register("trade.cancel", &TRADE_RENDERER)
        .register("trade.revision.propose", &TRADE_RENDERER)
        .register("trade.revision.accept", &TRADE_RENDERER)
        .register("trade.revision.decline", &TRADE_RENDERER)
        .register("trade.status.get", &TRADE_RENDERER)
        .register("trade.event.list", &TRADE_RENDERER)
        .register("trade.event.watch", &TRADE_RENDERER)
}

struct TradeRenderer;

static TRADE_RENDERER: TradeRenderer = TradeRenderer;

impl TerminalOperationRenderer for TradeRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        match envelope.operation_id.as_str() {
            "trade.list" => trade_list_document(envelope, result),
            "trade.app.list" => trade_app_list_document(envelope, result),
            "trade.app.export" => trade_app_export_document(envelope, result),
            "trade.get" => trade_detail_document(envelope, result),
            "trade.rebind" => trade_rebind_document(envelope, result),
            "trade.status.get" => trade_status_document(envelope, result),
            "trade.event.list" => trade_event_list_document(envelope, result),
            "trade.event.watch" => trade_event_watch_document(envelope, result),
            "trade.submit" => trade_publish_document(envelope, result),
            "trade.accept" | "trade.decline" => trade_decision_document(envelope, result),
            "trade.cancel" => trade_cancel_document(envelope, result),
            "trade.revision.propose" => trade_revision_propose_document(envelope, result),
            "trade.revision.accept" | "trade.revision.decline" => {
                trade_revision_decision_document(envelope, result)
            }
            _ => trade_detail_document(envelope, result),
        }
    }
}

fn trade_publish_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade submit");
    push_trade_identity_fields(&mut document, result);
    common::push_bool_field(&mut document, "Dry run", result, &["dry_run"]);
    common::push_bool_field(&mut document, "Deduplicated", result, &["deduplicated"]);
    common::push_path_field(&mut document, "Event", result, &["event_id"]);
    common::push_path_field(&mut document, "Kind", result, &["event_kind"]);
    common::push_path_field(&mut document, "Signer", result, &["signer_mode"]);
    push_relay_field(&mut document, result);
    push_job_fields(&mut document, result);
    push_trade_items_section(&mut document, result);
    push_issue_sections(&mut document, result);
    document
}

fn trade_decision_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(
        envelope,
        trade_decision_title(envelope.operation_id.as_str()),
    );
    push_trade_identity_fields(&mut document, result);
    common::push_path_field(&mut document, "Decision", result, &["decision"]);
    common::push_bool_field(&mut document, "Dry run", result, &["dry_run"]);
    common::push_path_field(&mut document, "Request", result, &["request_event_id"]);
    common::push_path_field(&mut document, "Event", result, &["event_id"]);
    common::push_path_field(&mut document, "Kind", result, &["event_kind"]);
    common::push_path_field(&mut document, "Signer", result, &["signer_mode"]);
    push_relay_field(&mut document, result);
    push_inventory_section(&mut document, result);
    push_issue_sections(&mut document, result);
    document
}

fn trade_cancel_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade cancel");
    push_trade_identity_fields(&mut document, result);
    common::push_path_field(
        &mut document,
        "Cancel reason",
        result,
        &["cancellation_reason"],
    );
    common::push_bool_field(&mut document, "Dry run", result, &["dry_run"]);
    common::push_path_field(&mut document, "Request", result, &["request_event_id"]);
    common::push_path_field(&mut document, "Decision", result, &["decision_event_id"]);
    common::push_path_field(&mut document, "Event", result, &["event_id"]);
    common::push_path_field(&mut document, "Signer", result, &["signer_mode"]);
    push_relay_field(&mut document, result);
    push_issue_sections(&mut document, result);
    document
}

fn trade_revision_propose_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade revision propose");
    push_trade_identity_fields(&mut document, result);
    common::push_path_field(&mut document, "Revision", result, &["revision_id"]);
    common::push_bool_field(&mut document, "Dry run", result, &["dry_run"]);
    common::push_path_field(&mut document, "Request", result, &["request_event_id"]);
    common::push_path_field(&mut document, "Decision", result, &["decision_event_id"]);
    common::push_path_field(&mut document, "Event", result, &["event_id"]);
    common::push_path_field(&mut document, "Signer", result, &["signer_mode"]);
    push_relay_field(&mut document, result);
    push_trade_items_section(&mut document, result);
    push_inventory_section(&mut document, result);
    push_issue_sections(&mut document, result);
    document
}

fn trade_revision_decision_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(
        envelope,
        trade_revision_title(envelope.operation_id.as_str()),
    );
    push_trade_identity_fields(&mut document, result);
    common::push_path_field(&mut document, "Revision", result, &["revision_id"]);
    common::push_path_field(&mut document, "Decision", result, &["decision"]);
    common::push_bool_field(&mut document, "Dry run", result, &["dry_run"]);
    common::push_path_field(&mut document, "Request", result, &["request_event_id"]);
    common::push_path_field(&mut document, "Agreement", result, &["agreement_event_id"]);
    common::push_path_field(&mut document, "Event", result, &["event_id"]);
    common::push_path_field(&mut document, "Signer", result, &["signer_mode"]);
    push_relay_field(&mut document, result);
    push_inventory_section(&mut document, result);
    push_issue_sections(&mut document, result);
    document
}

fn trade_detail_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Trade");
    push_trade_identity_fields(&mut document, result);
    common::push_bool_field(&mut document, "Submit ready", result, &["ready_for_submit"]);
    common::push_path_field(
        &mut document,
        "Buyer account",
        result,
        &["buyer_account_id"],
    );
    common::push_path_field(&mut document, "Buyer custody", result, &["buyer_custody"]);
    common::push_bool_field(
        &mut document,
        "Buyer can write",
        result,
        &["buyer_write_capable"],
    );
    common::push_path_field(&mut document, "Updated", result, &["updated_at_unix"]);
    push_job_fields(&mut document, result);
    push_workflow_fields(&mut document, result);
    push_trade_items_section(&mut document, result);
    push_issue_sections(&mut document, result);
    document
}

fn trade_rebind_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade rebind");
    common::push_path_field(&mut document, "Lookup", result, &["lookup"]);
    common::push_path_field(&mut document, "From", result, &["from_trade_id"]);
    common::push_path_field(&mut document, "To", result, &["to_trade_id"]);
    common::push_bool_field(
        &mut document,
        "Trade changed",
        result,
        &["trade_id_changed"],
    );
    common::push_path_field(
        &mut document,
        "From account",
        result,
        &["from_buyer_account_id"],
    );
    common::push_path_field(
        &mut document,
        "To account",
        result,
        &["to_buyer_account_id"],
    );
    common::push_bool_field(
        &mut document,
        "Buyer changed",
        result,
        &["buyer_pubkey_changed"],
    );
    common::push_path_field(
        &mut document,
        "Existing request",
        result,
        &["existing_request_check"],
    );
    push_lines_section(
        &mut document,
        "Existing requests",
        result,
        &["existing_request_event_ids"],
    );
    document
}

fn trade_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Trades");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    let rows = common::array(result, &["trades"])
        .into_iter()
        .flatten()
        .map(|trade| {
            TerminalTableRow::new(vec![
                common::string(trade, &["id"]).unwrap_or_default(),
                common::string(trade, &["state"]).unwrap_or_default(),
                common::bool_path(trade, &["ready_for_submit"])
                    .map(ready_blocked)
                    .unwrap_or_default()
                    .to_owned(),
                common::number_label_path(trade, &["item_count"]).unwrap_or_default(),
                common::string(trade, &["listing_addr"])
                    .or_else(|| common::string(trade, &["listing_lookup"]))
                    .unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Trades",
        vec![
            TerminalTableColumn::new("Trade", 6, 24),
            TerminalTableColumn::new("State", 5, 14),
            TerminalTableColumn::new("Submit", 6, 8),
            TerminalTableColumn::new("Items", 5, 5),
            TerminalTableColumn::new("Listing", 8, 28),
        ],
        rows,
        "No trade drafts found",
    ));
    document
}

fn trade_app_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Trade app records");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_bool_field(&mut document, "More", result, &["has_more"]);
    let rows = common::array(result, &["records"])
        .into_iter()
        .flatten()
        .map(|record| {
            TerminalTableRow::new(vec![
                common::string(record, &["record_id"]).unwrap_or_default(),
                common::string(record, &["status"]).unwrap_or_default(),
                common::string(record, &["trade_id"]).unwrap_or_default(),
                common::bool_path(record, &["exportable"])
                    .map(yes_no)
                    .unwrap_or_default()
                    .to_owned(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Records",
        vec![
            TerminalTableColumn::new("Record", 8, 28),
            TerminalTableColumn::new("Status", 6, 14),
            TerminalTableColumn::new("Trade", 6, 24),
            TerminalTableColumn::new("Export", 6, 6),
        ],
        rows,
        "No app-authored trade records found",
    ));
    document
}

fn trade_app_export_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade app export");
    common::push_path_field(&mut document, "Record", result, &["record_id"]);
    common::push_path_field(&mut document, "Trade", result, &["trade_id"]);
    common::push_bool_field(&mut document, "Dry run", result, &["dry_run"]);
    common::push_bool_field(&mut document, "Valid", result, &["valid"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_addr"]);
    common::push_path_field(
        &mut document,
        "Buyer account",
        result,
        &["buyer_account_id"],
    );
    push_issue_sections(&mut document, result);
    document
}

fn trade_status_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade status");
    push_trade_identity_fields(&mut document, result);
    common::push_path_field(
        &mut document,
        "Actor context",
        result,
        &["actor_context_source"],
    );
    common::push_path_field(&mut document, "Last event", result, &["last_event_id"]);
    common::push_path_field(&mut document, "Lifecycle", result, &["lifecycle", "phase"]);
    common::push_bool_field(
        &mut document,
        "Terminal",
        result,
        &["lifecycle", "terminal"],
    );
    common::push_path_field(&mut document, "Revision", result, &["revision", "state"]);
    push_relay_field(&mut document, result);
    push_inventory_section(&mut document, result);
    push_issue_sections(&mut document, result);
    push_nested_issues_section(
        &mut document,
        "Lifecycle issues",
        result,
        &["lifecycle", "issues"],
    );
    push_nested_issues_section(&mut document, "Reducer issues", result, &["reducer_issues"]);
    document
}

fn trade_event_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Trade events");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_path_field(
        &mut document,
        "Actor context",
        result,
        &["actor_context_source"],
    );
    common::push_path_field(&mut document, "Seller", result, &["seller_pubkey"]);
    push_relay_field(&mut document, result);
    let rows = common::array(result, &["trades"])
        .into_iter()
        .flatten()
        .map(|trade| {
            TerminalTableRow::new(vec![
                common::string(trade, &["id"]).unwrap_or_default(),
                common::string(trade, &["state"]).unwrap_or_default(),
                common::string(trade, &["event_id"]).unwrap_or_default(),
                common::number_label_path(trade, &["event_kind"]).unwrap_or_default(),
                common::string(trade, &["listing_addr"])
                    .or_else(|| common::string(trade, &["listing_lookup"]))
                    .unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Events",
        vec![
            TerminalTableColumn::new("Trade", 6, 22),
            TerminalTableColumn::new("State", 5, 14),
            TerminalTableColumn::new("Event", 5, 22),
            TerminalTableColumn::new("Kind", 4, 6),
            TerminalTableColumn::new("Listing", 8, 24),
        ],
        rows,
        "No trade events found",
    ));
    document
}

fn trade_event_watch_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Trade event watch");
    common::push_path_field(&mut document, "Trade", result, &["trade_id"]);
    common::push_path_field(&mut document, "Reason", result, &["reason"]);
    document
}

fn push_trade_identity_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(document, "State", result, &["state"]);
    common::push_path_field(document, "Trade", result, &["trade_id"]);
    common::push_path_field(
        document,
        "Locator root",
        result,
        &["locator", "root_event_id"],
    );
    common::push_path_field(
        document,
        "Locator listing",
        result,
        &["locator", "listing_addr"],
    );
    common::push_path_field(
        document,
        "Locator buyer",
        result,
        &["locator", "buyer_pubkey"],
    );
    common::push_path_field(
        document,
        "Locator seller",
        result,
        &["locator", "seller_pubkey"],
    );
    common::push_path_field(document, "Lookup", result, &["lookup"]);
    common::push_path_field(document, "Listing", result, &["listing_addr"]);
    common::push_path_field(document, "Listing", result, &["listing_lookup"]);
    common::push_path_field(document, "Listing event", result, &["listing_event_id"]);
    common::push_path_field(document, "Buyer", result, &["buyer_pubkey"]);
    common::push_path_field(document, "Seller", result, &["seller_pubkey"]);
    common::push_path_field(document, "Reason", result, &["reason"]);
}

fn push_relay_field(document: &mut TerminalDocument, result: &Value) {
    let acknowledged = array_len(result, &["acknowledged_relays"]);
    let connected = array_len(result, &["connected_relays"]);
    let failed = array_len(result, &["failed_relays"]);
    if acknowledged > 0 || failed > 0 {
        common::push_field(
            document,
            "Relays",
            relay_summary(acknowledged, failed, "acknowledged"),
        );
    } else if connected > 0 || failed > 0 {
        common::push_field(
            document,
            "Relays",
            relay_summary(connected, failed, "connected"),
        );
    }
}

fn push_job_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(document, "Job", result, &["job", "job_id"]);
    common::push_path_field(document, "Job state", result, &["job", "state"]);
    common::push_path_field(document, "Job event", result, &["job", "event_id"]);
}

fn push_workflow_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(document, "Workflow", result, &["workflow", "state"]);
    common::push_path_field(
        document,
        "Workflow event",
        result,
        &["workflow", "last_event_id"],
    );
}

fn push_trade_items_section(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["items"])
        .into_iter()
        .flatten()
        .map(|item| {
            TerminalTableRow::new(vec![
                common::string(item, &["bin_id"]).unwrap_or_default(),
                common::number_label_path(item, &["bin_count"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Items",
        vec![
            TerminalTableColumn::new("Bin", 3, 18),
            TerminalTableColumn::new("Count", 5, 8),
        ],
        rows,
        "No trade items",
    ));
}

fn push_inventory_section(document: &mut TerminalDocument, result: &Value) {
    let Some(inventory) = result.get("inventory") else {
        return;
    };
    common::push_path_field(document, "Inventory", inventory, &["state"]);
    common::push_bool_field(document, "Commitment", inventory, &["commitment_valid"]);
    let rows = common::array(inventory, &["bins"])
        .into_iter()
        .flatten()
        .map(|bin| {
            TerminalTableRow::new(vec![
                common::string(bin, &["bin_id"]).unwrap_or_default(),
                common::number_label_path(bin, &["committed_count"]).unwrap_or_default(),
                common::number_label_path(bin, &["available_count"]).unwrap_or_default(),
                common::number_label_path(bin, &["remaining_count"]).unwrap_or_default(),
                common::bool_path(bin, &["over_reserved"])
                    .map(yes_no)
                    .unwrap_or_default()
                    .to_owned(),
            ])
        })
        .collect::<Vec<_>>();
    if !rows.is_empty() {
        document.sections.push(common::table_section(
            "Inventory",
            vec![
                TerminalTableColumn::new("Bin", 3, 16),
                TerminalTableColumn::new("Commit", 6, 8),
                TerminalTableColumn::new("Avail", 5, 8),
                TerminalTableColumn::new("Remain", 6, 8),
                TerminalTableColumn::new("Over", 4, 5),
            ],
            rows,
            "No inventory bins",
        ));
    }
    push_nested_issues_section(document, "Inventory issues", inventory, &["issues"]);
}

fn push_issue_sections(document: &mut TerminalDocument, result: &Value) {
    push_nested_issues_section(document, "Issues", result, &["issues"]);
}

fn push_nested_issues_section(
    document: &mut TerminalDocument,
    title: &str,
    value: &Value,
    path: &[&str],
) {
    let rows = issue_rows(value, path);
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        title,
        vec![
            TerminalTableColumn::new("Code", 8, 32),
            TerminalTableColumn::new("Field", 8, 24),
            TerminalTableColumn::new("Message", 7, 48),
        ],
        rows,
        "No trade issues",
    ));
}

fn issue_rows(value: &Value, path: &[&str]) -> Vec<TerminalTableRow> {
    common::array(value, path)
        .into_iter()
        .flatten()
        .map(|issue| {
            TerminalTableRow::new(vec![
                common::string(issue, &["code"]).unwrap_or_default(),
                common::string(issue, &["field"]).unwrap_or_default(),
                common::string(issue, &["message"]).unwrap_or_default(),
            ])
        })
        .collect()
}

fn push_lines_section(document: &mut TerminalDocument, title: &str, value: &Value, path: &[&str]) {
    let lines = common::array(value, path)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !lines.is_empty() {
        document
            .sections
            .push(crate::out::terminal::layout::TerminalSection::lines(
                title, lines,
            ));
    }
}

fn array_len(value: &Value, path: &[&str]) -> usize {
    common::array(value, path).map(Vec::len).unwrap_or_default()
}

fn trade_decision_title(operation_id: &str) -> &'static str {
    match operation_id {
        "trade.accept" => "Trade accept",
        "trade.decline" => "Trade decline",
        _ => "Trade decision",
    }
}

fn trade_revision_title(operation_id: &str) -> &'static str {
    match operation_id {
        "trade.revision.accept" => "Trade revision accept",
        "trade.revision.decline" => "Trade revision decline",
        _ => "Trade revision decision",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::out::envelope::{EnvelopeContext, next_actions_from_result_value};
    use crate::out::terminal::renderer::render_terminal_document;

    use super::*;

    fn success_envelope(operation_id: &str, result: Value) -> OutputEnvelope {
        let mut envelope = OutputEnvelope::success(
            operation_id,
            result,
            EnvelopeContext::new(format!("req_{operation_id}"), false),
        );
        envelope.next_actions = next_actions_from_result_value(&envelope.result);
        envelope
    }

    #[test]
    fn trade_list_uses_trade_surface_without_internal_draft_path() {
        let envelope = success_envelope(
            "trade.list",
            json!({
                "state": "ready",
                "count": 1,
                "trades": [{
                    "id": "trade_test",
                    "state": "ready",
                    "ready_for_submit": true,
                    "file": "orders/drafts/trade_test.toml",
                    "item_count": 1,
                    "listing_addr": "listing_test"
                }],
                "actions": ["radroots trade get trade_test"]
            }),
        );
        let document = TRADE_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Trades"));
        assert!(rendered.contains("trade_test"));
        assert!(rendered.contains("radroots trade get trade_test"));
        assert!(!rendered.contains("orders/drafts"));
        assert!(!rendered.contains("Order"));
    }

    #[test]
    fn event_watch_result_is_deferred_status_not_stream_claim() {
        let envelope = success_envelope(
            "trade.event.watch",
            json!({
                "state": "not_implemented",
                "trade_id": "trade_test",
                "reason": "relay-backed trade event watch is not implemented",
                "actions": ["radroots trade status get trade_test"]
            }),
        );
        let document = TRADE_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Trade event watch"));
        assert!(rendered.contains("not implemented"));
        assert!(rendered.contains("radroots trade status get trade_test"));
        assert!(!rendered.contains("stream"));
    }
}
