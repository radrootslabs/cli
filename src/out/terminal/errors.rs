use crate::out::envelope::{OutputEnvelope, OutputError};

use super::actions::terminal_actions_from_next_actions;
use super::layout::{TerminalDocument, TerminalField, TerminalHeader, TerminalSymbol};

pub fn terminal_error_document(envelope: &OutputEnvelope) -> TerminalDocument {
    let error = envelope.errors.first();
    let title = error
        .map(error_title)
        .unwrap_or_else(|| "Command failed".to_owned());
    let mut document = TerminalDocument::new(TerminalHeader::new(TerminalSymbol::Failure, title));
    if let Some(error) = error {
        document
            .fields
            .push(TerminalField::new("Reason", error.message.clone()));
        document
            .fields
            .push(TerminalField::verbose("Code", error.reason_code.clone()));
    }
    document.next = terminal_actions_from_next_actions(&envelope.next_actions);
    document
}

fn error_title(error: &OutputError) -> String {
    match error.code.as_str() {
        "not_found" => "Not found",
        "invalid_input" => "Invalid input",
        "approval_required" => "Approval required",
        "operation_unavailable" => "Unavailable",
        _ => "Command failed",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::out::envelope::{CliExitCode, EnvelopeContext, OutputEnvelope, OutputError};

    use super::*;

    #[test]
    fn builds_terminal_error_document() {
        let error = OutputError::new("invalid_input", "missing input", CliExitCode::InvalidInput);
        let envelope = OutputEnvelope::failure(
            "trade.submit",
            error,
            EnvelopeContext::new("req_test", false),
        );
        let document = terminal_error_document(&envelope);

        assert_eq!(document.header.title, "Invalid input");
        assert_eq!(document.fields[0].label, "Reason");
        assert_eq!(document.fields[0].value, "missing input");
    }
}
