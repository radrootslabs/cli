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
        "radroots account attach-secret"
            | "radroots basket item add"
            | "radroots basket item update"
            | "radroots basket adjustment add"
            | "radroots basket adjustment remove"
            | "radroots basket quote create"
            | "radroots listing publish"
    )
}

fn command_with_placeholder(command: &str) -> String {
    match command {
        "radroots account attach-secret" => "radroots account attach-secret <account> <identity.json>",
        "radroots basket item add" => {
            "radroots basket item add <basket> --listing <product> --bin-id <bin>"
        }
        "radroots basket item update" => "radroots basket item update <basket> --item-id <item>",
        "radroots basket adjustment add" => {
            "radroots basket adjustment add <basket> --id <id> --effect <effect> --amount <amount> --currency <currency> --reason <reason>"
        }
        "radroots basket adjustment remove" => "radroots basket adjustment remove <basket> --id <id>",
        "radroots basket quote create" => "radroots basket quote create <basket>",
        "radroots listing publish" => "radroots listing publish <file>",
        other => other,
    }
    .to_owned()
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
