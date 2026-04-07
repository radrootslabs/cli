use crate::cli::JobWatchArgs;
use crate::domain::runtime::CommandOutput;
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn list(config: &RuntimeConfig) -> CommandOutput {
    crate::runtime::job::list(config)
}

pub fn get(config: &RuntimeConfig, job_id: &str) -> CommandOutput {
    crate::runtime::job::get(config, job_id)
}

pub fn watch(config: &RuntimeConfig, args: &JobWatchArgs) -> Result<CommandOutput, RuntimeError> {
    crate::runtime::job::watch(config, args)
}
