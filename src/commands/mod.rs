pub mod identity;
pub mod myc;
pub mod runtime;
pub mod signer;

use crate::cli::{Command, IdentityCommand, MycCommand, RuntimeCommand, SignerCommand};
use crate::domain::runtime::{CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;

pub fn dispatch(
    command: &Command,
    config: &RuntimeConfig,
    logging: &LoggingState,
) -> Result<CommandOutput, RuntimeError> {
    match command {
        Command::Identity(identity) => match identity.command {
            IdentityCommand::Init => Ok(CommandOutput::success(CommandView::IdentityInit(
                identity::init(config)?,
            ))),
            IdentityCommand::Show => Ok(CommandOutput::success(CommandView::IdentityShow(
                identity::show(config)?,
            ))),
        },
        Command::Myc(myc) => match myc.command {
            MycCommand::Status => Ok(myc::status(config)),
        },
        Command::Runtime(runtime) => match runtime.command {
            RuntimeCommand::Show => Ok(CommandOutput::success(CommandView::RuntimeShow(
                runtime::show(config, logging),
            ))),
        },
        Command::Signer(signer) => match signer.command {
            SignerCommand::Status => Ok(signer::status(config)),
        },
    }
}
