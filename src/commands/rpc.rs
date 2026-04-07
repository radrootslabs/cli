use crate::domain::runtime::CommandOutput;
use crate::runtime::config::RuntimeConfig;

pub fn status(config: &RuntimeConfig) -> CommandOutput {
    crate::runtime::daemon::status(config)
}

pub fn sessions(config: &RuntimeConfig) -> CommandOutput {
    crate::runtime::daemon::sessions(config)
}
