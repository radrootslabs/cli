use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("basket.create", &BASKET_RENDERER)
        .register("basket.get", &BASKET_RENDERER)
        .register("basket.list", &BASKET_RENDERER)
        .register("basket.item.add", &BASKET_RENDERER)
        .register("basket.item.update", &BASKET_RENDERER)
        .register("basket.item.remove", &BASKET_RENDERER)
        .register("basket.adjustment.add", &BASKET_RENDERER)
        .register("basket.adjustment.remove", &BASKET_RENDERER)
        .register("basket.validate", &BASKET_RENDERER)
        .register("basket.quote.create", &BASKET_RENDERER)
}

struct BasketRenderer;

static BASKET_RENDERER: BasketRenderer = BasketRenderer;

impl TerminalOperationRenderer for BasketRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::generic_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        match envelope.operation_id.as_str() {
            "basket.list" => basket_list_document(envelope, result),
            "basket.validate" => basket_validate_document(envelope, result),
            "basket.quote.create" => basket_quote_document(envelope, result),
            "basket.item.add" | "basket.item.update" | "basket.item.remove" => {
                basket_mutation_document(envelope, result)
            }
            "basket.adjustment.add" | "basket.adjustment.remove" => {
                basket_mutation_document(envelope, result)
            }
            _ => basket_detail_document(envelope, result),
        }
    }
}

fn basket_detail_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, basket_title(envelope.operation_id.as_str()));
    push_basket_fields(&mut document, result);
    push_items_section(&mut document, result);
    push_adjustments_section(&mut document, result);
    push_issues_section(&mut document, result);
    document
}

fn basket_mutation_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, basket_title(envelope.operation_id.as_str()));
    push_basket_fields(&mut document, result);
    push_single_item_fields(&mut document, result);
    push_single_adjustment_fields(&mut document, result);
    push_items_section(&mut document, result);
    push_adjustments_section(&mut document, result);
    push_issues_section(&mut document, result);
    document
}

fn basket_validate_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Basket validation");
    push_basket_fields(&mut document, result);
    push_issues_section(&mut document, result);
    document
}

fn basket_quote_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Basket quote");
    push_basket_fields(&mut document, result);
    common::push_path_field(&mut document, "Quote", result, &["quote", "quote_id"]);
    common::push_path_field(&mut document, "Trade", result, &["quote", "trade_id"]);
    common::push_path_field(&mut document, "Trade", result, &["trade", "trade_id"]);
    common::push_bool_field(
        &mut document,
        "Submit ready",
        result,
        &["quote", "ready_for_submit"],
    );
    push_single_item_fields(&mut document, result);
    push_issues_section(&mut document, result);
    push_trade_issues_section(&mut document, result);
    document
}

fn basket_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Baskets");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    let rows = common::array(result, &["baskets"])
        .into_iter()
        .flatten()
        .map(|basket| {
            TerminalTableRow::new(vec![
                common::string(basket, &["basket_id"]).unwrap_or_default(),
                common::string(basket, &["state"]).unwrap_or_default(),
                common::number_label_path(basket, &["item_count"]).unwrap_or_default(),
                common::bool_path(basket, &["ready_for_quote"])
                    .map(|ready| if ready { "yes" } else { "no" }.to_owned())
                    .unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Baskets",
        vec![
            TerminalTableColumn::new("Basket", 6, 24),
            TerminalTableColumn::new("State", 5, 14),
            TerminalTableColumn::new("Items", 5, 5),
            TerminalTableColumn::new("Quote", 5, 5),
        ],
        rows,
        "No baskets found",
    ));
    document
}

fn push_basket_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(document, "State", result, &["state"]);
    common::push_path_field(document, "Basket", result, &["basket_id"]);
    common::push_path_field(document, "Lookup", result, &["lookup"]);
    common::push_count_field(document, "Items", result, &["item_count"]);
    common::push_count_field(document, "Adjustments", result, &["adjustment_count"]);
    common::push_bool_field(document, "Quote ready", result, &["ready_for_quote"]);
    common::push_path_field(document, "File", result, &["file"]);
    common::push_path_field(document, "Reason", result, &["reason"]);
}

fn push_single_item_fields(document: &mut TerminalDocument, result: &Value) {
    let Some(item) = result.get("item") else {
        return;
    };
    common::push_path_field(document, "Item", item, &["item_id"]);
    common::push_path_field(document, "Listing", item, &["listing"]);
    common::push_path_field(document, "Listing", item, &["listing_addr"]);
    common::push_path_field(document, "Bin", item, &["bin_id"]);
    common::push_count_field(document, "Quantity", item, &["quantity"]);
}

fn push_single_adjustment_fields(document: &mut TerminalDocument, result: &Value) {
    let Some(adjustment) = result.get("adjustment") else {
        return;
    };
    common::push_path_field(document, "Adjustment", adjustment, &["id"]);
    common::push_path_field(document, "Effect", adjustment, &["effect"]);
    common::push_path_field(document, "Amount", adjustment, &["amount"]);
    common::push_path_field(document, "Currency", adjustment, &["currency"]);
    common::push_path_field(document, "Reason", adjustment, &["reason"]);
}

