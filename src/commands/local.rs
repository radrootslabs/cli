use crate::cli::{LocalBackupArgs, LocalExportArgs};
use crate::domain::runtime::{CommandDisposition, CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn init(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    Ok(CommandOutput::success(CommandView::LocalInit(
        crate::runtime::local::init(config)?,
    )))
}

pub fn status(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::local::status(config)?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::LocalStatus(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::LocalStatus(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::LocalStatus(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::LocalStatus(view))
        }
    })
}

pub fn backup(
    config: &RuntimeConfig,
    args: &LocalBackupArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::local::backup(config, args.output.as_path())?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::LocalBackup(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::LocalBackup(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::LocalBackup(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::LocalBackup(view))
        }
    })
}

pub fn export(
    config: &RuntimeConfig,
    args: &LocalExportArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::local::export(config, args.format, args.output.as_path())?;
    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::LocalExport(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::LocalExport(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::LocalExport(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::LocalExport(view))
        }
    })
}
