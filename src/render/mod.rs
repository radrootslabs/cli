use std::io::{self, Write};

use crate::domain::runtime::{CommandOutput, CommandView, DoctorCheckView, DoctorView};
use crate::runtime::RuntimeError;
use crate::runtime::config::{OutputConfig, OutputFormat};

const THIN_RULE: &str = "────────────────────────────────────────────────────";

pub fn render_output(output: &CommandOutput, config: &OutputConfig) -> Result<(), RuntimeError> {
    match config.format {
        OutputFormat::Human => render_human(output),
        OutputFormat::Json => render_json(output),
        OutputFormat::Ndjson => render_ndjson(output),
    }
}

fn render_human(output: &CommandOutput) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    render_human_to(&mut stdout, output)
}

fn render_human_to(stdout: &mut dyn Write, output: &CommandOutput) -> Result<(), RuntimeError> {
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
            render_myc_status(stdout, view)?;
        }
        CommandView::ConfigShow(view) => {
            render_config_show(stdout, view)?;
        }
        CommandView::Doctor(view) => {
            render_doctor(stdout, view)?;
        }
        CommandView::SignerStatus(view) => {
            writeln!(stdout, "signer")?;
            writeln!(stdout, "  backend: {}", view.backend)?;
            writeln!(stdout, "  state: {}", view.state)?;
            if let Some(reason) = &view.reason {
                writeln!(stdout, "  reason: {reason}")?;
            }
            if let Some(local) = &view.local {
                render_local_signer(stdout, "local signer", local)?;
            }
            if let Some(myc) = &view.myc {
                render_myc_status(stdout, myc)?;
            }
        }
    }
    Ok(())
}

fn render_json(output: &CommandOutput) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    render_json_to(&mut stdout, output)
}

