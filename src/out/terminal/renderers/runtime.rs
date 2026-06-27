use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("signer.status.get", &RUNTIME_RENDERER)
        .register("relay.list", &RUNTIME_RENDERER)
}

struct RuntimeRenderer;

static RUNTIME_RENDERER: RuntimeRenderer = RuntimeRenderer;

impl TerminalOperationRenderer for RuntimeRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::result(envelope);
        match envelope.operation_id.as_str() {
            "relay.list" => relay_document(envelope, result),
            _ => signer_document(envelope, result),
        }
    }
}

fn signer_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Signer");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "Mode", result, &["mode"]);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(
        &mut document,
        "Account",
        result,
        &["account_resolution", "status"],
    );
    common::push_path_field(&mut document, "Binding", result, &["binding", "state"]);
    common::push_path_field(&mut document, "Reason", result, &["reason"]);
    if let Some(write_kinds) = common::array(result, &["write_kinds"]) {
        let rows = write_kinds
            .iter()
            .map(|write_kind| {
                TerminalTableRow::new(vec![
                    common::string(write_kind, &["command"]).unwrap_or_default(),
                    common::string(write_kind, &["event_kind"]).unwrap_or_else(|| {
                        write_kind
                            .get("event_kind")
                            .and_then(Value::as_i64)
                            .map(|value| value.to_string())
                            .unwrap_or_default()
                    }),
                    common::bool_path(write_kind, &["ready"])
                        .map(|ready| if ready { "ready" } else { "blocked" }.to_owned())
                        .unwrap_or_default(),
                ])
            })
            .collect::<Vec<_>>();
        document.sections.push(common::table_section(
            "Write permissions",
            vec![
                TerminalTableColumn::new("Command", 7, 24),
                TerminalTableColumn::new("Kind", 4, 8),
                TerminalTableColumn::new("State", 5, 8),
            ],
            rows,
            "No signer write permissions reported",
        ));
    }
    document
}

fn relay_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Relays");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Count", result, &["count"]);
    common::push_path_field(&mut document, "Source", result, &["source"]);
    let rows = common::array(result, &["relays"])
        .into_iter()
        .flatten()
        .map(|relay| {
            TerminalTableRow::new(vec![
                common::string(relay, &["url"]).unwrap_or_default(),
                common::bool_path(relay, &["read"])
                    .map(|value| if value { "yes" } else { "no" }.to_owned())
                    .unwrap_or_default(),
                common::bool_path(relay, &["write"])
                    .map(|value| if value { "yes" } else { "no" }.to_owned())
                    .unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Relays",
        vec![
            TerminalTableColumn::new("URL", 12, 42),
            TerminalTableColumn::new("Read", 4, 4),
            TerminalTableColumn::new("Write", 5, 5),
        ],
        rows,
        "No relays configured",
    ));
    document
}
