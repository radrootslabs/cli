use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::TerminalDocument;
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("sync.status.get", &SYNC_RENDERER)
        .register("sync.pull", &SYNC_RENDERER)
        .register("sync.push", &SYNC_RENDERER)
        .register("sync.watch", &SYNC_RENDERER)
}

struct SyncRenderer;

static SYNC_RENDERER: SyncRenderer = SyncRenderer;

impl TerminalOperationRenderer for SyncRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::display_source(envelope);
        let mut document =
            common::document_with_title(envelope, sync_title(envelope.operation_id.as_str()));
        common::push_path_field(&mut document, "State", result, &["state"]);
        common::push_path_field(
            &mut document,
            "Freshness",
            result,
            &["freshness", "display"],
        );
        common::push_count_field(&mut document, "Relays", result, &["relay_count"]);
        common::push_count_field(&mut document, "Fetched", result, &["fetched_count"]);
        common::push_count_field(&mut document, "Ingested", result, &["ingested_count"]);
        common::push_count_field(&mut document, "Skipped", result, &["skipped_count"]);
        common::push_count_field(&mut document, "Failed", result, &["failed_count"]);
        common::push_count_field(
            &mut document,
            "Pending",
            result,
            &["queue", "pending_count"],
        );
        common::push_count_field(
            &mut document,
            "Ready",
            result,
            &["queue", "ready_signed_count"],
        );
        common::push_path_field(&mut document, "Reason", result, &["reason"]);
        common::push_verbose_path_field(&mut document, "Root", result, &["local_root"]);
        document
    }
}

fn sync_title(operation_id: &str) -> &'static str {
    match operation_id {
        "sync.status.get" => "Sync status",
        "sync.pull" => "Sync pull",
        "sync.push" => "Sync push",
        "sync.watch" => "Sync watch",
        _ => "Sync",
    }
}
