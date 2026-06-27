use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::actions::terminal_actions_from_next_actions;
use crate::out::terminal::errors::terminal_error_document;
use crate::out::terminal::layout::{
    TerminalDocument, TerminalField, TerminalHeader, TerminalReference, TerminalSection,
    TerminalSymbol, TerminalWarning,
};
use crate::out::terminal::tables::{TerminalTable, TerminalTableColumn, TerminalTableRow};
use crate::out::terminal::values::{proof_summary, string_path, transport_label};

pub(crate) fn base_terminal_document(envelope: &OutputEnvelope) -> TerminalDocument {
    let display = terminal_display_source(envelope);
    let mut document = if envelope.errors.is_empty() {
        let status = terminal_envelope_status(envelope);
        TerminalDocument::new(TerminalHeader::new(
            terminal_status_symbol(status, envelope.dry_run),
            terminal_title(envelope.operation_id.as_str(), status),
        ))
    } else {
        let mut document = terminal_error_document(envelope);
        add_terminal_display_fields(&mut document, display, false);
        document
    };
    if envelope.errors.is_empty() {
        add_terminal_display_fields(&mut document, display, true);
    }
    document.warnings = envelope
        .warnings
        .iter()
        .map(|warning| TerminalWarning::new(warning.code.clone(), warning.message.clone()))
        .collect();
    document.next = terminal_actions_from_next_actions(&envelope.next_actions);
    document.reference = terminal_reference(envelope);
    document
}

pub(crate) fn document_with_title(
    envelope: &OutputEnvelope,
    title: impl Into<String>,
) -> TerminalDocument {
    let mut document = base_terminal_document(envelope);
    document.header.title = title.into();
    document
}

pub(crate) fn document_with_status_title(
    envelope: &OutputEnvelope,
    title_prefix: &str,
) -> TerminalDocument {
    let status = terminal_envelope_status(envelope);
    let mut document = base_terminal_document(envelope);
    document.header.symbol = terminal_status_symbol(status, envelope.dry_run);
    document.header.title = format!("{title_prefix} {}", terminal_status_label(status));
    document
}

pub(crate) fn result(envelope: &OutputEnvelope) -> &Value {
    &envelope.result
}

pub(crate) fn display_source(envelope: &OutputEnvelope) -> &Value {
    terminal_display_source(envelope)
}

pub(crate) fn state(value: &Value) -> Option<&str> {
    terminal_state(value)
}

pub(crate) fn status_label(status: &str) -> String {
    terminal_status_label(status)
}

pub(crate) fn push_field(
    document: &mut TerminalDocument,
    label: impl Into<String>,
    value: impl Into<String>,
) {
    let label = label.into();
    let value = value.into();
    if value.trim().is_empty() {
        return;
    }
    if document
        .fields
        .iter()
        .any(|field| field.label == label && field.value == value)
    {
        return;
    }
    document.fields.push(TerminalField::new(label, value));
}

pub(crate) fn push_verbose_field(
    document: &mut TerminalDocument,
    label: impl Into<String>,
    value: impl Into<String>,
) {
    let value = value.into();
    if value.trim().is_empty() {
        return;
    }
    document.fields.push(TerminalField::verbose(label, value));
}

pub(crate) fn push_path_field(
    document: &mut TerminalDocument,
    label: &str,
    value: &Value,
    path: &[&str],
) {
    if let Some(value) = string_path(value, path) {
        push_field(document, label, value);
    }
}

pub(crate) fn push_verbose_path_field(
    document: &mut TerminalDocument,
    label: &str,
    value: &Value,
    path: &[&str],
) {
    if let Some(value) = string_path(value, path) {
        push_verbose_field(document, label, value);
    }
}

pub(crate) fn push_count_field(
    document: &mut TerminalDocument,
    label: &str,
    value: &Value,
    path: &[&str],
) {
    if let Some(value) = number_label_path(value, path) {
        push_field(document, label, value);
    }
}

pub(crate) fn push_number_field(
    document: &mut TerminalDocument,
    label: &str,
    value: &Value,
    path: &[&str],
) {
    if let Some(value) = number_label_path(value, path) {
        push_field(document, label, value);
    }
}

pub(crate) fn push_bool_field(
    document: &mut TerminalDocument,
    label: &str,
    value: &Value,
    path: &[&str],
) {
    if let Some(value) = bool_path(value, path) {
        push_field(document, label, if value { "yes" } else { "no" });
    }
}

pub(crate) fn table_section(
    title: &str,
    columns: Vec<TerminalTableColumn>,
    rows: Vec<TerminalTableRow>,
    empty: &str,
) -> TerminalSection {
    let mut table = TerminalTable::new(columns).with_empty(empty);
    for row in rows {
        table = table.with_row(row);
    }
    TerminalSection::table(title, table)
}

pub(crate) fn string(value: &Value, path: &[&str]) -> Option<String> {
    string_path(value, path).map(str::to_owned)
}

pub(crate) fn array<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_array()
}

