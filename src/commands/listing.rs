use crate::cli::{ListingFileArgs, ListingMutationArgs, ListingNewArgs, RecordKeyArgs};
use crate::domain::runtime::{CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn new(config: &RuntimeConfig, args: &ListingNewArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::scaffold(config, args)?;
    Ok(match view.disposition() {
        crate::domain::runtime::CommandDisposition::Success => {
            CommandOutput::success(CommandView::ListingNew(view))
        }
        crate::domain::runtime::CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::ListingNew(view))
        }
        crate::domain::runtime::CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::ListingNew(view))
        }
        crate::domain::runtime::CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::ListingNew(view))
        }
        crate::domain::runtime::CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::ListingNew(view))
        }
    })
}

pub fn validate(
    config: &RuntimeConfig,
    args: &ListingFileArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::validate(config, args)?;
    Ok(CommandOutput::success(CommandView::ListingValidate(view)))
}

pub fn get(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::get(config, args)?;
    let output = match view.disposition() {
        crate::domain::runtime::CommandDisposition::Success => {
            CommandOutput::success(CommandView::ListingGet(view))
        }
        crate::domain::runtime::CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::ListingGet(view))
        }
        crate::domain::runtime::CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::ListingGet(view))
        }
        crate::domain::runtime::CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::ListingGet(view))
        }
        crate::domain::runtime::CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::ListingGet(view))
        }
    };
    Ok(output)
}

pub fn publish(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::publish(config, args)?;
    Ok(match view.disposition() {
        crate::domain::runtime::CommandDisposition::Success => {
            CommandOutput::success(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::ListingMutation(view))
        }
    })
}

pub fn update(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::update(config, args)?;
    Ok(match view.disposition() {
        crate::domain::runtime::CommandDisposition::Success => {
            CommandOutput::success(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::ListingMutation(view))
        }
    })
}

pub fn archive(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::archive(config, args)?;
    Ok(match view.disposition() {
        crate::domain::runtime::CommandDisposition::Success => {
            CommandOutput::success(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::ListingMutation(view))
        }
        crate::domain::runtime::CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::ListingMutation(view))
        }
    })
}
