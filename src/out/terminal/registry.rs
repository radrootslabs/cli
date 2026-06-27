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
        if self.contains(operation_id) {
            panic!("duplicate terminal renderer registration for {operation_id}");
        }
        self.entries.push(TerminalRendererEntry {
            operation_id,
            renderer,
        });
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

    pub fn operation_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|entry| entry.operation_id)
    }
}

pub fn terminal_renderer_registry() -> TerminalRendererRegistry {
    let registry = TerminalRendererRegistry::new();
    let registry = crate::out::terminal::renderers::workspace::register(registry);
    let registry = crate::out::terminal::renderers::health::register(registry);
    let registry = crate::out::terminal::renderers::config::register(registry);
    let registry = crate::out::terminal::renderers::account::register(registry);
    let registry = crate::out::terminal::renderers::runtime::register(registry);
    let registry = crate::out::terminal::renderers::store::register(registry);
    let registry = crate::out::terminal::renderers::sync::register(registry);
    let registry = crate::out::terminal::renderers::farm::register(registry);
    let registry = crate::out::terminal::renderers::listing::register(registry);
    let registry = crate::out::terminal::renderers::market::register(registry);
    let registry = crate::out::terminal::renderers::basket::register(registry);
    let registry = crate::out::terminal::renderers::trade::register(registry);
    crate::out::terminal::renderers::validation::register(registry)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::out::terminal::layout::{TerminalDocument, TerminalHeader, TerminalSymbol};
    use crate::registry::OPERATION_REGISTRY;

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
    fn registers_operation_renderers() {
        let registry = TerminalRendererRegistry::new().register("workspace.get", &TEST_RENDERER);

        assert!(registry.contains("workspace.get"));
        assert_eq!(registry.len(), 1);
        assert!(registry.get("workspace.get").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    #[should_panic(expected = "duplicate terminal renderer registration for workspace.get")]
    fn duplicate_operation_renderer_registration_fails() {
        let _registry = TerminalRendererRegistry::new()
            .register("workspace.get", &TEST_RENDERER)
            .register("workspace.get", &TEST_RENDERER);
    }

    #[test]
    fn registry_covers_registered_operations() {
        let registry = terminal_renderer_registry();
        let expected = OPERATION_REGISTRY
            .iter()
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let actual = registry.operation_ids().collect::<BTreeSet<_>>();

        assert_eq!(registry.len(), OPERATION_REGISTRY.len());
        assert_eq!(actual, expected);
        assert_eq!(registry.len(), 76);
        for operation_id in expected {
            assert!(
                registry.contains(operation_id),
                "missing terminal renderer for {operation_id}"
            );
        }
    }
}
