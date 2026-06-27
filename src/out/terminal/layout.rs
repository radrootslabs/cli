use super::actions::TerminalAction;
use super::tables::TerminalTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSymbol {
    Success,
    Neutral,
    Attention,
    Failure,
}

impl TerminalSymbol {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Success => "✓",
            Self::Neutral => "◌",
            Self::Attention => "!",
            Self::Failure => "✕",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalVisibility {
    Normal,
    Verbose,
    Trace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalHeader {
    pub symbol: TerminalSymbol,
    pub title: String,
}

impl TerminalHeader {
    pub fn new(symbol: TerminalSymbol, title: impl Into<String>) -> Self {
        Self {
            symbol,
            title: title.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalField {
    pub label: String,
    pub value: String,
    pub visibility: TerminalVisibility,
}

impl TerminalField {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            visibility: TerminalVisibility::Normal,
        }
    }

    pub fn verbose(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            visibility: TerminalVisibility::Verbose,
        }
    }

    pub fn trace(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            visibility: TerminalVisibility::Trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalSectionBody {
    Lines(Vec<String>),
    Fields(Vec<TerminalField>),
    Table(TerminalTable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSection {
    pub title: String,
    pub body: TerminalSectionBody,
    pub visibility: TerminalVisibility,
}

impl TerminalSection {
    pub fn lines(title: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            title: title.into(),
            body: TerminalSectionBody::Lines(lines),
            visibility: TerminalVisibility::Normal,
        }
    }

    pub fn fields(title: impl Into<String>, fields: Vec<TerminalField>) -> Self {
        Self {
            title: title.into(),
            body: TerminalSectionBody::Fields(fields),
            visibility: TerminalVisibility::Normal,
        }
    }

    pub fn table(title: impl Into<String>, table: TerminalTable) -> Self {
        Self {
            title: title.into(),
            body: TerminalSectionBody::Table(table),
            visibility: TerminalVisibility::Normal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalWarning {
    pub code: String,
    pub message: String,
}

impl TerminalWarning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TerminalReference {
    pub request_id: Option<String>,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub event_id: Option<String>,
    pub event_addr: Option<String>,
    pub job_id: Option<String>,
    pub path: Option<String>,
    pub source: Option<String>,
}

impl TerminalReference {
    pub fn is_empty(&self) -> bool {
        self.request_id.is_none()
            && self.correlation_id.is_none()
            && self.idempotency_key.is_none()
            && self.event_id.is_none()
            && self.event_addr.is_none()
            && self.job_id.is_none()
            && self.path.is_none()
            && self.source.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalDocument {
    pub header: TerminalHeader,
    pub fields: Vec<TerminalField>,
    pub sections: Vec<TerminalSection>,
    pub warnings: Vec<TerminalWarning>,
    pub next: Vec<TerminalAction>,
    pub reference: Option<TerminalReference>,
}

impl TerminalDocument {
    pub fn new(header: TerminalHeader) -> Self {
        Self {
            header,
            fields: Vec::new(),
            sections: Vec::new(),
            warnings: Vec::new(),
            next: Vec::new(),
            reference: None,
        }
    }

    pub fn with_field(mut self, field: TerminalField) -> Self {
        self.fields.push(field);
        self
    }

    pub fn with_section(mut self, section: TerminalSection) -> Self {
        self.sections.push(section);
        self
    }

    pub fn with_next(mut self, action: TerminalAction) -> Self {
        self.next.push(action);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_match_terminal_contract() {
        assert_eq!(TerminalSymbol::Success.glyph(), "✓");
        assert_eq!(TerminalSymbol::Neutral.glyph(), "◌");
        assert_eq!(TerminalSymbol::Attention.glyph(), "!");
        assert_eq!(TerminalSymbol::Failure.glyph(), "✕");
    }

    #[test]
    fn document_defaults_to_empty_body() {
        let document = TerminalDocument::new(TerminalHeader::new(
            TerminalSymbol::Success,
            "Workspace ready",
        ));

        assert_eq!(document.header.title, "Workspace ready");
        assert!(document.fields.is_empty());
        assert!(document.sections.is_empty());
        assert!(document.warnings.is_empty());
        assert!(document.next.is_empty());
        assert!(document.reference.is_none());
    }
}
