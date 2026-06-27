use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::{TerminalDocument, TerminalSection};
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};
use crate::out::terminal::values::{price_label, quantity_label};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("market.refresh", &MARKET_RENDERER)
        .register("market.product.search", &MARKET_RENDERER)
        .register("market.listing.get", &MARKET_RENDERER)
}

struct MarketRenderer;

static MARKET_RENDERER: MarketRenderer = MarketRenderer;

impl TerminalOperationRenderer for MarketRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        match envelope.operation_id.as_str() {
            "market.product.search" => market_search_document(envelope, result),
            "market.listing.get" => market_listing_document(envelope, result),
            _ => market_refresh_document(envelope, result),
        }
    }
}

fn market_refresh_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Market refresh");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(
        &mut document,
        "Freshness",
        result,
        &["freshness", "display"],
    );
    common::push_count_field(&mut document, "Relays", result, &["relay_count"]);
    common::push_count_field(&mut document, "Fetched", result, &["fetched_count"]);
    common::push_count_field(&mut document, "Ingested", result, &["ingested_count"]);
    common::push_count_field(&mut document, "Skipped", result, &["skipped_count"]);
    common::push_count_field(&mut document, "Failed", result, &["failed_count"]);
    common::push_path_field(&mut document, "Replica", result, &["replica_db"]);
    common::push_path_field(&mut document, "Reason", result, &["reason"]);
    document
}

fn market_search_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Market search");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Query", result, &["query"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_path_field(
        &mut document,
        "Freshness",
        result,
        &["freshness", "display"],
    );
    common::push_count_field(&mut document, "Relays", result, &["relay_count"]);
    let rows = common::array(result, &["results"])
        .into_iter()
        .flatten()
        .map(|entry| {
            TerminalTableRow::new(vec![
                common::string(entry, &["product_key"])
                    .or_else(|| common::string(entry, &["id"]))
                    .unwrap_or_default(),
                common::string(entry, &["title"]).unwrap_or_default(),
                quantity(entry, &["available"]),
                price(entry, &["price"]),
                checkout_label(entry),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Results",
        vec![
            TerminalTableColumn::new("Listing", 8, 18),
            TerminalTableColumn::new("Title", 5, 22),
            TerminalTableColumn::new("Available", 9, 16),
            TerminalTableColumn::new("Price", 5, 18),
            TerminalTableColumn::new("Checkout", 8, 8),
        ],
        rows,
        "No matching market listings",
    ));
    if let Some(hyf) = result.get("hyf") {
        common::push_verbose_path_field(&mut document, "HYF", hyf, &["state"]);
        common::push_verbose_path_field(&mut document, "Rewritten", hyf, &["rewritten_query"]);
    }
    document
}

fn market_listing_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Market listing");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "Key", result, &["product_key"]);
    common::push_path_field(&mut document, "Title", result, &["title"]);
    common::push_path_field(&mut document, "Category", result, &["category"]);
    common::push_path_field(&mut document, "Location", result, &["location_primary"]);
    common::push_path_field(&mut document, "Primary bin", result, &["primary_bin_id"]);
    common::push_field(&mut document, "Available", quantity(result, &["available"]));
    common::push_field(&mut document, "Price", price(result, &["price"]));
    common::push_field(&mut document, "Checkout", checkout_label(result));
    common::push_path_field(
        &mut document,
        "Freshness",
        result,
        &["provenance", "freshness"],
    );
    push_reason_codes(&mut document, result);
    document
}

fn push_reason_codes(document: &mut TerminalDocument, result: &Value) {
    let lines = common::array(result, &["reason_codes"])
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !lines.is_empty() {
        document
            .sections
            .push(TerminalSection::lines("Reasons", lines));
    }
}

fn quantity(value: &Value, path: &[&str]) -> String {
    let Some(quantity) = nested(value, path) else {
        return String::new();
    };
    common::string(quantity, &["label"]).unwrap_or_else(|| {
        let amount = common::number_label_path(quantity, &["total_amount"])
            .or_else(|| common::number_label_path(quantity, &["available_amount"]))
            .unwrap_or_default();
        let unit = common::string(quantity, &["total_unit"]);
        quantity_label(amount, unit.as_deref())
    })
}

fn price(value: &Value, path: &[&str]) -> String {
    let Some(price) = nested(value, path) else {
        return String::new();
    };
    let amount = common::number_label_path(price, &["amount"]).unwrap_or_default();
    let currency = common::string(price, &["currency"]).unwrap_or_default();
    let unit = common::string(price, &["per_unit"]);
    if currency.is_empty() {
        return amount;
    }
    price_label(amount, currency.as_str(), unit.as_deref())
}

fn checkout_label(value: &Value) -> String {
    common::bool_path(value, &["checkout_enabled"])
        .map(|enabled| {
            if enabled {
                "ready".to_owned()
            } else {
                "blocked".to_owned()
            }
        })
        .unwrap_or_default()
}

fn nested<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::out::envelope::{EnvelopeContext, next_actions_from_result_value};
    use crate::out::terminal::renderer::render_terminal_document;

    use super::*;

    #[test]
    fn empty_search_renders_stable_empty_table() {
        let mut envelope = OutputEnvelope::success(
            "market.product.search",
            json!({
                "state": "empty",
                "query": "eggs",
                "count": 0,
                "relay_count": 0,
                "freshness": { "display": "missing" },
                "results": [],
                "actions": ["radroots market refresh"]
            }),
            EnvelopeContext::new("req_market_search", false),
        );
        envelope.next_actions = next_actions_from_result_value(&envelope.result);
        let document = MARKET_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Market search"));
        assert!(rendered.contains("No matching market listings"));
        assert!(rendered.contains("radroots market refresh"));
    }
}
