use crate::cli::FindArgs;
use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn search(config: &RuntimeConfig, args: &FindArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::find::search(config, args)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::Find(view)),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(CommandView::Find(view)),
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::Find(view))
        }
        CommandDisposition::InternalError => CommandOutput::internal_error(CommandView::Find(view)),
    })
}
