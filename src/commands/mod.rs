pub mod identity;
pub mod myc;
pub mod runtime;
pub mod signer;

use crate::cli::{Command, IdentityCommand, MycCommand, RuntimeCommand, SignerCommand};
use crate::domain::CommandOutput;
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
            IdentityCommand::Init => Ok(CommandOutput::IdentityInit(identity::init(config)?)),
            IdentityCommand::Show => Ok(CommandOutput::IdentityShow(identity::show(config)?)),
        },
        Command::Myc(myc) => match myc.command {
            MycCommand::Status => Ok(CommandOutput::MycStatus(myc::status(config))),
        },
        Command::Runtime(runtime) => match runtime.command {
            RuntimeCommand::Show => Ok(CommandOutput::RuntimeShow(runtime::show(config, logging))),
        },
        Command::Signer(signer) => match signer.command {
            SignerCommand::Status => Ok(CommandOutput::SignerStatus(signer::status(config))),
        },
    }
}
