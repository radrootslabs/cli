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
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            error.exit_code()
        }
    }
}

fn run() -> Result<(), runtime::RuntimeError> {
    let args = CliArgs::parse();
    let config = RuntimeConfig::from_system(&args)?;
    let logging = initialize_logging(&config.logging)?;
    let output = dispatch(&args.command, &config, &logging)?;
    render_output(&output, config.output_format)?;
    Ok(())
}