fn push_items_section(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["items"])
        .into_iter()
        .flatten()
        .map(|item| {
            TerminalTableRow::new(vec![
                common::string(item, &["item_id"]).unwrap_or_default(),
                common::string(item, &["listing"])
                    .or_else(|| common::string(item, &["listing_addr"]))
                    .unwrap_or_default(),
                common::string(item, &["bin_id"]).unwrap_or_default(),
                common::number_label_path(item, &["quantity"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Items",
        vec![
            TerminalTableColumn::new("Item", 4, 12),
            TerminalTableColumn::new("Listing", 8, 34),
            TerminalTableColumn::new("Bin", 3, 10),
            TerminalTableColumn::new("Qty", 3, 5),
        ],
        rows,
        "No basket items",
    ));
}

fn push_adjustments_section(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["adjustments"])
        .into_iter()
        .flatten()
        .map(|adjustment| {
            TerminalTableRow::new(vec![
                common::string(adjustment, &["id"]).unwrap_or_default(),
                common::string(adjustment, &["effect"]).unwrap_or_default(),
                common::string(adjustment, &["amount"]).unwrap_or_default(),
                common::string(adjustment, &["reason"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Adjustments",
        vec![
            TerminalTableColumn::new("ID", 2, 14),
            TerminalTableColumn::new("Effect", 6, 9),
            TerminalTableColumn::new("Amount", 6, 10),
            TerminalTableColumn::new("Reason", 6, 24),
        ],
        rows,
        "No basket adjustments",
    ));
}

fn push_issues_section(document: &mut TerminalDocument, result: &Value) {
    let rows = issue_rows(result, &["issues"]);
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Issues",
        vec![
            TerminalTableColumn::new("Code", 8, 32),
            TerminalTableColumn::new("Field", 8, 24),
            TerminalTableColumn::new("Message", 7, 48),
        ],
        rows,
        "No basket issues",
    ));
}

fn push_trade_issues_section(document: &mut TerminalDocument, result: &Value) {
    let rows = issue_rows(result, &["trade", "issues"]);
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Trade issues",
        vec![
            TerminalTableColumn::new("Code", 8, 32),
            TerminalTableColumn::new("Field", 8, 24),
            TerminalTableColumn::new("Message", 7, 48),
        ],
        rows,
        "No trade issues",
    ));
}

fn issue_rows(result: &Value, path: &[&str]) -> Vec<TerminalTableRow> {
    common::array(result, path)
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

fn basket_title(operation_id: &str) -> &'static str {
    match operation_id {
        "basket.create" => "Basket created",
        "basket.get" => "Basket",
        "basket.item.add" => "Basket item added",
        "basket.item.update" => "Basket item updated",
        "basket.item.remove" => "Basket item removed",
        "basket.adjustment.add" => "Basket adjustment added",
        "basket.adjustment.remove" => "Basket adjustment removed",
        _ => "Basket",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::out::envelope::{EnvelopeContext, next_actions_from_result_value};
    use crate::out::terminal::renderer::render_terminal_document;

    use super::*;

    fn success_envelope(operation_id: &str, result: Value, dry_run: bool) -> OutputEnvelope {
        let mut envelope = OutputEnvelope::success(
            operation_id,
            result,
            EnvelopeContext::new(format!("req_{operation_id}"), dry_run),
        );
        envelope.next_actions = next_actions_from_result_value(&envelope.result);
        envelope
    }

    #[test]
    fn empty_list_renders_stable_empty_table() {
        let envelope = success_envelope(
            "basket.list",
            json!({
                "state": "empty",
                "count": 0,
                "baskets": [],
                "actions": ["radroots basket create"]
            }),
            false,
        );
        let document = BASKET_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Baskets"));
        assert!(rendered.contains("No baskets found"));
        assert!(rendered.contains("radroots basket create"));
    }

    #[test]
    fn validation_issues_render_as_issue_table() {
        let envelope = success_envelope(
            "basket.validate",
            json!({
                "state": "unconfigured",
                "basket_id": "basket_test",
                "ready_for_quote": false,
                "item_count": 1,
                "adjustment_count": 0,
                "issues": [{
                    "code": "basket_market_replica_missing",
                    "field": "local.replica_db",
                    "message": "current local replica data is required before quote creation"
                }],
                "actions": ["radroots basket get basket_test"]
            }),
            false,
        );
        let document = BASKET_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Issues"));
        assert!(rendered.contains("basket_market_replica_missing"));
        assert!(rendered.contains("local.replica_db"));
    }

    #[test]
    fn incomplete_buyer_actions_are_placeholdered() {
        let envelope = success_envelope(
            "basket.item.add",
            json!({
                "state": "dry_run",
                "basket_id": "basket_test",
                "item": {
                    "item_id": "item_1",
                    "listing": "eggs",
                    "bin_id": "bin-1",
                    "quantity": 1
                },
                "actions": ["radroots basket item add"]
            }),
            true,
        );
        let document = BASKET_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(
            rendered
                .contains("radroots basket item add <basket> --listing <product> --bin-id <bin>")
        );
    }

    #[test]
    fn quote_next_action_uses_trade_surface() {
        let envelope = success_envelope(
            "basket.quote.create",
            json!({
                "state": "quoted",
                "basket_id": "basket_test",
                "quote": {
                    "quote_id": "quote_test",
                    "trade_id": "trade_test",
                    "trade_file": "internal-draft-path",
                    "ready_for_submit": true
                },
                "trade": {
                    "trade_id": "trade_test",
                    "file": "internal-draft-path",
                    "ready_for_submit": true
                },
                "actions": ["radroots trade submit trade_test"]
            }),
            false,
        );
        let document = BASKET_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Trade"));
        assert!(rendered.contains("radroots trade submit trade_test"));
        assert!(!rendered.contains("order"));
        assert!(!rendered.contains("internal-draft-path"));
    }
}
