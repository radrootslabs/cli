use serde_json::Value;

use crate::out::envelope::OutputEnvelope;
use crate::out::terminal::layout::{TerminalDocument, TerminalSection};
use crate::out::terminal::registry::{TerminalOperationRenderer, TerminalRendererRegistry};
use crate::out::terminal::renderer::TerminalRenderContext;
use crate::out::terminal::tables::{TerminalTableColumn, TerminalTableRow};

use super::common;

pub fn register(registry: TerminalRendererRegistry) -> TerminalRendererRegistry {
    registry
        .register("account.create", &ACCOUNT_RENDERER)
        .register("account.import", &ACCOUNT_RENDERER)
        .register("account.attach_secret", &ACCOUNT_RENDERER)
        .register("account.get", &ACCOUNT_RENDERER)
        .register("account.list", &ACCOUNT_RENDERER)
        .register("account.remove", &ACCOUNT_RENDERER)
        .register("account.selection.get", &ACCOUNT_RENDERER)
        .register("account.selection.update", &ACCOUNT_RENDERER)
        .register("account.selection.clear", &ACCOUNT_RENDERER)
}

struct AccountRenderer;

static ACCOUNT_RENDERER: AccountRenderer = AccountRenderer;

impl TerminalOperationRenderer for AccountRenderer {
    fn render(&self, envelope: &OutputEnvelope, _cx: &TerminalRenderContext) -> TerminalDocument {
        if !envelope.errors.is_empty() {
            return common::base_terminal_document(envelope);
        }
        let result = common::result(envelope);
        let mut document =
            common::document_with_title(envelope, account_title(envelope.operation_id.as_str()));
        match envelope.operation_id.as_str() {
            "account.list" => add_account_list(&mut document, result),
            _ => add_account_fields(&mut document, result),
        }
        document
    }
}

fn account_title(operation_id: &str) -> &'static str {
    match operation_id {
        "account.create" => "Account created",
        "account.import" => "Account imported",
        "account.attach_secret" => "Account secret attached",
        "account.get" => "Account",
        "account.list" => "Accounts",
        "account.remove" => "Account removed",
        "account.selection.get" => "Account selection",
        "account.selection.update" => "Account selection updated",
        "account.selection.clear" => "Account selection cleared",
        _ => "Account",
    }
}

fn add_account_fields(document: &mut TerminalDocument, result: &Value) {
    for path in [
        &["account", "id"][..],
        &["account_id"][..],
        &["resolved_account", "account_id"][..],
        &["default_account", "account_id"][..],
        &["selected_account", "account_id"][..],
        &["removed_account", "id"][..],
    ] {
        if let Some(account_id) = common::string(result, path) {
            common::push_field(document, "Account", account_id);
            break;
        }
    }
    common::push_path_field(document, "Label", result, &["account", "label"]);
    common::push_path_field(document, "Label", result, &["removed_account", "label"]);
    common::push_path_field(document, "Public key", result, &["account", "public_key"]);
    common::push_path_field(
        document,
        "Public key",
        result,
        &["removed_account", "public_key"],
    );
    common::push_bool_field(
        document,
        "Write capable",
        result,
        &["account", "write_capable"],
    );
    common::push_bool_field(
        document,
        "Write capable",
        result,
        &["removed_account", "write_capable"],
    );
    common::push_path_field(document, "Source", result, &["source"]);
}

fn add_account_list(document: &mut TerminalDocument, result: &Value) {
    common::push_count_field(document, "Count", result, &["count"]);
    let rows = common::array(result, &["accounts"])
        .into_iter()
        .flatten()
        .map(|account| {
            TerminalTableRow::new(vec![
                common::string(account, &["id"]).unwrap_or_default(),
                common::string(account, &["label"]).unwrap_or_default(),
                common::string(account, &["public_key"]).unwrap_or_default(),
            ])
        })
        .collect::<Vec<_>>();
    document.sections.push(common::table_section(
        "Accounts",
        vec![
            TerminalTableColumn::new("Account", 7, 22),
            TerminalTableColumn::new("Label", 5, 18),
            TerminalTableColumn::new("Public key", 10, 24),
        ],
        rows,
        "No accounts found",
    ));
    if let Some(source) = common::string(result, &["source"]) {
        document
            .sections
            .push(TerminalSection::lines("Source", vec![source]));
    }
}
