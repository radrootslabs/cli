#![forbid(unsafe_code)]

mod cli;
mod commands;
mod domain;
mod render;
mod runtime;

use std::io::Write;
use std::process::ExitCode;

use crate::cli::CliArgs;
use crate::commands::dispatch;
use crate::render::render_output;
use crate::runtime::config::{OutputFormat, RuntimeConfig};
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
    if let crate::cli::Command::Order(order) = command {
        if let crate::cli::OrderCommand::Submit(args) = &order.command {
            if args.watch && config.output.format != OutputFormat::Human {
                return Err(runtime::RuntimeError::Config(
                    "`order submit --watch` only supports human output; use `order submit` and `order watch` for machine output".to_owned(),
                ));
            }
        }
    }

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
