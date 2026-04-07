use crate::cli::{OrderNewArgs, OrderSubmitArgs, OrderWatchArgs, RecordKeyArgs};
use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn new(config: &RuntimeConfig, args: &OrderNewArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::scaffold(config, args)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderNew(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderNew(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderNew(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderNew(view))
        }
    })
}

pub fn get(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::get(config, args)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderGet(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderGet(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderGet(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderGet(view))
        }
    })
}

pub fn list(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::list(config)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderList(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderList(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderList(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderList(view))
        }
    })
}

pub fn submit(
    config: &RuntimeConfig,
    args: &OrderSubmitArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::submit(config, args)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderSubmit(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderSubmit(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderSubmit(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderSubmit(view))
        }
    })
}

pub fn watch(config: &RuntimeConfig, args: &OrderWatchArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::watch(config, args)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderWatch(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderWatch(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderWatch(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderWatch(view))
        }
    })
}

pub fn cancel(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::cancel(config, args)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderCancel(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderCancel(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderCancel(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderCancel(view))
        }
    })
}

pub fn history(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::order::history(config)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::OrderHistory(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::OrderHistory(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::OrderHistory(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::OrderHistory(view))
        }
    })
}
