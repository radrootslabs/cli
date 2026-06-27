use crate::out::envelope::OutputEnvelope;

use super::layout::TerminalDocument;
use super::renderer::TerminalRenderContext;

pub trait TerminalOperationRenderer {
    fn render(&self, envelope: &OutputEnvelope, cx: &TerminalRenderContext) -> TerminalDocument;
}

#[derive(Default)]
pub struct TerminalRendererRegistry {
    entries: Vec<TerminalRendererEntry>,
}

struct TerminalRendererEntry {
    operation_id: &'static str,
    renderer: &'static dyn TerminalOperationRenderer,
}

impl TerminalRendererRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn register(
        mut self,
        operation_id: &'static str,
        renderer: &'static dyn TerminalOperationRenderer,
    ) -> Self {
        if !self.contains(operation_id) {
            self.entries.push(TerminalRendererEntry {
                operation_id,
                renderer,
            });
        }
        self
    }

    pub fn contains(&self, operation_id: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.operation_id == operation_id)
    }

    pub fn get(&self, operation_id: &str) -> Option<&'static dyn TerminalOperationRenderer> {
        self.entries
            .iter()
            .find(|entry| entry.operation_id == operation_id)
            .map(|entry| entry.renderer)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::out::terminal::layout::{TerminalDocument, TerminalHeader, TerminalSymbol};

    use super::*;

    struct TestRenderer;

    impl TerminalOperationRenderer for TestRenderer {
        fn render(
            &self,
            _envelope: &OutputEnvelope,
            _cx: &TerminalRenderContext,
        ) -> TerminalDocument {
            TerminalDocument::new(TerminalHeader::new(TerminalSymbol::Success, "ok"))
        }
    }

    static TEST_RENDERER: TestRenderer = TestRenderer;

    #[test]
    fn registers_unique_operation_renderers() {
        let registry = TerminalRendererRegistry::new()
            .register("workspace.get", &TEST_RENDERER)
            .register("workspace.get", &TEST_RENDERER);

        assert!(registry.contains("workspace.get"));
        assert_eq!(registry.len(), 1);
        assert!(registry.get("workspace.get").is_some());
        assert!(registry.get("missing").is_none());
    }
}
