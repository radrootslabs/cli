use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};
use crate::out::terminal::values::{relay_summary, transport_label};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("farm.create", &FARM_RENDERER)
        .register("farm.get", &FARM_RENDERER)
        .register("farm.rebind", &FARM_RENDERER)
        .register("farm.profile.update", &FARM_RENDERER)
        .register("farm.location.set", &FARM_RENDERER)
        .register("farm.location.get", &FARM_RENDERER)
        .register("farm.location.clear", &FARM_RENDERER)
        .register("farm.fulfillment.update", &FARM_RENDERER)
        .register("farm.readiness.check", &FARM_RENDERER)
        .register("farm.publish", &FARM_RENDERER)
}

struct FarmRenderer;

static FARM_RENDERER: FarmRenderer = FarmRenderer;

impl TerminalOperationRenderer for FarmRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        match envelope.operation_id.as_str() {
            "farm.location.set" | "farm.location.get" | "farm.location.clear" => {
                farm_location_document(envelope, result)
            }
            "farm.readiness.check" => farm_readiness_document(envelope, result),
            "farm.publish" => farm_publish_document(envelope, result),
            "farm.get" => farm_get_document(envelope, result),
            "farm.rebind" => farm_rebind_document(envelope, result),
            "farm.profile.update" | "farm.fulfillment.update" => {
                farm_update_document(envelope, result)
            }
            _ => farm_create_document(envelope, result),
        }
    }
}

fn farm_create_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, farm_title(envelope.operation_id.as_str()));
    push_farm_summary_fields(&mut document, result);
    document
}

fn farm_get_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Farm");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Scope", result, &["scope"]);
    common::push_path_field(&mut document, "Path", result, &["path"]);
    push_farm_document_fields(&mut document, result);
    document
}

fn farm_rebind_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Farm rebound");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Scope", result, &["scope"]);
    common::push_path_field(&mut document, "Path", result, &["path"]);
    common::push_path_field(&mut document, "From", result, &["from_seller_account_id"]);
    common::push_path_field(&mut document, "To", result, &["to_seller_account_id"]);
    common::push_bool_field(
        &mut document,
        "Seller changed",
        result,
        &["seller_pubkey_changed"],
    );
    push_farm_summary_fields(&mut document, result);
    document
}

fn farm_update_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, farm_title(envelope.operation_id.as_str()));
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Field", result, &["field"]);
    common::push_path_field(&mut document, "Value", result, &["value"]);
    push_farm_summary_fields(&mut document, result);
    document
}

fn farm_location_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, farm_title(envelope.operation_id.as_str()));
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Farm", result, &["farm_d_tag"]);
    common::push_path_field(&mut document, "Label", result, &["label"]);
    common::push_number_field(
        &mut document,
        "Latitude",
        result,
        &["exact_location", "lat"],
    );
    common::push_number_field(
        &mut document,
        "Longitude",
        result,
        &["exact_location", "lng"],
    );
    common::push_path_field(
        &mut document,
        "Locality",
        result,
        &["public_locality", "primary"],
    );
    common::push_path_field(&mut document, "City", result, &["public_locality", "city"]);
    common::push_path_field(
        &mut document,
        "Region",
        result,
        &["public_locality", "region"],
    );
    common::push_path_field(
        &mut document,
        "Country",
        result,
        &["public_locality", "country"],
    );
    common::push_count_field(&mut document, "GeoNames", result, &["geonames_feature_id"]);
    common::push_bool_field(&mut document, "Cleared", result, &["cleared"]);
    common::push_verbose_path_field(
        &mut document,
        "GeoNames DB",
        result,
        &["geonames_database_path"],
    );
    push_location_candidates(&mut document, result);
    document
}

fn farm_readiness_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document =
        common::document_with_title(envelope, common::title_for(envelope, "Farm readiness"));
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Scope", result, &["scope"]);
    common::push_path_field(&mut document, "Account", result, &["account_state"]);
    common::push_path_field(
        &mut document,
        "Listings",
        result,
        &["listing_defaults_state"],
    );
    common::push_path_field(&mut document, "Publish", result, &["publish_state"]);
    if let Some(mode) = common::string(result, &["publish_transport"]) {
        common::push_field(&mut document, "Transport", transport_label(mode.as_str()));
    }
    common::push_bool_field(&mut document, "Executable", result, &["publish_executable"]);
    push_farm_summary_fields(&mut document, result);
    push_missing_section(&mut document, result);
    document
}

fn farm_publish_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_title(envelope, "Farm published");
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Farm", result, &["farm_d_tag"]);
    common::push_path_field(&mut document, "Account", result, &["seller_account_id"]);
    common::push_path_field(&mut document, "Profile", result, &["profile", "state"]);
    common::push_path_field(&mut document, "Farm publish", result, &["farm", "state"]);
    push_publish_component_fields(
        &mut document,
        "Profile relays",
        "Profile event",
        result,
        &["profile"],
    );
    push_publish_component_fields(
        &mut document,
        "Farm relays",
        "Farm event",
        result,
        &["farm"],
    );
    push_publish_components_section(&mut document, result);
    document
}

