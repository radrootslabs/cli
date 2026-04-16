use crate::cli::SetupArgs;
use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, SetupView, StatusView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn setup(config: &RuntimeConfig, args: &SetupArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::workflow::setup(config, args.role)?;
    Ok(setup_output(view))
}

pub fn status(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::workflow::status(config)?;
    Ok(status_output(view))
}

fn setup_output(view: SetupView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::Setup(view)),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(CommandView::Setup(view)),
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::Setup(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::Setup(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::Setup(view))
        }
    }
}

fn status_output(view: StatusView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::Status(view)),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(CommandView::Status(view)),
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::Status(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::Status(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::Status(view))
        }
    }
}
