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
        .register("transport.profile.get", &RUNTIME_RENDERER)
        .register("transport.profile.set", &RUNTIME_RENDERER)
        .register("transport.status", &RUNTIME_RENDERER)
        .register("transport.outbox.status", &RUNTIME_RENDERER)
        .register("transport.outbox.push", &RUNTIME_RENDERER)
        .register("mesh.scope.get", &RUNTIME_RENDERER)
        .register("mesh.scope.set", &RUNTIME_RENDERER)
        .register("mesh.status", &RUNTIME_RENDERER)
        .register("mesh.policy.check", &RUNTIME_RENDERER)
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
            "transport.profile.get" | "transport.profile.set" => {
                transport_profile_document(envelope, result)
            }
            "transport.status" => transport_status_document(envelope, result),
            "transport.outbox.status" => transport_outbox_status_document(envelope, result),
            "transport.outbox.push" => transport_outbox_push_document(envelope, result),
            "mesh.scope.get" | "mesh.scope.set" => mesh_scope_document(envelope, result),
            "mesh.status" | "mesh.policy.check" => mesh_status_document(envelope, result),
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

fn transport_profile_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Transport Profile");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Source", result, &["source"]);
    common::push_path_field(&mut document, "Profile", result, &["profile_id"]);
    common::push_path_field(&mut document, "Profile kind", result, &["profile_kind"]);
    common::push_path_field(&mut document, "Configured", result, &["configured_state"]);
    common::push_bool_field(
        &mut document,
        "Delivery usable",
        result,
        &["profile_delivery_usable"],
    );
    common::push_path_field(&mut document, "Message", result, &["message"]);
    common::push_path_field(
        &mut document,
        "Proxy token",
        result,
        &["proxy_token_source"],
    );
    common::push_path_field(
        &mut document,
        "Proxy token file",
        result,
        &["proxy_token_file"],
    );
    common::push_path_field(
        &mut document,
        "Proxy token secret",
        result,
        &["proxy_token_secret_id"],
    );
    if let Some(relays) = common::array(result, &["nostr_relays"]) {
        let rows = relays
            .iter()
            .map(|relay| TerminalTableRow::new(vec![relay.as_str().unwrap_or_default().to_owned()]))
            .collect::<Vec<_>>();
        document.sections.push(common::table_section(
            "Nostr relays",
            vec![TerminalTableColumn::new("URL", 12, 48)],
            rows,
            "No Nostr relays configured",
        ));
    }
    push_transport_status_table(&mut document, result, &["transport_statuses"]);
    document
}

fn transport_status_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Transport Status");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Source", result, &["source"]);
    common::push_path_field(
        &mut document,
        "Profile",
        result,
        &["active_profile", "profile_id"],
    );
    common::push_path_field(
        &mut document,
        "Profile kind",
        result,
        &["active_profile", "profile_kind"],
    );
    common::push_path_field(
        &mut document,
        "Configured",
        result,
        &["active_profile", "configured_state"],
    );
    common::push_bool_field(
        &mut document,
        "Delivery usable",
        result,
        &["active_profile", "profile_delivery_usable"],
    );
    common::push_path_field(
        &mut document,
        "Message",
        result,
        &["active_profile", "message"],
    );
    push_transport_status_table(&mut document, result, &["transports"]);
    document
}

fn push_transport_status_table(document: &mut TerminalDocument, result: &Value, path: &[&str]) {
    let rows = common::array(result, path)
        .into_iter()
        .flatten()
        .map(|transport| {
            TerminalTableRow::new(vec![
                common::string(transport, &["transport"]).unwrap_or_default(),
                common::string(transport, &["profile_id"]).unwrap_or_default(),
                common::bool_path(transport, &["configured"])
                    .map(|value| if value { "yes" } else { "no" }.to_owned())
                    .unwrap_or_default(),
                common::string(transport, &["implementation"]).unwrap_or_default(),
                common::bool_path(transport, &["usable_for_delivery"])
                    .map(|value| if value { "yes" } else { "no" }.to_owned())
                    .unwrap_or_default(),
                common::string(transport, &["endpoint_uri"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Transports",
        vec![
            TerminalTableColumn::new("Kind", 4, 12),
            TerminalTableColumn::new("Profile", 7, 18),
            TerminalTableColumn::new("Configured", 10, 10),
            TerminalTableColumn::new("Implementation", 14, 24),
            TerminalTableColumn::new("Usable", 6, 6),
            TerminalTableColumn::new("Endpoint", 8, 32),
        ],
        rows,
        "No transports reported",
    ));
}

fn transport_outbox_status_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Transport Outbox");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Profile", result, &["transport_profile"]);
    common::push_count_field(&mut document, "Total", result, &["total_count"]);
    common::push_count_field(&mut document, "Pending", result, &["pending_count"]);
    common::push_count_field(&mut document, "Retryable", result, &["retryable_count"]);
    common::push_count_field(&mut document, "Terminal", result, &["terminal_count"]);
    common::push_count_field(
        &mut document,
        "Preview unavailable",
        result,
        &["preview_unavailable_count"],
    );
    common::push_count_field(
        &mut document,
        "Deferred",
        result,
        &["deferred_until_implemented_count"],
    );
    common::push_count_field(
        &mut document,
        "Ready signed",
        result,
        &["ready_signed_count"],
    );
    common::push_count_field(&mut document, "Publishing", result, &["publishing_count"]);
    common::push_path_field(&mut document, "Last error", result, &["last_error"]);
    document
}

fn transport_outbox_push_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Transport Outbox Push");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_count_field(&mut document, "Attempted", result, &["attempted_events"]);
    common::push_count_field(&mut document, "Published", result, &["published_events"]);
    common::push_count_field(&mut document, "Retryable", result, &["retryable_events"]);
    common::push_count_field(&mut document, "Terminal", result, &["terminal_events"]);
    common::push_count_field(&mut document, "Targets", result, &["target_count"]);
    common::push_path_field(&mut document, "Reason", result, &["reason"]);
    document
}

fn mesh_scope_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Mesh Scope");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Scope", result, &["scope"]);
    common::push_path_field(&mut document, "Implementation", result, &["implementation"]);
    common::push_path_field(&mut document, "Message", result, &["message"]);
    document
}

fn mesh_status_document(envelope: &OutputEnvelope, result: &Value) -> TerminalDocument {
    let title = common::title_for(envelope, "Mesh");
    let mut document = common::document_with_title(envelope, title);
    common::push_path_field(&mut document, "State", result, &["state"]);
    common::push_path_field(&mut document, "Scope", result, &["scope"]);
    common::push_path_field(&mut document, "Transport", result, &["transport"]);
    common::push_bool_field(&mut document, "Configured", result, &["configured"]);
    common::push_path_field(&mut document, "Implementation", result, &["implementation"]);
    common::push_bool_field(&mut document, "Usable", result, &["usable_for_delivery"]);
    common::push_path_field(&mut document, "Decision", result, &["decision"]);
    common::push_path_field(&mut document, "Message", result, &["message"]);
    document
}
