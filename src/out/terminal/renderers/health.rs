use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("health.status.get", &HEALTH_RENDERER)
        .register("health.check.run", &HEALTH_RENDERER)
}

struct HealthRenderer;

static HEALTH_RENDERER: HealthRenderer = HealthRenderer;

impl TerminalOperationRenderer for HealthRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::result(envelope);
        let title = match envelope.operation_id.as_str() {
            "health.check.run" => common::title_for(envelope, "Health check"),
            _ => common::title_for(envelope, "Health"),
        };
        let mut document = common::document_with_title(envelope, title);
        common::push_path_field(
            &mut document,
            "Account",
            result,
            &["checks", "account", "state"],
        );
        common::push_path_field(
            &mut document,
            "Store",
            result,
            &["checks", "store", "state"],
        );
        common::push_path_field(
            &mut document,
            "Publish",
            result,
            &["checks", "publish", "state"],
        );
        common::push_path_field(
            &mut document,
            "Network",
            result,
            &["checks", "network", "state"],
        );
        document
    }
}
