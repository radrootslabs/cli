use std::io::{self, Write};

use crate::domain::runtime::{CommandOutput, CommandView};
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
    match output.view() {
        CommandView::AccountNew(view) => {
            writeln!(stdout, "account new")?;
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
        CommandView::AccountWhoami(view) => {
            writeln!(stdout, "account")?;
            writeln!(stdout, "  path: {}", view.path)?;
            writeln!(stdout, "  state: {}", view.state)?;
            if let Some(reason) = &view.reason {
                writeln!(stdout, "  reason: {reason}")?;
            }
            if let Some(public_identity) = &view.public_identity {
                writeln!(stdout, "  id: {}", public_identity.id)?;
                writeln!(
                    stdout,
                    "  public key hex: {}",
                    public_identity.public_key_hex
                )?;
                writeln!(
                    stdout,
                    "  public key npub: {}",
                    public_identity.public_key_npub
                )?;
            }
        }
        CommandView::MycStatus(view) => {
            render_myc_status(&mut stdout, view)?;
        }
        CommandView::ConfigShow(view) => {
            writeln!(stdout, "config")?;
            writeln!(stdout, "  output format: {}", view.output_format)?;
            writeln!(stdout, "paths")?;
            writeln!(stdout, "  user config: {}", view.paths.user_config_path)?;
            writeln!(
                stdout,
                "  workspace config: {}",
                view.paths.workspace_config_path
            )?;
            writeln!(stdout, "  user state root: {}", view.paths.user_state_root)?;
            writeln!(stdout, "logging")?;
            writeln!(
                stdout,
                "  initialized: {}",
                yes_no(view.logging.initialized)
            )?;
            writeln!(stdout, "  filter: {}", view.logging.filter)?;
            writeln!(stdout, "  stdout: {}", yes_no(view.logging.stdout))?;
            if let Some(directory) = &view.logging.directory {
                writeln!(stdout, "  directory: {directory}")?;
            }
            if let Some(current_file) = &view.logging.current_file {
                writeln!(stdout, "  current file: {current_file}")?;
            }
            writeln!(stdout, "account")?;
            writeln!(stdout, "  identity path: {}", view.account.identity_path)?;
            writeln!(stdout, "signer")?;
            writeln!(stdout, "  backend: {}", view.signer.backend)?;
            writeln!(stdout, "myc")?;
            writeln!(stdout, "  executable: {}", view.myc.executable)?;
        }
        CommandView::SignerStatus(view) => {
            writeln!(stdout, "signer")?;
            writeln!(stdout, "  backend: {}", view.backend)?;
            writeln!(stdout, "  state: {}", view.state)?;
            writeln!(
                stdout,
                "  reason: {}",
                view.reason.as_deref().unwrap_or("<none>")
            )?;
            if let Some(local) = &view.local {
                render_local_signer(&mut stdout, "local signer", local)?;
            }
            if let Some(myc) = &view.myc {
                render_myc_status(&mut stdout, myc)?;
            }
        }
    }
    Ok(())
}

fn render_json(output: &CommandOutput) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    match output.view() {
        CommandView::AccountNew(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountWhoami(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::MycStatus(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::ConfigShow(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SignerStatus(view) => {
            serde_json::to_writer_pretty(&mut stdout, view)?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn render_local_signer(
    stdout: &mut dyn Write,
    heading: &str,
    local: &crate::domain::runtime::LocalSignerStatusView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{heading}")?;
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
    Ok(())
}

fn render_myc_status(
    stdout: &mut dyn Write,
    view: &crate::domain::runtime::MycStatusView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "myc")?;
    writeln!(stdout, "  executable: {}", view.executable)?;
    writeln!(stdout, "  state: {}", view.state)?;
    writeln!(stdout, "  ready: {}", yes_no(view.ready))?;
    writeln!(
        stdout,
        "  service status: {}",
        view.service_status.as_deref().unwrap_or("<unknown>")
    )?;
    writeln!(
        stdout,
        "  reason: {}",
        view.reason.as_deref().unwrap_or("<none>")
    )?;
    if !view.reasons.is_empty() {
        writeln!(stdout, "  reasons: {}", view.reasons.join(" | "))?;
    }
    if let Some(local_signer) = &view.local_signer {
        render_local_signer(stdout, "myc local signer", local_signer)?;
    }
    if let Some(custody) = &view.custody {
        render_myc_custody_identity(stdout, "myc custody signer", &custody.signer)?;
        render_myc_custody_identity(stdout, "myc custody user", &custody.user)?;
        if let Some(discovery_app) = &custody.discovery_app {
            render_myc_custody_identity(stdout, "myc custody discovery app", discovery_app)?;
        }
    }
    Ok(())
}

fn render_myc_custody_identity(
    stdout: &mut dyn Write,
    heading: &str,
    identity: &crate::domain::runtime::MycCustodyIdentityView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{heading}")?;
    writeln!(stdout, "  resolved: {}", yes_no(identity.resolved))?;
    writeln!(
        stdout,
        "  selected account id: {}",
        identity.selected_account_id.as_deref().unwrap_or("<none>")
    )?;
    writeln!(
        stdout,
        "  selected account state: {}",
        identity
            .selected_account_state
            .as_deref()
            .unwrap_or("<none>")
    )?;
    writeln!(
        stdout,
        "  identity id: {}",
        identity.identity_id.as_deref().unwrap_or("<none>")
    )?;
    writeln!(
        stdout,
        "  public key hex: {}",
        identity.public_key_hex.as_deref().unwrap_or("<none>")
    )?;
    writeln!(
        stdout,
        "  error: {}",
        identity.error.as_deref().unwrap_or("<none>")
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::commands::runtime;
    use crate::runtime::config::{
        IdentityConfig, LoggingConfig, MycConfig, OutputFormat, PathsConfig, RuntimeConfig,
        SignerBackend, SignerConfig,
    };
    use crate::runtime::logging::LoggingState;

    #[test]
    fn human_render_contains_config_sections() {
        let view = runtime::show(
            &RuntimeConfig {
                output_format: OutputFormat::Human,
                paths: PathsConfig {
                    user_config_path: "/home/tester/.config/radroots/config.toml".into(),
                    workspace_config_path: "/workspace/.radroots/config.toml".into(),
                    user_state_root: "/home/tester/.local/share/radroots".into(),
                },
                logging: LoggingConfig {
                    filter: "info".to_owned(),
                    directory: None,
                    stdout: false,
                },
                identity: IdentityConfig {
                    path: "identity.json".into(),
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
        assert_eq!(view.output_format, "human");
        assert_eq!(
            view.paths.workspace_config_path,
            "/workspace/.radroots/config.toml"
        );
        assert_eq!(view.account.identity_path, "identity.json");
    }
}
