use std::io::{self, Write};

use crate::domain::runtime::{
    AccountListView, AccountSummaryView, CommandOutput, CommandView, DoctorCheckView, DoctorView,
    FindView, LocalBackupView, LocalExportView, LocalInitView, LocalStatusView, NetStatusView,
    RelayListView, SyncActionView, SyncStatusView, SyncWatchView,
};
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
        CommandView::AccountList(view) => render_account_list(stdout, view)?,
        CommandView::AccountNew(view) => {
            write_context(stdout, format!("account · {}", view.state).as_str())?;
            render_owned_pairs(
                stdout,
                "account",
                account_pairs(&view.account, Some(&view.public_identity)).as_slice(),
            )?;
            writeln!(stdout, "source: {}", view.source)?;
            render_actions(stdout, &view.actions)?;
        }
        CommandView::AccountUse(view) => {
            write_context(stdout, "account · active")?;
            render_owned_pairs(
                stdout,
                "account",
                account_pairs(&view.account, None).as_slice(),
            )?;
            writeln!(stdout, "active account id: {}", view.active_account_id)?;
            writeln!(stdout, "source: {}", view.source)?;
        }
        CommandView::AccountWhoami(view) => {
            write_context(
                stdout,
                match view.state.as_str() {
                    "ready" => "account · active",
                    "unconfigured" => "account · unconfigured",
                    _ => "account",
                },
            )?;
            if let Some(account) = &view.account {
                render_owned_pairs(
                    stdout,
                    "account",
                    account_pairs(account, view.public_identity.as_ref()).as_slice(),
                )?;
            } else {
                writeln!(stdout, "no local account selected")?;
                writeln!(stdout)?;
            }
            if let Some(reason) = &view.reason {
                writeln!(stdout, "reason: {reason}")?;
            }
            writeln!(stdout, "source: {}", view.source)?;
        }
        CommandView::MycStatus(view) => {
            render_myc_status(stdout, view, true)?;
        }
        CommandView::NetStatus(view) => {
            render_net_status(stdout, view)?;
        }
        CommandView::ConfigShow(view) => {
            render_config_show(stdout, view)?;
        }
        CommandView::Doctor(view) => {
            render_doctor(stdout, view)?;
        }
        CommandView::Find(view) => {
            render_find(stdout, view)?;
        }
        CommandView::LocalBackup(view) => {
            render_local_backup(stdout, view)?;
        }
        CommandView::LocalExport(view) => {
            render_local_export(stdout, view)?;
        }
        CommandView::LocalInit(view) => {
            render_local_init(stdout, view)?;
        }
        CommandView::LocalStatus(view) => {
            render_local_status(stdout, view)?;
        }
        CommandView::RelayList(view) => {
            render_relay_list(stdout, view)?;
        }
        CommandView::SignerStatus(view) => {
            write_context(
                stdout,
                match view.state.as_str() {
                    "ready" => "signer · active",
                    "unconfigured" => "signer · unconfigured",
                    "degraded" => "signer · degraded",
                    "unavailable" => "signer · unavailable",
                    _ => "signer · error",
                },
            )?;
            let mut signer_rows = vec![
                ("mode", view.mode.as_str()),
                ("status", view.state.as_str()),
            ];
            if let Some(account_id) = &view.account_id {
                signer_rows.push(("account id", account_id.as_str()));
            }
            render_pairs(stdout, "signer", signer_rows.as_slice())?;
            if let Some(reason) = &view.reason {
                writeln!(stdout, "reason: {reason}")?;
            }
            writeln!(stdout, "source: {}", view.source)?;
            if let Some(local) = &view.local {
                writeln!(stdout)?;
                render_local_signer(stdout, "local account", local)?;
            }
            if let Some(myc) = &view.myc {
                writeln!(stdout)?;
                render_myc_status(stdout, myc, false)?;
            }
        }
        CommandView::SyncPull(view) => {
            render_sync_action(stdout, view)?;
        }
        CommandView::SyncPush(view) => {
            render_sync_action(stdout, view)?;
        }
        CommandView::SyncStatus(view) => {
            render_sync_status(stdout, view)?;
        }
        CommandView::SyncWatch(view) => {
            render_sync_watch(stdout, view)?;
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
        CommandView::AccountList(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountNew(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountUse(view) => {
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
        CommandView::NetStatus(view) => {
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
        CommandView::Find(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::LocalBackup(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::LocalExport(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::LocalInit(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::LocalStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RelayList(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SignerStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SyncPull(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SyncPush(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SyncStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SyncWatch(view) => {
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

fn render_ndjson_to(stdout: &mut dyn Write, output: &CommandOutput) -> Result<(), RuntimeError> {
    match output.view() {
        CommandView::AccountList(view) => {
            for account in &view.accounts {
                serde_json::to_writer(&mut *stdout, account)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::RelayList(view) => {
            for relay in &view.relays {
                serde_json::to_writer(&mut *stdout, relay)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::Find(view) => {
            for result in &view.results {
                serde_json::to_writer(&mut *stdout, result)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::SyncWatch(view) => {
            for frame in &view.frames {
                serde_json::to_writer(&mut *stdout, frame)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        _ => Err(RuntimeError::Config(format!(
            "`{}` does not support --ndjson",
            human_command_name(output.view())
        ))),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn present_absent(value: bool) -> &'static str {
    if value { "present" } else { "absent" }
}

fn render_account_list(stdout: &mut dyn Write, view: &AccountListView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("accounts · {} local", view.count).as_str())?;
    if view.accounts.is_empty() {
        writeln!(stdout, "no accounts found")?;
        writeln!(stdout)?;
    } else {
        let table = Table {
            headers: &["account", "display name", "signer", "default"],
            rows: view
                .accounts
                .iter()
                .map(|account| {
                    vec![
                        account.id.clone(),
                        account.display_name.clone().unwrap_or_default(),
                        account.signer.clone(),
                        yes_no(account.is_default).to_owned(),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
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

    let mut account_rows = vec![
        ("store path", view.account.store_path.as_str()),
        ("secrets dir", view.account.secrets_dir.as_str()),
        (
            "legacy import path",
            view.account.legacy_identity_path.as_str(),
        ),
    ];
    if let Some(selector) = &view.account.selector {
        account_rows.insert(0, ("selector", selector.as_str()));
    }
    render_pairs(stdout, "account", account_rows.as_slice())?;
    render_pairs(stdout, "signer", &[("mode", view.signer.mode.as_str())])?;
    let relay_count = view.relay.count.to_string();
    render_pairs(
        stdout,
        "relay",
        &[
            ("count", relay_count.as_str()),
            ("publish policy", view.relay.publish_policy.as_str()),
            ("source", view.relay.source.as_str()),
        ],
    )?;
    render_pairs(
        stdout,
        "local",
        &[
            ("root", view.local.root.as_str()),
            ("replica db", view.local.replica_db_path.as_str()),
            ("backups dir", view.local.backups_dir.as_str()),
            ("exports dir", view.local.exports_dir.as_str()),
        ],
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

fn render_find(stdout: &mut dyn Write, view: &FindView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "unconfigured" => "market · local first · unconfigured".to_owned(),
        _ => format!(
            "market · local first · {} result{}",
            view.count,
            if view.count == 1 { "" } else { "s" }
        ),
    };
    write_context(stdout, context.as_str())?;
    writeln!(stdout, "query: {}", view.query)?;

    match view.state.as_str() {
        "unconfigured" => {
            if let Some(reason) = &view.reason {
                writeln!(stdout, "reason: {reason}")?;
            }
        }
        _ if view.results.is_empty() => {
            if let Some(reason) = &view.reason {
                writeln!(stdout, "{reason}")?;
            }
        }
        _ => {
            let table = Table {
                headers: &["product", "category", "price", "available", "location"],
                rows: view
                    .results
                    .iter()
                    .map(|result| {
                        vec![
                            result.title.clone(),
                            result.category.clone(),
                            format_price(
                                result.price.amount,
                                &result.price.currency,
                                result.price.per_amount,
                                &result.price.per_unit,
                            ),
                            format_available(
                                result
                                    .available
                                    .available_amount
                                    .unwrap_or(result.available.total_amount),
                                result
                                    .available
                                    .label
                                    .as_deref()
                                    .unwrap_or(result.available.total_unit.as_str()),
                            ),
                            result.location_primary.clone().unwrap_or_default(),
                        ]
                    })
                    .collect(),
            };
            render_table(stdout, &table)?;
        }
    }

    writeln!(stdout)?;
    writeln!(
        stdout,
        "provenance: local replica · {} · {}",
        view.freshness.display,
        relay_count_text(view.relay_count)
    )?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_relay_list(stdout: &mut dyn Write, view: &RelayListView) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        match view.state.as_str() {
            "configured" => "relays · configured",
            _ => "relays · unconfigured",
        },
    )?;
    if view.relays.is_empty() {
        if let Some(reason) = &view.reason {
            writeln!(stdout, "{reason}")?;
            writeln!(stdout)?;
        }
    } else {
        let table = Table {
            headers: &["relay", "read", "write"],
            rows: view
                .relays
                .iter()
                .map(|relay| {
                    vec![
                        relay.url.clone(),
                        yes_no(relay.read).to_owned(),
                        yes_no(relay.write).to_owned(),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "publish policy: {}", view.publish_policy)?;
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_net_status(stdout: &mut dyn Write, view: &NetStatusView) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        match view.state.as_str() {
            "configured" => "network · configured",
            _ => "network · unconfigured",
        },
    )?;
    let relay_count = view.relay_count.to_string();
    let mut rows = vec![
        ("status", view.state.as_str()),
        ("session", view.session.as_str()),
        ("relays configured", relay_count.as_str()),
        ("publish policy", view.publish_policy.as_str()),
        ("signer mode", view.signer_mode.as_str()),
    ];
    if let Some(account_id) = &view.active_account_id {
        rows.push(("active account id", account_id.as_str()));
    }
    render_pairs(stdout, "network", rows.as_slice())?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_sync_status(stdout: &mut dyn Write, view: &SyncStatusView) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        match view.state.as_str() {
            "ready" => "activity · sync status",
            _ => "activity · sync unconfigured",
        },
    )?;
    let relay_count = view.relay_count.to_string();
    let expected = view.queue.expected_count.to_string();
    let pending = view.queue.pending_count.to_string();
    render_pairs(
        stdout,
        "sync",
        &[
            ("status", view.state.as_str()),
            ("freshness", view.freshness.display.as_str()),
            ("pending", pending.as_str()),
            ("expected", expected.as_str()),
            ("relays", relay_count.as_str()),
            ("publish policy", view.publish_policy.as_str()),
            ("replica db", view.replica_db.as_str()),
            ("local root", view.local_root.as_str()),
        ],
    )?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_sync_action(stdout: &mut dyn Write, view: &SyncActionView) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        format!("activity · sync {} {}", view.direction, view.state).as_str(),
    )?;
    let relay_count = view.relay_count.to_string();
    let expected = view.queue.expected_count.to_string();
    let pending = view.queue.pending_count.to_string();
    render_pairs(
        stdout,
        "sync",
        &[
            ("direction", view.direction.as_str()),
            ("status", view.state.as_str()),
            ("freshness", view.freshness.display.as_str()),
            ("pending", pending.as_str()),
            ("expected", expected.as_str()),
            ("relays", relay_count.as_str()),
            ("publish policy", view.publish_policy.as_str()),
            ("replica db", view.replica_db.as_str()),
            ("local root", view.local_root.as_str()),
        ],
    )?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_sync_watch(stdout: &mut dyn Write, view: &SyncWatchView) -> Result<(), RuntimeError> {
    write_context(stdout, "activity · sync watch")?;
    if view.frames.is_empty() {
        writeln!(stdout, "no sync frames collected")?;
        writeln!(stdout)?;
    } else {
        let table = Table {
            headers: &["frame", "status", "freshness", "pending", "relays"],
            rows: view
                .frames
                .iter()
                .map(|frame| {
                    vec![
                        frame.sequence.to_string(),
                        frame.state.clone(),
                        frame.freshness.display.clone(),
                        frame.queue.pending_count.to_string(),
                        frame.relay_count.to_string(),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "interval ms: {}", view.interval_ms)?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_local_init(stdout: &mut dyn Write, view: &LocalInitView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("local · {}", view.state).as_str())?;
    render_pairs(
        stdout,
        "local",
        &[
            ("replica db", view.replica_db.as_str()),
            ("path", view.path.as_str()),
            ("local root", view.local_root.as_str()),
            ("replica db version", view.replica_db_version.as_str()),
            ("backup format version", view.backup_format_version.as_str()),
        ],
    )?;
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn render_local_status(stdout: &mut dyn Write, view: &LocalStatusView) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        match view.state.as_str() {
            "ready" => "local · status",
            _ => "local · unconfigured",
        },
    )?;
    let mut rows = vec![
        ("replica db", view.replica_db.as_str()),
        ("path", view.path.as_str()),
        ("local root", view.local_root.as_str()),
    ];
    if view.state == "ready" {
        rows.push(("replica db version", view.replica_db_version.as_str()));
        rows.push(("backup format version", view.backup_format_version.as_str()));
        rows.push(("schema hash", view.schema_hash.as_str()));
    }
    render_pairs(stdout, "local", rows.as_slice())?;
    if view.state == "ready" {
        let sync_expected = view.sync.expected_count.to_string();
        let sync_pending = view.sync.pending_count.to_string();
        render_pairs(
            stdout,
            "sync",
            &[
                ("expected", sync_expected.as_str()),
                ("pending", sync_pending.as_str()),
            ],
        )?;
        let farms = view.counts.farms.to_string();
        let listings = view.counts.listings.to_string();
        let profiles = view.counts.profiles.to_string();
        let relays = view.counts.relays.to_string();
        let event_states = view.counts.event_states.to_string();
        render_pairs(
            stdout,
            "counts",
            &[
                ("farms", farms.as_str()),
                ("listings", listings.as_str()),
                ("profiles", profiles.as_str()),
                ("relays", relays.as_str()),
                ("event states", event_states.as_str()),
            ],
        )?;
    }
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_local_backup(stdout: &mut dyn Write, view: &LocalBackupView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("local · {}", view.state).as_str())?;
    let size_bytes = view.size_bytes.to_string();
    let mut rows = vec![("file", view.file.as_str())];
    if view.state != "unconfigured" {
        rows.push(("size bytes", size_bytes.as_str()));
        rows.push(("backup format version", view.backup_format_version.as_str()));
        rows.push(("replica db version", view.replica_db_version.as_str()));
    }
    render_pairs(stdout, "backup", rows.as_slice())?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_local_export(stdout: &mut dyn Write, view: &LocalExportView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("local · {}", view.state).as_str())?;
    let records = view.records.to_string();
    let mut rows = vec![
        ("format", view.format.as_str()),
        ("file", view.file.as_str()),
    ];
    if view.state != "unconfigured" {
        rows.push(("records", records.as_str()));
        rows.push(("export version", view.export_version.as_str()));
        rows.push(("schema hash", view.schema_hash.as_str()));
    }
    render_pairs(stdout, "export", rows.as_slice())?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
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

fn render_actions(stdout: &mut dyn Write, actions: &[String]) -> Result<(), RuntimeError> {
    if actions.is_empty() {
        return Ok(());
    }
    writeln!(stdout)?;
    writeln!(stdout, "actions")?;
    for action in actions {
        writeln!(stdout, "  › {action}")?;
    }
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

fn render_owned_pairs(
    stdout: &mut dyn Write,
    heading: &str,
    rows: &[(&str, String)],
) -> Result<(), RuntimeError> {
    let borrowed = rows
        .iter()
        .map(|(label, value)| (*label, value.as_str()))
        .collect::<Vec<_>>();
    render_pairs(stdout, heading, borrowed.as_slice())
}

fn account_pairs(
    account: &AccountSummaryView,
    public_identity: Option<&crate::domain::runtime::IdentityPublicView>,
) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        ("account id", account.id.clone()),
        ("signer", account.signer.clone()),
        ("default", yes_no(account.is_default).to_owned()),
    ];
    if let Some(display_name) = &account.display_name {
        rows.insert(1, ("display name", display_name.clone()));
    }
    if let Some(public_identity) = public_identity {
        rows.push(("public key npub", public_identity.public_key_npub.clone()));
        rows.push(("public key hex", public_identity.public_key_hex.clone()));
    }
    rows
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
    standalone: bool,
) -> Result<(), RuntimeError> {
    if standalone {
        write_context(stdout, format!("myc · {}", view.state).as_str())?;
    }
    let mut rows = vec![
        ("executable", view.executable.as_str()),
        ("status", view.state.as_str()),
        ("ready", yes_no(view.ready)),
    ];
    if let Some(service_status) = &view.service_status {
        rows.push(("service status", service_status.as_str()));
    }
    render_pairs(stdout, "myc", rows.as_slice())?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    if !view.reasons.is_empty() {
        writeln!(stdout, "reasons: {}", view.reasons.join(" | "))?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    if let Some(local_signer) = &view.local_signer {
        writeln!(stdout)?;
        render_local_signer(stdout, "myc local signer", local_signer)?;
    }
    if let Some(custody) = &view.custody {
        writeln!(stdout)?;
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

fn relay_count_text(count: usize) -> String {
    if count == 1 {
        "1 relay configured".to_owned()
    } else {
        format!("{count} relays configured")
    }
}

fn format_price(amount: f64, currency: &str, per_amount: u32, per_unit: &str) -> String {
    format!(
        "{} {currency}/{} {per_unit}",
        trim_decimal(amount),
        per_amount
    )
}

fn format_available(amount: i64, unit: &str) -> String {
    format!("{amount} {unit}")
}

fn trim_decimal(value: f64) -> String {
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

struct Table {
    headers: &'static [&'static str],
    rows: Vec<Vec<String>>,
}

fn human_command_name(view: &CommandView) -> &'static str {
    match view {
        CommandView::AccountList(_) => "account ls",
        CommandView::AccountNew(_) => "account new",
        CommandView::AccountUse(_) => "account use",
        CommandView::AccountWhoami(_) => "account whoami",
        CommandView::ConfigShow(_) => "config show",
        CommandView::Doctor(_) => "doctor",
        CommandView::Find(_) => "find",
        CommandView::LocalBackup(_) => "local backup",
        CommandView::LocalExport(_) => "local export",
        CommandView::LocalInit(_) => "local init",
        CommandView::LocalStatus(_) => "local status",
        CommandView::MycStatus(_) => "myc status",
        CommandView::NetStatus(_) => "net status",
        CommandView::RelayList(_) => "relay ls",
        CommandView::SignerStatus(_) => "signer status",
        CommandView::SyncPull(_) => "sync pull",
        CommandView::SyncPush(_) => "sync push",
        CommandView::SyncStatus(_) => "sync status",
        CommandView::SyncWatch(_) => "sync watch",
    }
}

#[cfg(test)]
mod tests {
    use super::{Table, render_human_to, render_ndjson_to, render_table};
    use crate::commands::runtime;
    use crate::domain::runtime::{
        AccountListView, CommandOutput, CommandView, DoctorCheckView, DoctorView, MycStatusView,
        RelayEntryView, RelayListView,
    };
    use crate::runtime::config::{
        AccountConfig, IdentityConfig, LocalConfig, LoggingConfig, MycConfig, OutputConfig,
        OutputFormat, PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy,
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
                account: AccountConfig {
                    selector: Some("acct_demo".into()),
                    store_path: "/home/tester/.local/share/radroots/accounts/store.json".into(),
                    secrets_dir: "/home/tester/.local/share/radroots/accounts/secrets".into(),
                },
                identity: IdentityConfig {
                    path: "identity.json".into(),
                },
                signer: SignerConfig {
                    backend: SignerBackend::Local,
                },
                relay: RelayConfig {
                    urls: vec!["wss://relay.one".into(), "wss://relay.two".into()],
                    publish_policy: RelayPublishPolicy::Any,
                    source: RelayConfigSource::WorkspaceConfig,
                },
                local: LocalConfig {
                    root: "/home/tester/.local/share/radroots/replica".into(),
                    replica_db_path: "/home/tester/.local/share/radroots/replica/replica.sqlite"
                        .into(),
                    backups_dir: "/home/tester/.local/share/radroots/replica/backups".into(),
                    exports_dir: "/home/tester/.local/share/radroots/replica/exports".into(),
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
        assert_eq!(view.account.selector.as_deref(), Some("acct_demo"));
        assert!(
            view.account
                .store_path
                .ends_with(".local/share/radroots/accounts/store.json")
        );
        assert_eq!(view.relay.count, 2);
        assert_eq!(view.relay.publish_policy, "any");
        assert!(
            view.local
                .replica_db_path
                .ends_with(".local/share/radroots/replica/replica.sqlite")
        );
    }

    #[test]
    fn human_render_omits_placeholder_tokens() {
        let output = CommandOutput::success(CommandView::MycStatus(MycStatusView {
            executable: "myc".to_owned(),
            state: "unavailable".to_owned(),
            source: "myc status command · local first".to_owned(),
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
                account: AccountConfig {
                    selector: None,
                    store_path: "/home/tester/.local/share/radroots/accounts/store.json".into(),
                    secrets_dir: "/home/tester/.local/share/radroots/accounts/secrets".into(),
                },
                identity: IdentityConfig {
                    path: "identity.json".into(),
                },
                signer: SignerConfig {
                    backend: SignerBackend::Local,
                },
                relay: RelayConfig {
                    urls: Vec::new(),
                    publish_policy: RelayPublishPolicy::Any,
                    source: RelayConfigSource::Defaults,
                },
                local: LocalConfig {
                    root: "/home/tester/.local/share/radroots/replica".into(),
                    replica_db_path: "/home/tester/.local/share/radroots/replica/replica.sqlite"
                        .into(),
                    backups_dir: "/home/tester/.local/share/radroots/replica/backups".into(),
                    exports_dir: "/home/tester/.local/share/radroots/replica/exports".into(),
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
    fn account_list_ndjson_emits_one_json_object_per_account() {
        let output = CommandOutput::success(CommandView::AccountList(AccountListView {
            source: "local account store · local first".to_owned(),
            count: 2,
            accounts: vec![
                crate::domain::runtime::AccountSummaryView {
                    id: "acct_a".to_owned(),
                    display_name: Some("Alpha".to_owned()),
                    signer: "local".to_owned(),
                    is_default: true,
                },
                crate::domain::runtime::AccountSummaryView {
                    id: "acct_b".to_owned(),
                    display_name: None,
                    signer: "local".to_owned(),
                    is_default: false,
                },
            ],
            actions: Vec::new(),
        }));
        let mut buffer = Vec::new();
        render_ndjson_to(&mut buffer, &output).expect("render ndjson");
        let rendered = String::from_utf8(buffer).expect("utf8");
        let lines = rendered.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"id\":\"acct_a\""));
        assert!(lines[1].contains("\"id\":\"acct_b\""));
    }

    #[test]
    fn relay_list_ndjson_emits_one_json_object_per_relay() {
        let output = CommandOutput::success(CommandView::RelayList(RelayListView {
            state: "configured".to_owned(),
            source: "workspace config · local first".to_owned(),
            publish_policy: "any".to_owned(),
            count: 2,
            reason: None,
            relays: vec![
                RelayEntryView {
                    url: "wss://relay.one".to_owned(),
                    read: true,
                    write: true,
                },
                RelayEntryView {
                    url: "wss://relay.two".to_owned(),
                    read: true,
                    write: true,
                },
            ],
            actions: Vec::new(),
        }));
        let mut buffer = Vec::new();
        render_ndjson_to(&mut buffer, &output).expect("render relay ndjson");
        let rendered = String::from_utf8(buffer).expect("utf8");
        let lines = rendered.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"url\":\"wss://relay.one\""));
        assert!(lines[1].contains("\"url\":\"wss://relay.two\""));
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
                    detail: "no local account in store".to_owned(),
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