fn render_json_to(stdout: &mut dyn Write, output: &CommandOutput) -> Result<(), RuntimeError> {
    match output.view() {
        CommandView::AccountNew(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountWhoami(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::MycStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::ConfigShow(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::Doctor(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SignerStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

fn render_ndjson(output: &CommandOutput) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    render_ndjson_to(&mut stdout, output)
}

fn render_ndjson_to(_stdout: &mut dyn Write, output: &CommandOutput) -> Result<(), RuntimeError> {
    Err(RuntimeError::Config(format!(
        "`{}` does not support --ndjson",
        human_command_name(output.view())
    )))
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn present_absent(value: bool) -> &'static str {
    if value { "present" } else { "absent" }
}

fn render_config_show(
    stdout: &mut dyn Write,
    view: &crate::domain::runtime::ConfigShowView,
) -> Result<(), RuntimeError> {
    write_context(stdout, "config · effective")?;
    render_pairs(
        stdout,
        "output",
        &[
            ("format", view.output.format.as_str()),
            ("verbosity", view.output.verbosity.as_str()),
            ("color", yes_no(view.output.color)),
            ("dry run", yes_no(view.output.dry_run)),
        ],
    )?;
    let user_config = format!(
        "{} · {}",
        present_absent(view.config_files.user_present),
        view.paths.user_config_path
    );
    let workspace_config = format!(
        "{} · {}",
        present_absent(view.config_files.workspace_present),
        view.paths.workspace_config_path
    );
    render_pairs(
        stdout,
        "config roots",
        &[
            ("user config", user_config.as_str()),
            ("workspace config", workspace_config.as_str()),
            ("user state root", view.paths.user_state_root.as_str()),
        ],
    )?;

    let mut logging_rows = vec![
        ("filter", view.logging.filter.as_str()),
        ("stdout", yes_no(view.logging.stdout)),
    ];
    if let Some(directory) = &view.logging.directory {
        logging_rows.push(("directory", directory.as_str()));
    }
    if let Some(current_file) = &view.logging.current_file {
        logging_rows.push(("file", current_file.as_str()));
    }
    render_pairs(stdout, "logging", logging_rows.as_slice())?;
    render_pairs(
        stdout,
        "account",
        &[("identity path", view.account.identity_path.as_str())],
    )?;
    render_pairs(
        stdout,
        "signer",
        &[("backend", view.signer.backend.as_str())],
    )?;
    render_pairs(
        stdout,
        "myc",
        &[("executable", view.myc.executable.as_str())],
    )?;
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn render_doctor(stdout: &mut dyn Write, view: &DoctorView) -> Result<(), RuntimeError> {
    write_context(stdout, "system · checks")?;
    let table = Table {
        headers: &["check", "status", "detail"],
        rows: view.checks.iter().map(doctor_row).collect(),
    };
    render_table(stdout, &table)?;
    if !view.actions.is_empty() {
        writeln!(stdout)?;
        writeln!(stdout, "actions")?;
        for action in &view.actions {
            writeln!(stdout, "  › {action}")?;
        }
    }
    writeln!(stdout)?;
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn doctor_row(check: &DoctorCheckView) -> Vec<String> {
    vec![
        check.name.clone(),
        check.status.clone(),
        check.detail.clone(),
    ]
}

fn write_context(stdout: &mut dyn Write, line: &str) -> Result<(), RuntimeError> {
    writeln!(stdout, "{line}")?;
    writeln!(stdout, "{THIN_RULE}")?;
    Ok(())
}

fn render_pairs(
    stdout: &mut dyn Write,
    heading: &str,
    rows: &[(&str, &str)],
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{heading}")?;
    let label_width = rows
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or_default();
    for (label, value) in rows {
        writeln!(stdout, "  {label:label_width$}  {value}")?;
    }
    writeln!(stdout)?;
    Ok(())
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
    if let Some(service_status) = &view.service_status {
        writeln!(stdout, "  service status: {service_status}")?;
    }
    if let Some(reason) = &view.reason {
        writeln!(stdout, "  reason: {reason}")?;
    }
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
    if let Some(selected_account_id) = &identity.selected_account_id {
        writeln!(stdout, "  selected account id: {selected_account_id}")?;
    }
    if let Some(selected_account_state) = &identity.selected_account_state {
        writeln!(stdout, "  selected account state: {selected_account_state}")?;
    }
    if let Some(identity_id) = &identity.identity_id {
        writeln!(stdout, "  identity id: {identity_id}")?;
    }
    if let Some(public_key_hex) = &identity.public_key_hex {
        writeln!(stdout, "  public key hex: {public_key_hex}")?;
    }
    if let Some(error) = &identity.error {
        writeln!(stdout, "  error: {error}")?;
    }
    Ok(())
}

#[allow(dead_code)]
fn render_table(stdout: &mut dyn Write, table: &Table) -> Result<(), RuntimeError> {
    let mut widths: Vec<usize> = table.headers.iter().map(|header| header.len()).collect();
    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(cell.len());
            }
        }
    }

    for (index, header) in table.headers.iter().enumerate() {
        if index > 0 {
            write!(stdout, "  ")?;
        }
        write!(stdout, "{header:width$}", width = widths[index])?;
    }
    writeln!(stdout)?;

    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                write!(stdout, "  ")?;
            }
            write!(stdout, "{cell:width$}", width = widths[index])?;
        }
        writeln!(stdout)?;
    }

    Ok(())
}

#[allow(dead_code)]
struct Table {
    headers: &'static [&'static str],
    rows: Vec<Vec<String>>,
}

fn human_command_name(view: &CommandView) -> &'static str {
    match view {
        CommandView::AccountNew(_) => "account new",
        CommandView::AccountWhoami(_) => "account whoami",
        CommandView::ConfigShow(_) => "config show",
        CommandView::Doctor(_) => "doctor",
        CommandView::MycStatus(_) => "myc status",
        CommandView::SignerStatus(_) => "signer status",
    }
}

