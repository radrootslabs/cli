use std::fmt::Write as _;

use super::actions::TerminalAction;
use super::layout::{
    TerminalDocument, TerminalField, TerminalReference, TerminalSection, TerminalSectionBody,
    TerminalVisibility,
};
use super::tables::{TerminalTable, truncate_cell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalVerbosity {
    Quiet,
    Normal,
    Verbose,
    Trace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRenderContext {
    pub verbosity: TerminalVerbosity,
    pub width: usize,
    pub dry_run: bool,
}

impl Default for TerminalRenderContext {
    fn default() -> Self {
        Self {
            verbosity: TerminalVerbosity::Normal,
            width: 80,
            dry_run: false,
        }
    }
}

impl TerminalRenderContext {
    pub fn allows(&self, visibility: TerminalVisibility) -> bool {
        match visibility {
            TerminalVisibility::Normal => true,
            TerminalVisibility::Verbose => {
                matches!(
                    self.verbosity,
                    TerminalVerbosity::Verbose | TerminalVerbosity::Trace
                )
            }
            TerminalVisibility::Trace => matches!(self.verbosity, TerminalVerbosity::Trace),
        }
    }
}

pub fn render_terminal_document(document: &TerminalDocument, cx: &TerminalRenderContext) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "{} {}",
        document.header.symbol.glyph(),
        document.header.title
    );
    write_fields(&mut output, &document.fields, cx);
    for section in &document.sections {
        write_section(&mut output, section, cx);
    }
    if !document.warnings.is_empty() {
        let _ = writeln!(output);
        let _ = writeln!(output, "Warnings");
        for warning in &document.warnings {
            let _ = writeln!(output, "  {}: {}", warning.code, warning.message);
        }
    }
    write_next(&mut output, &document.next);
    if let Some(reference) = &document.reference
        && cx.allows(TerminalVisibility::Verbose)
        && !reference.is_empty()
    {
        write_reference(&mut output, reference);
    }
    output.trim_end_matches('\n').to_owned()
}

fn write_fields(output: &mut String, fields: &[TerminalField], cx: &TerminalRenderContext) {
    let fields = fields
        .iter()
        .filter(|field| cx.allows(field.visibility))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return;
    }
    let width = fields
        .iter()
        .map(|field| field.label.chars().count())
        .max()
        .unwrap_or(0);
    let _ = writeln!(output);
    for field in fields {
        let _ = writeln!(
            output,
            "  {label:<width$}  {value}",
            label = field.label,
            width = width,
            value = field.value
        );
    }
}

fn write_section(output: &mut String, section: &TerminalSection, cx: &TerminalRenderContext) {
    if !cx.allows(section.visibility) {
        return;
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "{}", section.title);
    match &section.body {
        TerminalSectionBody::Lines(lines) => {
            for line in lines {
                let _ = writeln!(output, "  {line}");
            }
        }
        TerminalSectionBody::Fields(fields) => write_fields(output, fields, cx),
        TerminalSectionBody::Table(table) => write_table(output, table, cx.width),
    }
}

fn write_table(output: &mut String, table: &TerminalTable, width: usize) {
    if table.rows.is_empty() {
        if let Some(empty) = &table.empty {
            let _ = writeln!(output, "  {empty}");
        }
        return;
    }
    let widths = table.column_widths(width.saturating_sub(2));
    let labels = table
        .columns
        .iter()
        .zip(widths.iter())
        .map(|(column, width)| {
            format!(
                "{:<width$}",
                truncate_cell(&column.label, *width),
                width = *width
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    let _ = writeln!(output, "  {labels}");
    for row in &table.rows {
        let line = widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                let cell = row.cells.get(index).map(String::as_str).unwrap_or("");
                format!("{:<width$}", truncate_cell(cell, *width), width = *width)
            })
            .collect::<Vec<_>>()
            .join("  ");
        let _ = writeln!(output, "  {line}");
    }
}

fn write_next(output: &mut String, actions: &[TerminalAction]) {
    let visible = actions
        .iter()
        .filter_map(|action| action.render_line())
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return;
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "Next");
    for action in visible {
        let _ = writeln!(output, "  {action}");
    }
}

fn write_reference(output: &mut String, reference: &TerminalReference) {
    let fields = [
        ("Request", reference.request_id.as_ref()),
        ("Correlation", reference.correlation_id.as_ref()),
        ("Idempotency", reference.idempotency_key.as_ref()),
        ("Event", reference.event_id.as_ref()),
        ("Address", reference.event_addr.as_ref()),
        ("Job", reference.job_id.as_ref()),
        ("Path", reference.path.as_ref()),
        ("Source", reference.source.as_ref()),
    ]
    .into_iter()
    .filter_map(|(label, value)| value.map(|value| TerminalField::new(label, value)))
    .collect::<Vec<_>>();
    let _ = writeln!(output);
    let _ = writeln!(output, "Reference");
    write_fields(output, &fields, &TerminalRenderContext::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::out::terminal::actions::TerminalAction;
    use crate::out::terminal::layout::{TerminalField, TerminalHeader, TerminalSymbol};

    #[test]
    fn renders_canonical_receipt_layout() {
        let document = TerminalDocument::new(TerminalHeader::new(
            TerminalSymbol::Success,
            "Listing published",
        ))
        .with_field(TerminalField::new("Listing", "AAAAAAAAAAAAAAAAAAAAAg"))
        .with_field(TerminalField::new("Transport", "direct nostr relay"))
        .with_field(TerminalField::new("Relays", "2 acknowledged · 0 failed"))
        .with_field(TerminalField::new("Event", "9f3a…c12"))
        .with_next(TerminalAction::command(
            "radroots listing get AAAAAAAAAAAAAAAAAAAAAg",
        ));

        assert_eq!(
            render_terminal_document(&document, &TerminalRenderContext::default()),
            "✓ Listing published\n\n  Listing    AAAAAAAAAAAAAAAAAAAAAg\n  Transport  direct nostr relay\n  Relays     2 acknowledged · 0 failed\n  Event      9f3a…c12\n\nNext\n  radroots listing get AAAAAAAAAAAAAAAAAAAAAg"
        );
    }

    #[test]
    fn hides_reference_until_verbose() {
        let mut document =
            TerminalDocument::new(TerminalHeader::new(TerminalSymbol::Success, "Ready"));
        document.reference = Some(TerminalReference {
            request_id: Some("req_test".to_owned()),
            ..TerminalReference::default()
        });

        assert_eq!(
            render_terminal_document(&document, &TerminalRenderContext::default()),
            "✓ Ready"
        );

        let cx = TerminalRenderContext {
            verbosity: TerminalVerbosity::Verbose,
            ..TerminalRenderContext::default()
        };
        assert!(render_terminal_document(&document, &cx).contains("Reference"));
    }
}
