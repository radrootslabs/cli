use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};
use crate::out::terminal::values::relay_summary;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("validation.receipt.get", &VALIDATION_RENDERER)
        .register("validation.receipt.list", &VALIDATION_RENDERER)
        .register("validation.receipt.verify", &VALIDATION_RENDERER)
}

struct ValidationRenderer;

static VALIDATION_RENDERER: ValidationRenderer = ValidationRenderer;

impl TerminalOperationRenderer for ValidationRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        match envelope.operation_id.as_str() {
            "validation.receipt.list" => validation_receipt_list_document(envelope, result),
            "validation.receipt.verify" => validation_receipt_inspection_document(
                envelope,
                result,
                "Validation receipt verification",
            ),
            _ => validation_receipt_inspection_document(envelope, result, "Validation receipt"),
        }
    }
}

fn validation_receipt_inspection_document(
    envelope: &OutputEnvelope,
    result: &Value,
    title: &str,
) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, title);
    common::push_path_field(&mut document, "Receipt", result, &["receipt_event_id"]);
    common::push_path_field(&mut document, "Trade", result, &["order_id"]);
    common::push_path_field(&mut document, "Validation", result, &["validation_state"]);
    push_proof_fields(&mut document, result);
    common::push_path_field(&mut document, "Event", result, &["event", "id"]);
    common::push_path_field(&mut document, "Author", result, &["event", "author"]);
    common::push_path_field(&mut document, "Reason", result, &["reason"]);
    push_relay_field(&mut document, result);
    push_receipt_tag_fields(&mut document, result);
    document
}

fn validation_receipt_list_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let mut document = common::document_with_status_title(envelope, "Validation receipts");
    common::push_path_field(&mut document, "Trade", result, &["order_id"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_count_field(&mut document, "Valid", result, &["valid_count"]);
    common::push_count_field(&mut document, "Invalid", result, &["invalid_count"]);
    common::push_path_field(&mut document, "Reason", result, &["reason"]);
    push_relay_field(&mut document, result);
    let rows = common::array(result, &["receipts"])
        .into_iter()
        .flatten()
        .map(|receipt| {
            TerminalTableRow::new(vec![
                common::string(receipt, &["receipt_event_id"]).unwrap_or_default(),
                common::string(receipt, &["result"]).unwrap_or_default(),
                proof_summary_from_summary(receipt),
                common::string(receipt, &["receipt_type"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Receipts",
        vec![
            TerminalTableColumn::new("Receipt", 7, 24),
            TerminalTableColumn::new("Result", 6, 12),
            TerminalTableColumn::new("Proof", 5, 18),
            TerminalTableColumn::new("Type", 4, 18),
        ],
        rows,
        "No validation receipts found",
    ));
    push_invalid_receipts_section(&mut document, result);
    document
}

fn push_proof_fields(document: &mut TerminalDocument, result: &Value) {
    let Some(proof) = result.get("proof_verification") else {
        return;
    };
    common::push_field(document, "Proof", proof_state_label(proof));
    common::push_path_field(document, "Proof system", proof, &["proof_system"]);
    common::push_bool_field(
        document,
        "Cryptographic proof",
        proof,
        &["cryptographic_proof_verified"],
    );
    common::push_path_field(document, "Verifier", proof, &["verifier"]);
    common::push_path_field(document, "Proof mode", proof, &["mode"]);
    common::push_path_field(document, "Program", proof, &["program_hash"]);
    common::push_path_field(document, "Verifying key", proof, &["verifying_key_hash"]);
    common::push_path_field(document, "Proof reason", proof, &["reason"]);
}

fn push_receipt_tag_fields(document: &mut TerminalDocument, result: &Value) {
    common::push_path_field(
        document,
        "Receipt type",
        result,
        &["receipt_tags", "receipt_type"],
    );
    common::push_path_field(
        document,
        "Root event",
        result,
        &["receipt_tags", "root_event_id"],
    );
    common::push_path_field(
        document,
        "Target event",
        result,
        &["receipt_tags", "target_event_id"],
    );
}

fn push_invalid_receipts_section(document: &mut TerminalDocument, result: &Value) {
    let rows = common::array(result, &["invalid_receipts"])
        .into_iter()
        .flatten()
        .map(|receipt| {
            TerminalTableRow::new(vec![
                common::string(receipt, &["receipt_event_id"]).unwrap_or_default(),
                common::string(receipt, &["reason_code"]).unwrap_or_default(),
                common::string(receipt, &["reason"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return;
    }
    document.sections.push(common::table_section(
        "Invalid receipts",
        vec![
            TerminalTableColumn::new("Receipt", 7, 24),
            TerminalTableColumn::new("Code", 8, 28),
            TerminalTableColumn::new("Reason", 6, 46),
        ],
        rows,
        "No invalid validation receipts",
    ));
}

fn push_relay_field(document: &mut TerminalDocument, result: &Value) {
    let connected = array_len(result, &["connected_relays"]);
    let failed = array_len(result, &["failed_relays"]);
    if connected > 0 || failed > 0 {
        common::push_field(
            document,
            "Relays",
            relay_summary(connected, failed, "connected"),
        );
    }
}

fn proof_state_label(proof: &Value) -> String {
    if common::bool_path(proof, &["cryptographic_proof_verified"]) == Some(true) {
        return "verified".to_owned();
    }
    if common::bool_path(proof, &["cryptographic_proof_required"]) == Some(true) {
        return "unverified".to_owned();
    }
    "not required".to_owned()
}

fn proof_summary_from_summary(receipt: &Value) -> String {
    match common::string(receipt, &["proof_verification_state"]).as_deref() {
        Some("deterministic_receipt_verified") => "not required".to_owned(),
        Some("sp1_execute_checked") => "not required".to_owned(),
        Some(_) => "available".to_owned(),
        None => String::new(),
    }
}

fn array_len(value: &Value, path: &[&str]) -> usize {
    common::array(value, path).map(Vec::len).unwrap_or_default()
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
    fn receipt_list_renders_trade_id_surface() {
        let envelope = success_envelope(
            "validation.receipt.list",
            json!({
                "state": "empty",
                "order_id": "trade_test",
                "count": 0,
                "valid_count": 0,
                "invalid_count": 0,
                "receipts": [],
                "invalid_receipts": [],
                "actions": ["radroots --relay wss://relay.example.com validation receipt list --trade-id trade_test"]
            }),
        );
        let document = VALIDATION_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Validation receipts"));
        assert!(rendered.contains("--trade-id trade_test"));
        assert!(!rendered.contains("--order-id"));
    }

    #[test]
    fn proof_output_does_not_claim_verified_without_crypto_source() {
        let envelope = success_envelope(
            "validation.receipt.verify",
            json!({
                "state": "valid",
                "receipt_event_id": "receipt_test",
                "order_id": "trade_test",
                "validation_state": "valid",
                "proof_verification": {
                    "state": "sp1_reference_unresolved",
                    "proof_system": "sp1_core",
                    "cryptographic_proof_required": true,
                    "cryptographic_proof_verified": false,
                    "verifier": "radroots_cli_validation_receipt_v1"
                },
                "actions": []
            }),
        );
        let document = VALIDATION_RENDERER.render(&envelope, &TerminalRenderContext::default());
        let rendered = render_terminal_document(&document, &TerminalRenderContext::default());

        assert!(rendered.contains("Proof                unverified"));
        assert!(!rendered.contains("Proof                verified"));
    }
}
