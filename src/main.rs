#![forbid(unsafe_code)]

mod cli;
mod commands;
mod domain;
mod render;
mod runtime;

use clap::Parser;
use std::io::Write;
use std::process::ExitCode;

use crate::cli::CliArgs;
use crate::commands::dispatch;
use crate::render::render_output;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::initialize_logging;

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            error.exit_code()
        }
    }
}

fn run() -> Result<ExitCode, runtime::RuntimeError> {
    let args = CliArgs::parse();
    let config = RuntimeConfig::from_system(&args)?;
    validate_command_contracts(&args.command, &config)?;
    let logging = initialize_logging(&config.logging)?;
    let output = dispatch(&args.command, &config, &logging)?;
    render_output(&output, &config.output)?;
    Ok(output.exit_code())
}

fn validate_command_contracts(
    command: &crate::cli::Command,
    config: &RuntimeConfig,
) -> Result<(), runtime::RuntimeError> {
    if !command.supports_output_format(config.output.format) {
        return Err(runtime::RuntimeError::Config(format!(
            "`{}` does not support --{}",
            command.display_name(),
            config.output.format.as_str()
        )));
    }

    if config.output.dry_run && !command.supports_dry_run() {
        return Err(runtime::RuntimeError::Config(format!(
            "`{}` does not support --dry-run yet",
            command.display_name()
        )));
    }

    Ok(())
}
