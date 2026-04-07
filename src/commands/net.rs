use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::network;

pub fn status(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = network::net_status(config)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::NetStatus(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::NetStatus(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::NetStatus(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::NetStatus(view))
        }
    })
}
