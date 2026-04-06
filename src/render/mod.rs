use std::io::{self, Write};

use crate::domain::runtime::CommandOutput;
use crate::runtime::RuntimeError;
use crate::runtime::config::OutputFormat;

pub fn render_output(output: &CommandOutput, format: OutputFormat) -> Result<(), RuntimeError> {
    match format {
        OutputFormat::Human => render_human(output),
        OutputFormat::Json => render_json(output),
    }
}

fn render_human(output: &CommandOutput) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    match output {
        CommandOutput::IdentityInit(view) => {
            writeln!(stdout, "identity init")?;
            writeln!(stdout, "  path: {}", view.path)?;
            writeln!(stdout, "  created: {}", yes_no(view.created))?;
            writeln!(stdout, "  id: {}", view.public_identity.id)?;
            writeln!(
                stdout,
                "  public key hex: {}",
                view.public_identity.public_key_hex
            )?;
            writeln!(
                stdout,
                "  public key npub: {}",
                view.public_identity.public_key_npub
            )?;
        }
        CommandOutput::IdentityShow(view) => {
            writeln!(stdout, "identity")?;
            writeln!(stdout, "  path: {}", view.path)?;
            writeln!(stdout, "  id: {}", view.public_identity.id)?;
            writeln!(
                stdout,
                "  public key hex: {}",
                view.public_identity.public_key_hex
            )?;
            writeln!(
                stdout,
                "  public key npub: {}",
                view.public_identity.public_key_npub
            )?;
        }
        CommandOutput::RuntimeShow(view) => {
            writeln!(stdout, "runtime")?;
            writeln!(stdout, "  output format: {}", view.output_format)?;
            writeln!(stdout, "logging")?;
            writeln!(
                stdout,
                "  initialized: {}",
                yes_no(view.logging.initialized)
            )?;
            writeln!(stdout, "  filter: {}", view.logging.filter)?;
            writeln!(stdout, "  stdout: {}", yes_no(view.logging.stdout))?;
            writeln!(
                stdout,
                "  directory: {}",
                view.logging.directory.as_deref().unwrap_or("<disabled>")
            )?;
            writeln!(
                stdout,
                "  current file: {}",
                view.logging.current_file.as_deref().unwrap_or("<disabled>")
            )?;
            writeln!(stdout, "identity")?;
            writeln!(stdout, "  path: {}", view.identity.path)?;
            writeln!(
                stdout,
                "  allow generate: {}",
                yes_no(view.identity.allow_generate)
            )?;
            writeln!(stdout, "signer")?;
            writeln!(stdout, "  backend: {}", view.signer.backend)?;
            writeln!(stdout, "myc")?;
            writeln!(stdout, "  executable: {}", view.myc.executable)?;
        }
        CommandOutput::SignerStatus(view) => {
            writeln!(stdout, "signer")?;
            writeln!(stdout, "  backend: {}", view.backend)?;
            writeln!(stdout, "  state: {}", view.state)?;
            writeln!(
                stdout,
                "  reason: {}",
                view.reason.as_deref().unwrap_or("<none>")
            )?;
            if let Some(local) = &view.local {
                writeln!(stdout, "local signer")?;
                writeln!(stdout, "  account id: {}", local.account_id)?;
                writeln!(
                    stdout,
                    "  public key hex: {}",
                    local.public_identity.public_key_hex
                )?;
                writeln!(
                    stdout,
                    "  public key npub: {}",
                    local.public_identity.public_key_npub
                )?;
                writeln!(stdout, "  availability: {}", local.availability)?;
                writeln!(stdout, "  secret backed: {}", yes_no(local.secret_backed))?;
            }
        }
    }
    Ok(())
}

fn render_json(output: &CommandOutput) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    match output {
        CommandOutput::IdentityInit(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandOutput::IdentityShow(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandOutput::RuntimeShow(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandOutput::SignerStatus(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use crate::commands::runtime;
    use crate::runtime::config::{
        IdentityConfig, LoggingConfig, MycConfig, OutputFormat, RuntimeConfig, SignerBackend,
        SignerConfig,
    };
    use crate::runtime::logging::LoggingState;

    #[test]
    fn human_render_contains_runtime_sections() {
        let view = runtime::show(
            &RuntimeConfig {
                output_format: OutputFormat::Human,
                logging: LoggingConfig {
                    filter: "info".to_owned(),
                    directory: None,
                    stdout: false,
                },
                identity: IdentityConfig {
                    path: "identity.json".into(),
                    allow_generate: false,
                },
                signer: SignerConfig {
                    backend: SignerBackend::Local,
                },
                myc: MycConfig {
                    executable: "myc".into(),
                },
            },
            &LoggingState {
                initialized: true,
                current_file: None,
            },
        );
        let rendered = format!(
            "runtime\n  output format: {}\nlogging\n  initialized: {}\n",
            view.output_format,
            if view.logging.initialized {
                "yes"
            } else {
                "no"
            }
        );
        assert!(rendered.contains("runtime"));
        assert!(rendered.contains("logging"));
    }
}
