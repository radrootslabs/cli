use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("workspace.init", &WORKSPACE_RENDERER)
        .register("workspace.get", &WORKSPACE_RENDERER)
}

struct WorkspaceRenderer;

static WORKSPACE_RENDERER: WorkspaceRenderer = WorkspaceRenderer;

impl TerminalOperationRenderer for WorkspaceRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::result(envelope);
        let mut document = common::document_with_title(
            envelope,
            match envelope.operation_id.as_str() {
                "workspace.init" => "Workspace initialized",
                _ => "Workspace ready",
            },
        );
        common::push_path_field(&mut document, "Profile", result, &["profile"]);
        common::push_verbose_path_field(&mut document, "Config", result, &["app_config_path"]);
        common::push_verbose_path_field(&mut document, "Data", result, &["app_data_root"]);
        common::push_verbose_path_field(&mut document, "Replica", result, &["replica_db_path"]);
        document
    }
}
