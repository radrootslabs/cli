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
    crate::out::terminal::renderers::basket::register(registry)
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

    #[test]
    fn registry_covers_core_seller_and_buyer_operations() {
        let registry = terminal_renderer_registry();
        let expected = [
            "workspace.init",
            "workspace.get",
            "health.status.get",
            "health.check.run",
            "config.get",
            "account.create",
            "account.import",
            "account.attach_secret",
            "account.get",
            "account.list",
            "account.remove",
            "account.selection.get",
            "account.selection.update",
            "account.selection.clear",
            "signer.status.get",
            "relay.list",
            "store.init",
            "store.status.get",
            "store.export",
            "store.backup.create",
            "store.backup.restore",
            "sync.status.get",
            "sync.pull",
            "sync.push",
            "sync.watch",
            "farm.create",
            "farm.get",
            "farm.rebind",
            "farm.profile.update",
            "farm.location.set",
            "farm.location.get",
            "farm.location.clear",
            "farm.fulfillment.update",
            "farm.readiness.check",
            "farm.publish",
            "listing.create",
            "listing.get",
            "listing.list",
            "listing.app.list",
            "listing.app.export",
            "listing.update",
            "listing.validate",
            "listing.rebind",
            "listing.publish",
            "listing.archive",
            "market.refresh",
            "market.product.search",
            "market.listing.get",
            "basket.create",
            "basket.get",
            "basket.list",
            "basket.item.add",
            "basket.item.update",
            "basket.item.remove",
            "basket.adjustment.add",
            "basket.adjustment.remove",
            "basket.validate",
            "basket.quote.create",
        ];

        assert_eq!(registry.len(), expected.len());
        for operation_id in expected {
            assert!(
                registry.contains(operation_id),
                "missing terminal renderer for {operation_id}"
            );
        }
    }
}
