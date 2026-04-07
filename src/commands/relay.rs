use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::network;

pub fn list(config: &RuntimeConfig) -> CommandOutput {
    let view = network::relay_list(config);
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::RelayList(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::RelayList(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::RelayList(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::RelayList(view))
        }
    }
}
