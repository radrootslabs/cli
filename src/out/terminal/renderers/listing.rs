use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};
use crate::out::terminal::values::{proof_summary, relay_summary, transport_label};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("listing.create", &LISTING_RENDERER)
        .register("listing.get", &LISTING_RENDERER)
        .register("listing.list", &LISTING_RENDERER)
        .register("listing.app.list", &LISTING_RENDERER)
        .register("listing.app.export", &LISTING_RENDERER)
        .register("listing.update", &LISTING_RENDERER)
        .register("listing.validate", &LISTING_RENDERER)
        .register("listing.rebind", &LISTING_RENDERER)
        .register("listing.publish", &LISTING_RENDERER)
        .register("listing.archive", &LISTING_RENDERER)
}

struct ListingRenderer;

static LISTING_RENDERER: ListingRenderer = ListingRenderer;

impl TerminalOperationRenderer for ListingRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        match envelope.operation_id.as_str() {
            "listing.list" => listing_list_document(envelope, result),
            "listing.app.list" => listing_app_list_document(envelope, result),
            "listing.app.export" => listing_app_export_document(envelope, result),
            "listing.validate" => listing_validate_document(envelope, result),
            "listing.rebind" => listing_rebind_document(envelope, result),
            "listing.publish" => listing_publish_document(envelope, result),
            "listing.archive" | "listing.update" => listing_mutation_document(envelope, result),
            "listing.get" => listing_get_document(envelope, result),
            _ => listing_create_document(envelope, result),
        }
    }
}

fn listing_create_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listing draft created");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "Key", result, &["key"]);
    common::push_path_field(&mut document, "Farm", result, &["farm_d_tag"]);
    common::push_path_field(&mut document, "Delivery", result, &["delivery_method"]);
    common::push_path_field(&mut document, "Location", result, &["location_primary"]);
    common::push_path_field(&mut document, "File", result, &["file"]);
    document
}

fn listing_get_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listing");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "Key", result, &["product_key"]);
    common::push_path_field(&mut document, "Title", result, &["title"]);
    common::push_path_field(&mut document, "Category", result, &["category"]);
    common::push_path_field(&mut document, "Location", result, &["location_primary"]);
    common::push_path_field(&mut document, "Available", result, &["available", "label"]);
    common::push_path_field(&mut document, "Currency", result, &["price", "currency"]);
    common::push_number_field(&mut document, "Price", result, &["price", "amount"]);
    document
}

fn listing_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listings");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_verbose_path_field(&mut document, "Drafts", result, &["draft_dir"]);
    let rows = common::array(result, &["listings"])
        .into_iter()
        .flatten()
        .map(|listing| {
            TerminalTableRow::new(vec![
                common::string(listing, &["id"]).unwrap_or_default(),
                common::string(listing, &["title"])
                    .or_else(|| common::string(listing, &["product_key"]))
                    .unwrap_or_default(),
                common::string(listing, &["state"]).unwrap_or_default(),
                common::string(listing, &["file"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Listings",
        vec![
            TerminalTableColumn::new("Listing", 8, 22),
            TerminalTableColumn::new("Title", 5, 24),
            TerminalTableColumn::new("State", 5, 12),
            TerminalTableColumn::new("File", 4, 34),
        ],
        rows,
        "No listing drafts found",
    ));
    document
}

fn listing_app_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listing app records");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_bool_field(&mut document, "More", result, &["has_more"]);
    let rows = common::array(result, &["records"])
        .into_iter()
        .flatten()
        .map(|record| {
            TerminalTableRow::new(vec![
                common::string(record, &["record_id"]).unwrap_or_default(),
                common::string(record, &["title"])
                    .or_else(|| common::string(record, &["listing_id"]))
                    .unwrap_or_default(),
                common::string(record, &["status"]).unwrap_or_default(),
                common::bool_path(record, &["exportable"])
                    .map(|value| if value { "yes" } else { "no" }.to_owned())
                    .unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Records",
        vec![
            TerminalTableColumn::new("Record", 8, 22),
            TerminalTableColumn::new("Listing", 8, 24),
            TerminalTableColumn::new("Status", 6, 14),
            TerminalTableColumn::new("Export", 6, 6),
        ],
        rows,
        "No app-authored listing records found",
    ));
    document
}

fn listing_app_export_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listing app export");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Record", result, &["record_id"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "Farm", result, &["farm_d_tag"]);
    common::push_bool_field(&mut document, "Valid", result, &["valid"]);
    common::push_path_field(&mut document, "File", result, &["file"]);
    push_issues_section(&mut document, result);
    document
}

fn listing_validate_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = if common::bool_path(result, &["valid"]) == Some(true) {
        "Listing valid"
    } else {
        "Listing validation"
    };
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_bool_field(&mut document, "Valid", result, &["valid"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "Farm", result, &["farm_d_tag"]);
    common::push_path_field(&mut document, "File", result, &["file"]);
    push_issues_section(&mut document, result);
    document
}

fn listing_rebind_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listing rebound");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "From", result, &["from_seller_account_id"]);
    common::push_path_field(&mut document, "To", result, &["to_seller_account_id"]);
    common::push_path_field(&mut document, "Farm", result, &["to_farm_d_tag"]);
    common::push_bool_field(
        &mut document,
        "Seller changed",
        result,
        &["seller_pubkey_changed"],
    );
    common::push_bool_field(
        &mut document,
        "Address changed",
        result,
        &["listing_addr_changed"],
    );
    common::push_path_field(&mut document, "File", result, &["file"]);
    document
}

fn listing_mutation_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, listing_title(envelope.operation_id.as_str()));
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Listing", result, &["listing_id"]);
    common::push_path_field(&mut document, "Address", result, &["listing_addr"]);
    common::push_path_field(&mut document, "Account", result, &["seller_account_id"]);
    common::push_path_field(&mut document, "Event", result, &["event_id"]);
    common::push_path_field(&mut document, "File", result, &["file"]);
    push_relay_field(&mut document, result);
    document
}

