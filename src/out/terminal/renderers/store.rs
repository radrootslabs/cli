use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("store.init", &STORE_RENDERER)
        .register("store.status.get", &STORE_RENDERER)
        .register("store.export", &STORE_RENDERER)
        .register("store.backup.create", &STORE_RENDERER)
        .register("store.backup.restore", &STORE_RENDERER)
}

struct StoreRenderer;

static STORE_RENDERER: StoreRenderer = StoreRenderer;

impl TerminalOperationRenderer for StoreRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::result(envelope);
        let mut document =
            common::document_with_title(envelope, store_title(envelope.operation_id.as_str()));
        common::push_path_field(&mut document, "State", result, &["state"]);
        common::push_path_field(&mut document, "Source", result, &["source"]);
        common::push_path_field(&mut document, "Storage", result, &["sdk_storage"]);
        common::push_path_field(
            &mut document,
            "Derived",
            result,
            &["derived_projection", "state"],
        );
        common::push_count_field(
            &mut document,
            "Events",
            result,
            &["event_store", "total_events"],
        );
        common::push_count_field(&mut document, "Outbox", result, &["outbox", "total_events"]);
        common::push_verbose_path_field(&mut document, "Root", result, &["local_root"]);
        common::push_verbose_path_field(&mut document, "SDK root", result, &["sdk_root"]);
        common::push_verbose_path_field(&mut document, "Export", result, &["path"]);
        common::push_verbose_path_field(&mut document, "Backup", result, &["backup_path"]);
        document
    }
}

fn store_title(operation_id: &str) -> &'static str {
    match operation_id {
        "store.init" => "Store initialized",
        "store.status.get" => "Store status",
        "store.export" => "Store exported",
        "store.backup.create" => "Store backup created",
        "store.backup.restore" => "Store backup restored",
        _ => "Store",
    }
}
