use crate::cli::SyncWatchArgs;
use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn status(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::sync::status(config)?;
    Ok(output_from_disposition(
        view.disposition(),
        CommandView::SyncStatus(view),
    ))
}

pub fn pull(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::sync::pull(config)?;
    Ok(output_from_disposition(
        view.disposition(),
        CommandView::SyncPull(view),
    ))
}

pub fn push(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::sync::push(config)?;
    Ok(output_from_disposition(
        view.disposition(),
        CommandView::SyncPush(view),
    ))
}

pub fn watch(config: &RuntimeConfig, args: &SyncWatchArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::sync::watch(config, args)?;
    Ok(output_from_disposition(
        view.disposition(),
        CommandView::SyncWatch(view),
    ))
}

fn output_from_disposition(disposition: CommandDisposition, view: CommandView) -> CommandOutput {
    match disposition {
        CommandDisposition::Success => CommandOutput::success(view),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(view),
        CommandDisposition::ExternalUnavailable => CommandOutput::external_unavailable(view),
        CommandDisposition::Unsupported => CommandOutput::unsupported(view),
        CommandDisposition::InternalError => CommandOutput::internal_error(view),
    }
}
