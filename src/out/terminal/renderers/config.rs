use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::{TerminalDocument, TerminalSymbol};
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::values::transport_label;

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry.register("config.get", &CONFIG_RENDERER)
}

struct ConfigRenderer;

static CONFIG_RENDERER: ConfigRenderer = ConfigRenderer;

impl TerminalOperationRenderer for ConfigRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::generic_terminal_document(envelope);
        }
        let result = common::result(envelope);
        let publish_state =
            common::string(result, &["publish", "state"]).unwrap_or_else(|| "ready".to_owned());
        let mut document = common::document_with_title(
            envelope,
            format!("Config {}", common::status_label(publish_state.as_str())),
        );
        if !matches!(publish_state.as_str(), "ready" | "configured" | "ok") {
            document.header.symbol = TerminalSymbol::Attention;
        }
        common::push_path_field(&mut document, "Output", result, &["output", "format"]);
        if let Some(transport) = common::string(result, &["publish", "transport"]) {
            common::push_field(
                &mut document,
                "Transport",
                transport_label(transport.as_str()),
            );
        }
        common::push_path_field(&mut document, "Publish", result, &["publish", "state"]);
        common::push_path_field(&mut document, "Signer", result, &["signer", "mode"]);
        common::push_path_field(
            &mut document,
            "Account",
            result,
            &["account_resolution", "status"],
        );
        common::push_count_field(&mut document, "Relays", result, &["relay", "count"]);
        common::push_verbose_path_field(&mut document, "Profile", result, &["paths", "profile"]);
        common::push_verbose_path_field(
            &mut document,
            "Config",
            result,
            &["paths", "app_config_path"],
        );
        document
    }
}