fn listing_publish_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Listing published");
    if let Some(listing) = first_string(
        result,
        &[
            &["listing_id"],
            &["listing", "id"],
            &["listing", "d_tag"],
            &["resource", "id"],
            &["listing_addr"],
            &["listing_address"],
        ],
    ) {
        common::push_field(&mut document, "Listing", listing);
    }
    if let Some(mode) = first_string(
        result,
        &[
            &["publish", "mode"],
            &["publish_transport"],
            &["transport"],
            &["source"],
        ],
    ) {
        common::push_field(&mut document, "Transport", transport_label(mode.as_str()));
    }
    push_relay_field(&mut document, result);
    if let Some(event_id) = first_string(
        result,
        &[
            &["event_id"],
            &["publish", "event_id"],
            &["event", "id"],
            &["event", "event_id"],
            &["receipt", "event_id"],
        ],
    ) {
        common::push_field(&mut document, "Event", event_id);
    }
    if let Some(proof) = proof_summary(result) {
        common::push_field(&mut document, "Proof", proof);
    }
    document
}

fn push_issues_section(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["issues"])
        .into_iter()
        .flatten()
        .map(|issue| {
            TerminalTableRow::new(vec![
                common::string(issue, &["field"]).unwrap_or_default(),
                common::string(issue, &["message"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Issues",
        vec![
            TerminalTableColumn::new("Field", 5, 18),
            TerminalTableColumn::new("Message", 7, 54),
        ],
        rows,
        "No validation issues",
    ));
}

fn push_relay_field(document: &mut TerminalDocument, result: &Value) {
    if let Some(summary) = relay_field_summary(result) {
        common::push_field(document, "Relays", summary);
    }
}

fn relay_field_summary(result: &Value) -> Option<String> {
    let acknowledged = common::array(result, &["acknowledged_relays"])
        .map(Vec::len)
        .or_else(|| {
            common::number_path(result, &["publish", "acknowledged_count"])
                .and_then(|value| usize::try_from(value).ok())
        })
        .or_else(|| {
            common::number_path(result, &["receipt", "acknowledged_count"])
                .and_then(|value| usize::try_from(value).ok())
        })
        .unwrap_or(0);
    let failed = common::array(result, &["failed_relays"])
        .map(Vec::len)
        .or_else(|| {
            common::number_path(result, &["publish", "failed_count"])
                .and_then(|value| usize::try_from(value).ok())
        })
        .or_else(|| {
            common::number_path(result, &["receipt", "failed_count"])
                .and_then(|value| usize::try_from(value).ok())
        })
        .unwrap_or(0);
    let target = common::array(result, &["target_relays"])
        .map(Vec::len)
        .unwrap_or(0);
    (acknowledged > 0 || failed > 0 || target > 0)
        .then(|| relay_summary(acknowledged, failed, "acknowledged"))
}

fn first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| common::string(value, path))
}

fn listing_title(operation_id: &str) -> &'static str {
    match operation_id {
        "listing.update" => "Listing updated",
        "listing.archive" => "Listing archived",
        _ => "Listing",
    }
}

#[cfg(test)]
mod tests {
    use crate::out::envelope::EnvelopeContext;
    use crate::out::terminal::renderer::render_terminal_document;
    use serde_json::json;

    use super::*;

    #[test]
    fn publish_receipt_omits_absent_proof_source() {
        let envelope = OutputEnvelope::success(
            "listing.publish",
            json!({
                "state": "published",
                "listing_id": "AAAAAAAAAAAAAAAAAAAAAg",
                "source": "SDK listing publish · configured signer",
                "acknowledged_relays": ["wss://relay.example"],
                "failed_relays": [],
                "event_id": "9f3ac129f3ac129f3ac129f3ac129f3ac129f3ac129f3ac129f3ac129f3ac12",
                "actions": ["radroots listing get AAAAAAAAAAAAAAAAAAAAAg"]
            }),
            EnvelopeContext::new("req_listing_publish", false),
        );
        let document = LISTING_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("✓ Listing published"));
        assert!(rendered.contains("Listing"));
        assert!(rendered.contains("Relays"));
        assert!(rendered.contains("Event"));
        assert!(!rendered.contains("Proof"));
        assert!(!rendered.contains("verified"));
    }

    #[test]
    fn publish_receipt_requires_verified_true_for_verified_claim() {
        let envelope = OutputEnvelope::success(
            "listing.publish",
            json!({
                "state": "published",
                "listing_id": "AAAAAAAAAAAAAAAAAAAAAg",
                "proof_verification": {
                    "state": "valid",
                    "cryptographic_proof_verified": false,
                    "proof_system": "SP1"
                }
            }),
            EnvelopeContext::new("req_listing_publish", false),
        );
        let document = LISTING_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Proof"));
        assert!(!rendered.contains("Proof verified"));
        assert!(!rendered.contains("verified ·"));
    }
}
