use crate::cli::{ListingFileArgs, ListingNewArgs, RecordKeyArgs};
use crate::domain::runtime::{CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn new(config: &RuntimeConfig, args: &ListingNewArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::scaffold(config, args)?;
    Ok(CommandOutput::success(CommandView::ListingNew(view)))
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
        crate::domain::runtime::CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::ListingGet(view))
        }
    };
    Ok(output)
}