fn push_farm_summary_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(document, "State", result, &["state"]);
    common::push_path_field(document, "Source", result, &["source"]);
    common::push_path_field(document, "Scope", result, &["config", "scope"]);
    common::push_path_field(document, "Farm", result, &["config", "farm_d_tag"]);
    common::push_path_field(document, "Name", result, &["config", "name"]);
    common::push_path_field(
        document,
        "Location",
        result,
        &["config", "location_primary"],
    );
    common::push_path_field(document, "Delivery", result, &["config", "delivery_method"]);
    common::push_path_field(document, "Path", result, &["config", "path"]);
}

fn push_farm_document_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(
        document,
        "Farm",
        result,
        &["document", "selection", "farm_d_tag"],
    );
    common::push_path_field(
        document,
        "Account",
        result,
        &["document", "selection", "seller_account_id"],
    );
    common::push_path_field(document, "Name", result, &["document", "farm", "name"]);
    common::push_path_field(
        document,
        "Display",
        result,
        &["document", "profile", "display_name"],
    );
    common::push_path_field(
        document,
        "Location",
        result,
        &["document", "listing_defaults", "location", "primary"],
    );
    common::push_path_field(
        document,
        "Delivery",
        result,
        &["document", "listing_defaults", "delivery_method"],
    );
}

fn push_location_candidates(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["candidates"])
        .into_iter()
        .flatten()
        .map(|candidate| {
            TerminalTableRow::new(vec![
                common::number_label_path(candidate, &["geonames_feature_id"]).unwrap_or_default(),
                common::string(candidate, &["display_name"]).unwrap_or_default(),
                common::number_label_path(candidate, &["exact_location", "lat"])
                    .unwrap_or_default(),
                common::number_label_path(candidate, &["exact_location", "lng"])
                    .unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "GeoNames candidates",
        vec![
            TerminalTableColumn::new("ID", 2, 10),
            TerminalTableColumn::new("Name", 8, 32),
            TerminalTableColumn::new("Lat", 3, 10),
            TerminalTableColumn::new("Lng", 3, 11),
        ],
        rows,
        "No GeoNames candidates",
    ));
}

fn push_missing_section(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["missing"])
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|value| TerminalTableRow::new(vec![value.to_owned()]))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Missing",
        vec![TerminalTableColumn::new("Item", 8, 48)],
        rows,
        "No missing requirements",
    ));
}

fn push_publish_component_fields(
    document: &mut TerminalDocument,
    relay_label: &str,
    event_label: &str,
    result: &Value,
    path: &[&str],
) {
    let Some(component) = nested_value(result, path) else {
        return;
    };
    if let Some(summary) = relay_component_summary(component) {
        common::push_field(document, relay_label, summary);
    }
    common::push_path_field(document, event_label, component, &["event_id"]);
    common::push_path_field(document, "Job", component, &["job_id"]);
}

fn push_publish_components_section(document: &mut TerminalDocument, result: &Value) {
    let rows = ["profile", "farm"]
        .into_iter()
        .filter_map(|component_name| {
            let component = result.get(component_name)?;
            Some(TerminalTableRow::new(vec![
                component_name.to_owned(),
                common::string(component, &["state"]).unwrap_or_default(),
                relay_component_summary(component).unwrap_or_default(),
            ]))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Publish components",
        vec![
            TerminalTableColumn::new("Part", 4, 8),
            TerminalTableColumn::new("State", 5, 16),
            TerminalTableColumn::new("Relays", 8, 28),
        ],
        rows,
        "No publish components",
    ));
}

fn relay_component_summary(value: &Value) -> Option<String> {
    let acknowledged = common::array(value, &["acknowledged_relays"])
        .map(Vec::len)
        .unwrap_or(0);
    let failed = common::array(value, &["failed_relays"])
        .map(Vec::len)
        .unwrap_or(0);
    let target = common::array(value, &["target_relays"])
        .map(Vec::len)
        .unwrap_or(0);
    (acknowledged > 0 || failed > 0 || target > 0)
        .then(|| relay_summary(acknowledged, failed, "acknowledged"))
}

fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn farm_title(operation_id: &str) -> &'static str {
    match operation_id {
        "farm.create" => "Farm created",
        "farm.rebind" => "Farm rebound",
        "farm.profile.update" => "Farm profile updated",
        "farm.location.set" => "Farm location set",
        "farm.location.get" => "Farm location",
        "farm.location.clear" => "Farm location cleared",
        "farm.fulfillment.update" => "Farm fulfillment updated",
        "farm.publish" => "Farm published",
        _ => "Farm",
    }
}