pub(crate) fn number_path(value: &Value, path: &[&str]) -> Option<i64> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_i64()
}

pub(crate) fn number_label_path(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    number_label(current)
}

pub(crate) fn number_label(value: &Value) -> Option<String> {
    if let Some(value) = value.as_i64() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_u64() {
        return Some(value.to_string());
    }
    value
        .as_f64()
        .filter(|value| value.is_finite())
        .map(|value| {
            let mut rendered = format!("{value:.6}");
            while rendered.contains('.') && rendered.ends_with('0') {
                rendered.pop();
            }
            if rendered.ends_with('.') {
                rendered.pop();
            }
            rendered
        })
}

pub(crate) fn bool_path(value: &Value, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_bool()
}

pub(crate) fn title_for(envelope: &OutputEnvelope, noun: &str) -> String {
    let status = terminal_envelope_status(envelope);
    format!("{noun} {}", terminal_status_label(status))
}

fn add_terminal_display_fields(
    document: &mut TerminalDocument,
    display: &Value,
    include_reason: bool,
) {
    if let Some(state) = terminal_state(display) {
        push_field(document, "State", terminal_status_label(state));
    }
    if let Some(mode) = terminal_publish_transport(display) {
        push_field(document, "Transport", transport_label(mode));
    }
    if let Some(state) = terminal_publish_state(display) {
        push_field(document, "Publish", terminal_status_label(state));
    }
    if let Some(proof) = proof_summary(display) {
        push_field(document, "Proof", proof);
    }
    if include_reason && let Some(reason) = terminal_reason(display) {
        push_field(document, "Reason", reason.to_owned());
    }
}

fn terminal_reference(envelope: &OutputEnvelope) -> Option<TerminalReference> {
    let reference = TerminalReference {
        request_id: Some(envelope.request_id.clone()),
        correlation_id: envelope.correlation_id.clone(),
        idempotency_key: envelope.idempotency_key.clone(),
        event_id: None,
        event_addr: None,
        job_id: None,
        path: None,
        source: None,
    };
    (!reference.is_empty()).then_some(reference)
}

fn terminal_display_source(envelope: &OutputEnvelope) -> &Value {
    if !envelope.result.is_null() {
        return &envelope.result;
    }
    envelope
        .errors
        .first()
        .and_then(|error| error.detail.as_ref())
        .unwrap_or(&envelope.result)
}

fn terminal_state(result: &Value) -> Option<&str> {
    string_path(result, &["state"])
}

fn terminal_publish_transport(result: &Value) -> Option<&str> {
    string_path(result, &["publish", "mode"])
        .or_else(|| string_path(result, &["checks", "publish", "mode"]))
        .or_else(|| string_path(result, &["publish_transport"]))
}

fn terminal_publish_state(result: &Value) -> Option<&str> {
    string_path(result, &["publish", "state"])
        .or_else(|| string_path(result, &["checks", "publish", "state"]))
        .or_else(|| string_path(result, &["publish_state"]))
}

fn terminal_reason(result: &Value) -> Option<&str> {
    string_path(result, &["reason"])
        .or_else(|| string_path(result, &["publish", "reason"]))
        .or_else(|| string_path(result, &["checks", "publish", "reason"]))
        .or_else(|| string_path(result, &["store", "reason"]))
        .or_else(|| string_path(result, &["checks", "store", "reason"]))
        .or_else(|| string_path(result, &["checks", "account", "reason"]))
}

fn terminal_envelope_status(envelope: &OutputEnvelope) -> &str {
    if !envelope.errors.is_empty() {
        return "error";
    }
    if let Some(state) = envelope
        .result
        .get("state")
        .and_then(|value| value.as_str())
    {
        return state;
    }
    if envelope.dry_run {
        return "dry_run";
    }
    "ok"
}

fn terminal_status_symbol(status: &str, dry_run: bool) -> TerminalSymbol {
    if dry_run || status == "dry_run" {
        return TerminalSymbol::Neutral;
    }
    if terminal_status_needs_attention(status) {
        TerminalSymbol::Attention
    } else {
        TerminalSymbol::Success
    }
}

fn terminal_status_needs_attention(status: &str) -> bool {
    matches!(
        status,
        "needs_attention"
            | "unconfigured"
            | "unavailable"
            | "blocked"
            | "invalid"
            | "not_ready"
            | "partial"
            | "degraded"
            | "failed"
            | "conflict"
            | "missing"
    )
}

fn terminal_title(operation_id: &str, status: &str) -> String {
    format!(
        "{} {}",
        operation_title(operation_id),
        terminal_status_label(status)
    )
}

fn operation_title(operation_id: &str) -> String {
    let mut words = operation_id
        .split('.')
        .flat_map(|part| part.split('_'))
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(first) = words.first_mut() {
        *first = capitalize_ascii_word(first);
    }
    words.join(" ")
}

fn terminal_status_label(status: &str) -> String {
    status
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_ascii_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut rendered = String::new();
    rendered.push(first.to_ascii_uppercase());
    rendered.extend(chars);
    rendered
}
