use crate::cli::{FarmPublishArgs, FarmScopedArgs, FarmSetupArgs};
use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, FarmGetView, FarmPublishView, FarmSetupView,
    FarmStatusView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn setup(config: &RuntimeConfig, args: &FarmSetupArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::farm::setup(config, args)?;
    Ok(farm_setup_output(view))
}

pub fn publish(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::farm::publish(config, args)?;
    Ok(farm_publish_output(view))
}

pub fn status(
    config: &RuntimeConfig,
    args: &FarmScopedArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::farm::status(config, args)?;
    Ok(farm_status_output(view))
}

pub fn get(config: &RuntimeConfig, args: &FarmScopedArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::farm::get(config, args)?;
    Ok(farm_get_output(view))
}

fn farm_publish_output(view: FarmPublishView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::FarmPublish(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::FarmPublish(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::FarmPublish(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::FarmPublish(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::FarmPublish(view))
        }
    }
}

fn farm_setup_output(view: FarmSetupView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::FarmSetup(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::FarmSetup(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::FarmSetup(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::FarmSetup(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::FarmSetup(view))
        }
    }
}

fn farm_status_output(view: FarmStatusView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::FarmStatus(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::FarmStatus(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::FarmStatus(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::FarmStatus(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::FarmStatus(view))
        }
    }
}

fn farm_get_output(view: FarmGetView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::FarmGet(view)),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(CommandView::FarmGet(view)),
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::FarmGet(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::FarmGet(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::FarmGet(view))
        }
    }
}
