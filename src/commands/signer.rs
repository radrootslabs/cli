use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView, SignerStatusView};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::signer::resolve_signer_status;

pub fn status(config: &RuntimeConfig) -> CommandOutput {
    let view: SignerStatusView = resolve_signer_status(config);
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SignerStatus(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::SignerStatus(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SignerStatus(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SignerStatus(view))
        }
    }
}
