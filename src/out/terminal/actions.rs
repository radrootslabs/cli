use crate::out::envelope::{NextAction, NextActionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalActionKind {
    Command,
    Setup,
    Placeholder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalAction {
    pub label: Option<String>,
    pub command: Option<String>,
    pub description: Option<String>,
    pub kind: TerminalActionKind,
}

impl TerminalAction {
    pub fn command(command: impl Into<String>) -> Self {
        Self {
            label: None,
            command: Some(command.into()),
            description: None,
            kind: TerminalActionKind::Command,
        }
    }

    pub fn setup(description: impl Into<String>) -> Self {
        Self {
            label: None,
            command: None,
            description: Some(description.into()),
            kind: TerminalActionKind::Setup,
        }
    }

    pub fn placeholder(command: impl Into<String>) -> Self {
        Self {
            label: None,
            command: Some(command.into()),
            description: None,
            kind: TerminalActionKind::Placeholder,
        }
    }

    pub fn render_line(&self) -> Option<String> {
        self.command
            .clone()
            .or_else(|| self.description.clone())
            .or_else(|| self.label.clone())
    }
}

pub fn terminal_actions_from_next_actions(actions: &[NextAction]) -> Vec<TerminalAction> {
    actions
        .iter()
        .filter_map(terminal_action_from_next_action)
        .fold(Vec::<TerminalAction>::new(), |mut unique, action| {
            if !unique.contains(&action) {
                unique.push(action);
            }
            unique
        })
}

fn terminal_action_from_next_action(action: &NextAction) -> Option<TerminalAction> {
    match action.kind {
        NextActionKind::CliCommand => action
            .command
            .as_deref()
            .map(terminal_action_from_command)
            .or_else(|| action.description.clone().map(TerminalAction::setup)),
        NextActionKind::OperatorConfig => action
            .description
            .clone()
            .map(TerminalAction::setup)
            .or_else(|| Some(TerminalAction::setup(action.label.clone()))),
    }
}

pub fn terminal_action_from_command(command: &str) -> TerminalAction {
    let command = command.trim();
    if command_requires_placeholder(command) {
        TerminalAction::placeholder(command_with_placeholder(command))
    } else {
        TerminalAction::command(command)
    }
}

fn command_requires_placeholder(command: &str) -> bool {
    matches!(
        command,
        "radroots account attach-secret" | "radroots listing publish"
    ) || basket_item_add_requires_placeholder(command)
        || basket_item_update_requires_placeholder(command)
        || basket_adjustment_add_requires_placeholder(command)
        || basket_adjustment_remove_requires_placeholder(command)
        || basket_quote_create_requires_placeholder(command)
}

fn command_with_placeholder(command: &str) -> String {
    if basket_item_add_requires_placeholder(command) {
        return basket_item_add_placeholder(command);
    }
    if basket_item_update_requires_placeholder(command) {
        return basket_item_update_placeholder(command);
    }
    if basket_adjustment_add_requires_placeholder(command) {
        return basket_adjustment_add_placeholder(command);
    }
    if basket_adjustment_remove_requires_placeholder(command) {
        return basket_adjustment_remove_placeholder(command);
    }
    if basket_quote_create_requires_placeholder(command) {
        return basket_quote_create_placeholder(command);
    }
    match command {
        "radroots account attach-secret" => {
            "radroots account attach-secret <account> <identity.json>"
        }
        "radroots listing publish" => "radroots listing publish <file>",
        other => other,
    }
    .to_owned()
}

fn basket_item_add_requires_placeholder(command: &str) -> bool {
    command_has_prefix(command, &["radroots", "basket", "item", "add"])
        && (!has_basket_arg(command, 4)
            || (!has_flag(command, "--listing") && !has_flag(command, "--listing-addr"))
            || !has_flag(command, "--bin-id"))
}

fn basket_item_update_requires_placeholder(command: &str) -> bool {
    command_has_prefix(command, &["radroots", "basket", "item", "update"])
        && (!has_basket_arg(command, 4)
            || !has_flag(command, "--item-id")
            || !has_any_flag(
                command,
                &["--listing", "--listing-addr", "--bin-id", "--quantity"],
            ))
}

fn basket_adjustment_add_requires_placeholder(command: &str) -> bool {
    command_has_prefix(command, &["radroots", "basket", "adjustment", "add"])
        && (!has_basket_arg(command, 4)
            || !has_flag(command, "--id")
            || !has_flag(command, "--effect")
            || !has_flag(command, "--amount")
            || !has_flag(command, "--currency")
            || !has_flag(command, "--reason"))
}

fn basket_adjustment_remove_requires_placeholder(command: &str) -> bool {
    command_has_prefix(command, &["radroots", "basket", "adjustment", "remove"])
        && (!has_basket_arg(command, 4) || !has_flag(command, "--id"))
}

fn basket_quote_create_requires_placeholder(command: &str) -> bool {
    command_has_prefix(command, &["radroots", "basket", "quote", "create"])
        && !has_basket_arg(command, 4)
}

fn basket_item_add_placeholder(command: &str) -> String {
    let basket = basket_arg(command, 4);
    let listing = flag_value(command, "--listing")
        .map(|value| format!("--listing {value}"))
        .or_else(|| {
            flag_value(command, "--listing-addr").map(|value| format!("--listing-addr {value}"))
        })
        .unwrap_or_else(|| "--listing <product>".to_owned());
    let bin = flag_value(command, "--bin-id").unwrap_or_else(|| "<bin>".to_owned());
    let quantity = flag_value(command, "--quantity")
        .map(|value| format!(" --quantity {value}"))
        .unwrap_or_default();
    format!("radroots basket item add {basket} {listing} --bin-id {bin}{quantity}")
}

fn basket_item_update_placeholder(command: &str) -> String {
    let basket = basket_arg(command, 4);
    let item = flag_value(command, "--item-id").unwrap_or_else(|| "<item>".to_owned());
    let update = if let Some(value) = flag_value(command, "--listing") {
        format!("--listing {value}")
    } else if let Some(value) = flag_value(command, "--listing-addr") {
        format!("--listing-addr {value}")
    } else if let Some(value) = flag_value(command, "--bin-id") {
        format!("--bin-id {value}")
    } else if let Some(value) = flag_value(command, "--quantity") {
        format!("--quantity {value}")
    } else {
        "--quantity <quantity>".to_owned()
    };
    format!("radroots basket item update {basket} --item-id {item} {update}")
}

fn basket_adjustment_add_placeholder(command: &str) -> String {
    let basket = basket_arg(command, 4);
    let id = flag_value(command, "--id").unwrap_or_else(|| "<id>".to_owned());
    let effect = flag_value(command, "--effect").unwrap_or_else(|| "<effect>".to_owned());
    let amount = flag_value(command, "--amount").unwrap_or_else(|| "<amount>".to_owned());
    let currency = flag_value(command, "--currency").unwrap_or_else(|| "<currency>".to_owned());
    let reason = flag_value(command, "--reason").unwrap_or_else(|| "<reason>".to_owned());
    format!(
        "radroots basket adjustment add {basket} --id {id} --effect {effect} --amount {amount} --currency {currency} --reason {reason}"
    )
}

fn basket_adjustment_remove_placeholder(command: &str) -> String {
    let basket = basket_arg(command, 4);
    let id = flag_value(command, "--id").unwrap_or_else(|| "<id>".to_owned());
    format!("radroots basket adjustment remove {basket} --id {id}")
}

fn basket_quote_create_placeholder(command: &str) -> String {
    let basket = basket_arg(command, 4);
    format!("radroots basket quote create {basket}")
}

fn command_has_prefix(command: &str, prefix: &[&str]) -> bool {
    let parts = command.split_whitespace().collect::<Vec<_>>();
    parts.len() >= prefix.len()
        && parts
            .iter()
            .zip(prefix.iter())
            .all(|(part, prefix)| part == prefix)
}

fn has_basket_arg(command: &str, index: usize) -> bool {
    command
        .split_whitespace()
        .nth(index)
        .is_some_and(|value| !value.starts_with("--"))
}

fn basket_arg(command: &str, index: usize) -> String {
    command
        .split_whitespace()
        .nth(index)
        .filter(|value| !value.starts_with("--"))
        .unwrap_or("<basket>")
        .to_owned()
}

fn has_any_flag(command: &str, flags: &[&str]) -> bool {
    flags.iter().any(|flag| has_flag(command, flag))
}

fn has_flag(command: &str, flag: &str) -> bool {
    flag_value(command, flag).is_some()
}

fn flag_value(command: &str, flag: &str) -> Option<String> {
    let mut parts = command.split_whitespace();
    while let Some(part) = parts.next() {
        if part == flag {
            return parts
                .next()
                .filter(|value| !value.starts_with("--"))
                .map(str::to_owned);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::out::envelope::NextAction;

    #[test]
    fn exact_command_renders_as_command() {
        let action = terminal_action_from_command("radroots store init");

        assert_eq!(action.kind, TerminalActionKind::Command);
        assert_eq!(action.command.as_deref(), Some("radroots store init"));
    }

    #[test]
    fn incomplete_command_is_placeholdered() {
        let action = terminal_action_from_command("radroots basket item add");

        assert_eq!(action.kind, TerminalActionKind::Placeholder);
        assert_eq!(
            action.command.as_deref(),
            Some("radroots basket item add <basket> --listing <product> --bin-id <bin>")
        );
    }

    #[test]
    fn partially_bound_basket_item_add_is_placeholdered() {
        let action = terminal_action_from_command("radroots basket item add basket_test");

        assert_eq!(action.kind, TerminalActionKind::Placeholder);
        assert_eq!(
            action.command.as_deref(),
            Some("radroots basket item add basket_test --listing <product> --bin-id <bin>")
        );
    }

    #[test]
    fn complete_basket_item_add_renders_as_command() {
        let action = terminal_action_from_command(
            "radroots basket item add basket_test --listing eggs --bin-id bin-1",
        );

        assert_eq!(action.kind, TerminalActionKind::Command);
        assert_eq!(
            action.command.as_deref(),
            Some("radroots basket item add basket_test --listing eggs --bin-id bin-1")
        );
    }

    #[test]
    fn converts_operator_config_action_to_setup() {
        let action = NextAction {
            kind: NextActionKind::OperatorConfig,
            label: "configure token".to_owned(),
            command: None,
            description: Some("configure RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE".to_owned()),
            env_var: Some("RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE".to_owned()),
            config_key: None,
        };

        let terminal = terminal_actions_from_next_actions(&[action]);

        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].kind, TerminalActionKind::Setup);
        assert_eq!(
            terminal[0].description.as_deref(),
            Some("configure RADROOTS_CLI_RADROOTSD_PROXY_TOKEN_FILE")
        );
    }
}
