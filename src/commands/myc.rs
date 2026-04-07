use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView, MycStatusView};
use crate::runtime::config::RuntimeConfig;

pub fn status(config: &RuntimeConfig) -> CommandOutput {
    let view: MycStatusView = crate::runtime::myc::resolve_status(&config.myc);
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::MycStatus(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::MycStatus(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::MycStatus(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::MycStatus(view))
        }
    }
}