#[cfg(test)]
mod tests {
    use super::{Table, render_human_to, render_ndjson_to, render_table};
    use crate::commands::runtime;
    use crate::domain::runtime::{
        CommandOutput, CommandView, DoctorCheckView, DoctorView, MycStatusView,
    };
    use crate::runtime::config::{
        IdentityConfig, LoggingConfig, MycConfig, OutputConfig, OutputFormat, PathsConfig,
        RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime::logging::LoggingState;

    #[test]
    fn human_render_contains_config_sections() {
        let view = runtime::show(
            &RuntimeConfig {
                output: OutputConfig {
                    format: OutputFormat::Human,
                    verbosity: Verbosity::Normal,
                    color: true,
                    dry_run: false,
                },
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
        assert_eq!(view.output.format, "human");
        assert_eq!(
            view.paths.workspace_config_path,
            "/workspace/.radroots/config.toml"
        );
        assert_eq!(view.account.identity_path, "identity.json");
    }

    #[test]
    fn human_render_omits_placeholder_tokens() {
        let output = CommandOutput::success(CommandView::MycStatus(MycStatusView {
            executable: "myc".to_owned(),
            state: "unavailable".to_owned(),
            service_status: None,
            ready: false,
            reason: None,
            reasons: Vec::new(),
            local_signer: None,
            custody: None,
        }));
        let mut buffer = Vec::new();
        render_human_to(&mut buffer, &output).expect("render human");
        let rendered = String::from_utf8(buffer).expect("utf8");
        assert!(!rendered.contains("<none>"));
        assert!(!rendered.contains("<unknown>"));
        assert!(!rendered.contains("<disabled>"));
    }

    #[test]
    fn ndjson_rejects_singular_views() {
        let output = CommandOutput::success(CommandView::ConfigShow(runtime::show(
            &RuntimeConfig {
                output: OutputConfig {
                    format: OutputFormat::Ndjson,
                    verbosity: Verbosity::Trace,
                    color: false,
                    dry_run: true,
                },
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
        )));
        let mut buffer = Vec::new();
        let error = render_ndjson_to(&mut buffer, &output).expect_err("unsupported ndjson");
        assert!(
            error
                .to_string()
                .contains("`config show` does not support --ndjson")
        );
    }

    #[test]
    fn human_render_doctor_uses_check_table_and_actions() {
        let output = CommandOutput::unconfigured(CommandView::Doctor(DoctorView {
            ok: false,
            state: "warn".to_owned(),
            checks: vec![
                DoctorCheckView {
                    name: "config".to_owned(),
                    status: "ok".to_owned(),
                    detail: "defaults active".to_owned(),
                },
                DoctorCheckView {
                    name: "account".to_owned(),
                    status: "warn".to_owned(),
                    detail: "no local account at identity.json".to_owned(),
                },
            ],
            source: "local diagnostics".to_owned(),
            actions: vec!["radroots account new".to_owned()],
        }));
        let mut buffer = Vec::new();
        render_human_to(&mut buffer, &output).expect("render human");
        let rendered = String::from_utf8(buffer).expect("utf8");
        assert!(rendered.contains("system · checks"));
        assert!(rendered.contains("check"));
        assert!(rendered.contains("account  warn"));
        assert!(rendered.contains("actions"));
        assert!(rendered.contains("› radroots account new"));
        assert!(rendered.contains("source: local diagnostics"));
    }

    #[test]
    fn table_renderer_aligns_columns() {
        let table = Table {
            headers: &["item", "status"],
            rows: vec![
                vec!["alpha".to_owned(), "ready".to_owned()],
                vec!["beta-long".to_owned(), "pending".to_owned()],
            ],
        };
        let mut buffer = Vec::new();
        render_table(&mut buffer, &table).expect("render table");
        let rendered = String::from_utf8(buffer).expect("utf8");
        assert!(rendered.contains("item       status"));
        assert!(rendered.contains("alpha      ready"));
        assert!(rendered.contains("beta-long  pending"));
    }
}
