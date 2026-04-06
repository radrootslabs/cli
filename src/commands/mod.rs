pub mod runtime;

use crate::cli::{Command, RuntimeCommand};
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
        Command::Runtime(runtime) => match runtime.command {
            RuntimeCommand::Show => Ok(CommandOutput::RuntimeShow(runtime::show(config, logging))),
        },
    }
}
