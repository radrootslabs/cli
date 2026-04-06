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
    let logging = initialize_logging(&config.logging)?;
    let output = dispatch(&args.command, &config, &logging)?;
    render_output(&output, config.output_format)?;
    Ok(output.exit_code())
}
