use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::registry::OPERATION_REGISTRY;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    OPERATION_REGISTRY
        .iter()
        .fold(registry, |registry, operation| {
            registry.register(operation.operation_id, &V1_RENDERER)
        })
}

struct V1Renderer;

static V1_RENDERER: V1Renderer = V1Renderer;

impl TerminalOperationRenderer for V1Renderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        common::base_terminal_document(envelope)
    }
}
