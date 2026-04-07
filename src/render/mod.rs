use std::io::{self, Write};

use crate::domain::runtime::{
    AccountListView, AccountSummaryView, CommandOutput, CommandView, DoctorCheckView, DoctorView,
    FindView, JobGetView, JobListView, JobWatchView, ListingGetView, ListingMutationView,
    ListingNewView, ListingValidateView, LocalBackupView, LocalExportView, LocalInitView,
    LocalStatusView, NetStatusView, OrderCancelView, OrderDraftItemView, OrderGetView,
    OrderHistoryView, OrderJobView, OrderListView, OrderNewView, OrderSubmitView, OrderWatchView,
    RelayListView, RpcSessionsView, RpcStatusView, SyncActionView, SyncStatusView, SyncWatchView,
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
        CommandView::OrderCancel(view) => {
            render_order_cancel(stdout, view)?;
        }
        CommandView::OrderGet(view) => {
            render_order_get(stdout, view)?;
        }
        CommandView::OrderHistory(view) => {
            render_order_history(stdout, view)?;
        }
        CommandView::OrderList(view) => {
            render_order_list(stdout, view)?;
        }
        CommandView::OrderNew(view) => {
            render_order_new(stdout, view)?;
        }
        CommandView::OrderSubmit(view) => {
            render_order_submit(stdout, view)?;
        }
        CommandView::OrderWatch(view) => {
            render_order_watch(stdout, view)?;
        }
        CommandView::RpcSessions(view) => {
            render_rpc_sessions(stdout, view)?;
        }
        CommandView::RpcStatus(view) => {
            render_rpc_status(stdout, view)?;
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
        CommandView::JobGet(view) => {
            render_job_get(stdout, view)?;
        }
        CommandView::JobList(view) => {
            render_job_list(stdout, view)?;
        }
        CommandView::JobWatch(view) => {
            render_job_watch(stdout, view)?;
        }
        CommandView::ListingGet(view) => {
            render_listing_get(stdout, view)?;
        }
        CommandView::ListingMutation(view) => {
            render_listing_mutation(stdout, view)?;
        }
        CommandView::ListingNew(view) => {
            render_listing_new(stdout, view)?;
        }
        CommandView::ListingValidate(view) => {
            render_listing_validate(stdout, view)?;
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
        CommandView::OrderCancel(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::OrderGet(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::OrderHistory(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::OrderList(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::OrderNew(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::OrderSubmit(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::OrderWatch(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RpcSessions(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RpcStatus(view) => {
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
        CommandView::JobGet(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::JobList(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::JobWatch(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::ListingGet(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::ListingMutation(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::ListingNew(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::ListingValidate(view) => {
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
        CommandView::JobList(view) => {
            for job in &view.jobs {
                serde_json::to_writer(&mut *stdout, job)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::JobWatch(view) => {
            for frame in &view.frames {
                serde_json::to_writer(&mut *stdout, frame)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::OrderHistory(view) => {
            for order in &view.orders {
                serde_json::to_writer(&mut *stdout, order)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::OrderList(view) => {
            for order in &view.orders {
                serde_json::to_writer(&mut *stdout, order)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::OrderWatch(view) => {
            for frame in &view.frames {
                serde_json::to_writer(&mut *stdout, frame)?;
                writeln!(stdout)?;
            }
            Ok(())
        }
        CommandView::RpcSessions(view) => {
            for session in &view.sessions {
                serde_json::to_writer(&mut *stdout, session)?;
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
            "secret backend",
            view.account.secret_backend.configured_primary.as_str(),
        ),
        (
            "legacy import path",
            view.account.legacy_identity_path.as_str(),
        ),
    ];
    if let Some(fallback) = &view.account.secret_backend.configured_fallback {
        account_rows.push(("secret fallback", fallback.as_str()));
    }
    account_rows.push(("secret status", view.account.secret_backend.state.as_str()));
    if let Some(active_backend) = &view.account.secret_backend.active_backend {
        account_rows.push(("active secret backend", active_backend.as_str()));
    }
    account_rows.push((
        "used secret fallback",
        yes_no(view.account.secret_backend.used_fallback),
    ));
    if let Some(selector) = &view.account.selector {
        account_rows.insert(0, ("selector", selector.as_str()));
    }
    render_pairs(stdout, "account", account_rows.as_slice())?;
    if let Some(reason) = &view.account.secret_backend.reason {
        writeln!(stdout, "account secret backend reason: {reason}")?;
    }
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
    render_pairs(
        stdout,
        "rpc",
        &[
            ("url", view.rpc.url.as_str()),
            (
                "bridge auth configured",
                yes_no(view.rpc.bridge_auth_configured),
            ),
        ],
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

fn render_job_list(stdout: &mut dyn Write, view: &JobListView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "ready" => format!(
            "activity · {} job{}",
            view.count,
            if view.count == 1 { "" } else { "s" }
        ),
        "empty" => "activity · no retained jobs".to_owned(),
        "unconfigured" => "activity · jobs unconfigured".to_owned(),
        "unavailable" => "activity · jobs unavailable".to_owned(),
        _ => "activity · jobs error".to_owned(),
    };
    write_context(stdout, context.as_str())?;
    if view.jobs.is_empty() {
        if let Some(reason) = &view.reason {
            writeln!(stdout, "{reason}")?;
            writeln!(stdout)?;
        }
    } else {
        let table = Table {
            headers: &["job", "type", "state", "signer", "updated"],
            rows: view
                .jobs
                .iter()
                .map(|job| {
                    let updated_at = job.completed_at_unix.unwrap_or(job.requested_at_unix);
                    vec![
                        job.id.clone(),
                        job.command.clone(),
                        job.state.clone(),
                        job.signer.clone(),
                        crate::runtime::job::format_timestamp(updated_at),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "rpc url: {}", view.rpc_url)?;
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_job_get(stdout: &mut dyn Write, view: &JobGetView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("activity · {}", view.lookup).as_str())?;
    if let Some(job) = &view.job {
        render_owned_pairs(
            stdout,
            "job",
            &[
                ("id", job.id.clone()),
                ("type", job.command.clone()),
                ("state", job.state.clone()),
                ("signer", job.signer.clone()),
                (
                    "requested",
                    crate::runtime::job::format_timestamp(job.requested_at_unix),
                ),
                (
                    "completed",
                    job.completed_at_unix
                        .map(crate::runtime::job::format_timestamp)
                        .unwrap_or_else(|| "pending".to_owned()),
                ),
                ("terminal", yes_no(job.terminal).to_owned()),
                (
                    "recovered after restart",
                    yes_no(job.recovered_after_restart).to_owned(),
                ),
                ("delivery policy", job.delivery_policy.clone()),
                ("relay outcome", job.relay_outcome_summary.clone()),
            ],
        )?;
        if !job.attempt_summaries.is_empty() {
            writeln!(stdout, "attempts")?;
            for attempt in &job.attempt_summaries {
                writeln!(stdout, "  {attempt}")?;
            }
            writeln!(stdout)?;
        }
    } else if let Some(reason) = &view.reason {
        writeln!(stdout, "{reason}")?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "rpc url: {}", view.rpc_url)?;
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_job_watch(stdout: &mut dyn Write, view: &JobWatchView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("activity · watch {}", view.job_id).as_str())?;
    if view.frames.is_empty() {
        if let Some(reason) = &view.reason {
            writeln!(stdout, "{reason}")?;
            writeln!(stdout)?;
        } else {
            writeln!(stdout, "no frames collected")?;
            writeln!(stdout)?;
        }
    } else {
        let table = Table {
            headers: &["frame", "time", "state", "terminal", "summary"],
            rows: view
                .frames
                .iter()
                .map(|frame| {
                    vec![
                        frame.sequence.to_string(),
                        crate::runtime::job::format_clock(frame.observed_at_unix),
                        frame.state.clone(),
                        yes_no(frame.terminal).to_owned(),
                        frame.summary.clone(),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "interval ms: {}", view.interval_ms)?;
    writeln!(stdout, "rpc url: {}", view.rpc_url)?;
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_new(stdout: &mut dyn Write, view: &OrderNewView) -> Result<(), RuntimeError> {
    write_context(stdout, "order · draft created")?;
    let mut rows = vec![
        ("order id", view.order_id.as_str()),
        ("file", view.file.as_str()),
        ("ready for submit", yes_no(view.ready_for_submit)),
    ];
    if let Some(listing_lookup) = &view.listing_lookup {
        rows.push(("listing", listing_lookup.as_str()));
    }
    if let Some(listing_addr) = &view.listing_addr {
        rows.push(("listing addr", listing_addr.as_str()));
    }
    if let Some(account_id) = &view.buyer_account_id {
        rows.push(("buyer account", account_id.as_str()));
    }
    if let Some(buyer_pubkey) = &view.buyer_pubkey {
        rows.push(("buyer pubkey", buyer_pubkey.as_str()));
    }
    if let Some(seller_pubkey) = &view.seller_pubkey {
        rows.push(("seller pubkey", seller_pubkey.as_str()));
    }
    render_pairs(stdout, "draft", rows.as_slice())?;
    render_order_items(stdout, &view.items)?;
    render_order_issues(stdout, &view.issues)?;
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_get(stdout: &mut dyn Write, view: &OrderGetView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "missing" => format!("order · {} missing", view.lookup),
        "submitted" => format!("order · {} submitted", view.lookup),
        "ready" => format!("order · {} ready", view.lookup),
        "draft" => format!("order · {} draft", view.lookup),
        "error" => format!("order · {} error", view.lookup),
        _ => format!("order · {}", view.lookup),
    };
    write_context(stdout, context.as_str())?;

    if view.state == "missing" || view.state == "error" {
        if let Some(reason) = &view.reason {
            writeln!(stdout, "{reason}")?;
            writeln!(stdout)?;
        }
        if let Some(file) = &view.file {
            writeln!(stdout, "file: {file}")?;
        }
        writeln!(stdout, "source: {}", view.source)?;
        render_actions(stdout, &view.actions)?;
        return Ok(());
    }

    let mut rows = Vec::<(&str, &str)>::new();
    if let Some(order_id) = &view.order_id {
        rows.push(("order id", order_id.as_str()));
    }
    if let Some(file) = &view.file {
        rows.push(("file", file.as_str()));
    }
    rows.push(("ready for submit", yes_no(view.ready_for_submit)));
    if let Some(listing_lookup) = &view.listing_lookup {
        rows.push(("listing", listing_lookup.as_str()));
    }
    if let Some(listing_addr) = &view.listing_addr {
        rows.push(("listing addr", listing_addr.as_str()));
    }
    if let Some(account_id) = &view.buyer_account_id {
        rows.push(("buyer account", account_id.as_str()));
    }
    if let Some(buyer_pubkey) = &view.buyer_pubkey {
        rows.push(("buyer pubkey", buyer_pubkey.as_str()));
    }
    if let Some(seller_pubkey) = &view.seller_pubkey {
        rows.push(("seller pubkey", seller_pubkey.as_str()));
    }
    render_pairs(stdout, "order", rows.as_slice())?;
    if let Some(updated_at_unix) = view.updated_at_unix {
        writeln!(
            stdout,
            "updated: {}",
            crate::runtime::job::format_timestamp(updated_at_unix)
        )?;
    }
    render_order_items(stdout, &view.items)?;
    if let Some(job) = &view.job {
        render_order_job(stdout, job)?;
    }
    render_order_issues(stdout, &view.issues)?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_list(stdout: &mut dyn Write, view: &OrderListView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "empty" => "orders · no local drafts".to_owned(),
        "degraded" => format!("orders · {} local drafts with issues", view.count),
        _ => format!(
            "orders · {} local draft{}",
            view.count,
            if view.count == 1 { "" } else { "s" }
        ),
    };
    write_context(stdout, context.as_str())?;
    if view.orders.is_empty() {
        writeln!(stdout, "no order drafts found")?;
        writeln!(stdout)?;
    } else {
        let table = Table {
            headers: &["order", "listing", "state", "ready", "job", "updated"],
            rows: view
                .orders
                .iter()
                .map(|order| {
                    vec![
                        order.id.clone(),
                        order
                            .listing_lookup
                            .clone()
                            .or_else(|| order.listing_addr.clone())
                            .unwrap_or_default(),
                        order.state.clone(),
                        yes_no(order.ready_for_submit).to_owned(),
                        order
                            .job
                            .as_ref()
                            .map(|job| job.state.clone())
                            .unwrap_or_default(),
                        crate::runtime::job::format_timestamp(order.updated_at_unix),
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

fn render_order_submit(stdout: &mut dyn Write, view: &OrderSubmitView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "missing" => format!("order · {} missing", view.order_id),
        "already_submitted" => format!("order · {} already submitted", view.order_id),
        "unconfigured" => format!("order · {} not ready", view.order_id),
        "unavailable" => format!("order · {} submit unavailable", view.order_id),
        "error" => format!("order · {} error", view.order_id),
        "dry_run" => format!("order · {} dry run", view.order_id),
        "deduplicated" => format!("order · {} deduplicated", view.order_id),
        _ => format!("order · {} submitted", view.order_id),
    };
    write_context(stdout, context.as_str())?;

    let mut rows = vec![
        ("order id", view.order_id.as_str()),
        ("file", view.file.as_str()),
    ];
    if let Some(listing_lookup) = &view.listing_lookup {
        rows.push(("listing", listing_lookup.as_str()));
    }
    if let Some(listing_addr) = &view.listing_addr {
        rows.push(("listing addr", listing_addr.as_str()));
    }
    if let Some(account_id) = &view.buyer_account_id {
        rows.push(("buyer account", account_id.as_str()));
    }
    if let Some(buyer_pubkey) = &view.buyer_pubkey {
        rows.push(("buyer pubkey", buyer_pubkey.as_str()));
    }
    if let Some(seller_pubkey) = &view.seller_pubkey {
        rows.push(("seller pubkey", seller_pubkey.as_str()));
    }
    if view.dry_run {
        rows.push(("dry run", yes_no(true)));
    }
    if view.deduplicated {
        rows.push(("deduplicated", yes_no(true)));
    }
    if let Some(idempotency_key) = &view.idempotency_key {
        rows.push(("idempotency key", idempotency_key.as_str()));
    }
    render_pairs(stdout, "order", rows.as_slice())?;
    if let Some(job) = &view.job {
        render_order_job(stdout, job)?;
    }
    render_order_issues(stdout, &view.issues)?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_watch(stdout: &mut dyn Write, view: &OrderWatchView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "missing" => format!("order · {} watch missing", view.order_id),
        "not_submitted" => format!("order · {} not submitted", view.order_id),
        "unconfigured" => format!("order · {} watch unconfigured", view.order_id),
        "unavailable" => format!("order · {} watch unavailable", view.order_id),
        "error" => format!("order · {} watch error", view.order_id),
        "watching" => format!("order · {} watching", view.order_id),
        _ => format!("order · {} {}", view.order_id, view.state),
    };
    write_context(stdout, context.as_str())?;

    let interval = format!("{} ms", view.interval_ms);
    let mut rows = vec![("order id", view.order_id.clone()), ("interval", interval)];
    if let Some(job_id) = &view.job_id {
        rows.push(("job id", job_id.clone()));
    }
    render_owned_pairs(stdout, "watch", rows.as_slice())?;
    if !view.frames.is_empty() {
        let table = Table {
            headers: &["frame", "time", "state", "terminal", "summary"],
            rows: view
                .frames
                .iter()
                .map(|frame| {
                    vec![
                        frame.sequence.to_string(),
                        crate::runtime::job::format_clock(frame.observed_at_unix),
                        frame.state.clone(),
                        yes_no(frame.terminal).to_owned(),
                        frame.summary.clone(),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_history(
    stdout: &mut dyn Write,
    view: &OrderHistoryView,
) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "empty" => "order history · no submitted orders".to_owned(),
        _ => format!(
            "order history · {} submitted order{}",
            view.count,
            if view.count == 1 { "" } else { "s" }
        ),
    };
    write_context(stdout, context.as_str())?;
    if view.orders.is_empty() {
        if let Some(reason) = &view.reason {
            writeln!(stdout, "{reason}")?;
            writeln!(stdout)?;
        }
    } else {
        let table = Table {
            headers: &["order", "listing", "state", "job", "submitted", "updated"],
            rows: view
                .orders
                .iter()
                .map(|order| {
                    vec![
                        order.id.clone(),
                        order
                            .listing_lookup
                            .clone()
                            .or_else(|| order.listing_addr.clone())
                            .unwrap_or_default(),
                        order.state.clone(),
                        order
                            .job
                            .as_ref()
                            .map(|job| job.job_id.clone())
                            .unwrap_or_default(),
                        order
                            .submitted_at_unix
                            .map(crate::runtime::job::format_timestamp)
                            .unwrap_or_default(),
                        crate::runtime::job::format_timestamp(order.updated_at_unix),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
        if let Some(reason) = &view.reason {
            writeln!(stdout, "note: {reason}")?;
        }
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_cancel(stdout: &mut dyn Write, view: &OrderCancelView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "missing" => format!("order · {} missing", view.lookup),
        "not_submitted" => format!("order · {} not submitted", view.lookup),
        "unconfigured" => format!("order · {} cancel unavailable", view.lookup),
        "unavailable" => format!("order · {} cancel unavailable", view.lookup),
        "error" => format!("order · {} cancel error", view.lookup),
        _ => format!("order · {} cancel", view.lookup),
    };
    write_context(stdout, context.as_str())?;
    let mut rows = vec![("lookup", view.lookup.as_str())];
    if let Some(order_id) = &view.order_id {
        rows.push(("order id", order_id.as_str()));
    }
    render_pairs(stdout, "order", rows.as_slice())?;
    if let Some(job) = &view.job {
        render_order_job(stdout, job)?;
    }
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_order_items(
    stdout: &mut dyn Write,
    items: &[OrderDraftItemView],
) -> Result<(), RuntimeError> {
    if items.is_empty() {
        writeln!(stdout, "items: no line items yet")?;
        writeln!(stdout)?;
        return Ok(());
    }

    let table = Table {
        headers: &["bin", "qty"],
        rows: items
            .iter()
            .map(|item| vec![item.bin_id.clone(), item.bin_count.to_string()])
            .collect(),
    };
    render_table(stdout, &table)?;
    writeln!(stdout)?;
    Ok(())
}

fn render_order_job(stdout: &mut dyn Write, job: &OrderJobView) -> Result<(), RuntimeError> {
    let mut rows = vec![
        ("job id", job.job_id.as_str()),
        ("state", job.state.as_str()),
    ];
    if let Some(command) = &job.command {
        rows.push(("command", command.as_str()));
    }
    if let Some(event_id) = &job.event_id {
        rows.push(("event id", event_id.as_str()));
    }
    if let Some(event_addr) = &job.event_addr {
        rows.push(("event addr", event_addr.as_str()));
    }
    render_pairs(stdout, "job", rows.as_slice())?;
    if let Some(reason) = &job.reason {
        writeln!(stdout, "job reason: {reason}")?;
    }
    Ok(())
}

fn render_order_issues(
    stdout: &mut dyn Write,
    issues: &[crate::domain::runtime::OrderIssueView],
) -> Result<(), RuntimeError> {
    if issues.is_empty() {
        return Ok(());
    }

    writeln!(stdout, "issues")?;
    for issue in issues {
        writeln!(stdout, "  {}  {}", issue.field, issue.message)?;
    }
    writeln!(stdout)?;
    Ok(())
}

fn render_listing_new(stdout: &mut dyn Write, view: &ListingNewView) -> Result<(), RuntimeError> {
    write_context(stdout, "listing · draft created")?;
    let mut rows = vec![
        ("file", view.file.as_str()),
        ("listing id", view.listing_id.as_str()),
    ];
    if let Some(account_id) = &view.selected_account_id {
        rows.push(("account id", account_id.as_str()));
    }
    if let Some(seller_pubkey) = &view.seller_pubkey {
        rows.push(("seller", seller_pubkey.as_str()));
    }
    if let Some(farm_d_tag) = &view.farm_d_tag {
        rows.push(("farm d_tag", farm_d_tag.as_str()));
    }
    render_pairs(stdout, "draft", rows.as_slice())?;
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_listing_validate(
    stdout: &mut dyn Write,
    view: &ListingValidateView,
) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        match view.state.as_str() {
            "valid" => "listing · valid",
            _ => "listing · invalid",
        },
    )?;
    let status = if view.valid {
        "ready to publish"
    } else {
        "needs edits"
    };
    let mut rows = vec![("file", view.file.as_str()), ("status", status)];
    if let Some(listing_id) = &view.listing_id {
        rows.push(("listing id", listing_id.as_str()));
    }
    if let Some(seller_pubkey) = &view.seller_pubkey {
        rows.push(("seller", seller_pubkey.as_str()));
    }
    if let Some(farm_d_tag) = &view.farm_d_tag {
        rows.push(("farm d_tag", farm_d_tag.as_str()));
    }
    render_pairs(stdout, "validation", rows.as_slice())?;
    if !view.issues.is_empty() {
        writeln!(stdout, "issues")?;
        for issue in &view.issues {
            match issue.line {
                Some(line) => writeln!(
                    stdout,
                    "  {field}  {message} (line {line})",
                    field = issue.field,
                    message = issue.message
                )?,
                None => writeln!(
                    stdout,
                    "  {field}  {message}",
                    field = issue.field,
                    message = issue.message
                )?,
            }
        }
        writeln!(stdout)?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_listing_get(stdout: &mut dyn Write, view: &ListingGetView) -> Result<(), RuntimeError> {
    let context = view
        .listing_id
        .clone()
        .unwrap_or_else(|| view.lookup.clone());
    write_context(stdout, format!("listing · {context}").as_str())?;

    match view.state.as_str() {
        "unconfigured" | "missing" => {
            if let Some(reason) = &view.reason {
                writeln!(stdout, "{reason}")?;
            }
        }
        _ => {
            if let Some(title) = &view.title {
                writeln!(stdout, "{title}")?;
                writeln!(stdout)?;
            }
            let mut rows = Vec::<(&str, String)>::new();
            if let Some(product_key) = &view.product_key {
                rows.push(("key", product_key.clone()));
            }
            if let Some(category) = &view.category {
                rows.push(("category", category.clone()));
            }
            if let Some(price) = &view.price {
                rows.push((
                    "price",
                    format_price(
                        price.amount,
                        &price.currency,
                        price.per_amount,
                        &price.per_unit,
                    ),
                ));
            }
            if let Some(available) = &view.available {
                rows.push((
                    "available",
                    format_available(
                        available.available_amount.unwrap_or(available.total_amount),
                        available
                            .label
                            .as_deref()
                            .unwrap_or(available.total_unit.as_str()),
                    ),
                ));
            }
            if let Some(location_primary) = &view.location_primary {
                rows.push(("location", location_primary.clone()));
            }
            if let Some(listing_id) = &view.listing_id {
                rows.push(("listing id", listing_id.clone()));
            }
            render_owned_pairs(stdout, "listing", rows.as_slice())?;
            if let Some(description) = &view.description {
                writeln!(stdout, "{description}")?;
                writeln!(stdout)?;
            }
            writeln!(
                stdout,
                "provenance: local replica · {} · {}",
                view.provenance.freshness,
                relay_count_text(view.provenance.relay_count)
            )?;
            writeln!(stdout, "source: {}", view.source)?;
        }
    }

    if view.state != "ready" {
        writeln!(stdout)?;
        writeln!(stdout, "source: {}", view.source)?;
    }
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_listing_mutation(
    stdout: &mut dyn Write,
    view: &ListingMutationView,
) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "dry_run" => format!("listing · {} dry run", view.operation),
        "deduplicated" => format!("listing · {} deduplicated", view.operation),
        "published" => format!("listing · {} completed", view.operation),
        "failed" | "unavailable" => format!("listing · {} unavailable", view.operation),
        "unconfigured" => format!("listing · {} unconfigured", view.operation),
        "error" => format!("listing · {} error", view.operation),
        other => format!("listing · {} {other}", view.operation),
    };
    write_context(stdout, context.as_str())?;

    let mut rows = vec![
        ("file", view.file.as_str()),
        ("listing id", view.listing_id.as_str()),
        ("event addr", view.listing_addr.as_str()),
    ];
    if let Some(job_id) = &view.job_id {
        rows.push(("job id", job_id.as_str()));
    }
    if let Some(job_status) = &view.job_status {
        rows.push(("status", job_status.as_str()));
    }
    if let Some(event_id) = &view.event_id {
        rows.push(("event id", event_id.as_str()));
    }
    if let Some(signer_mode) = &view.signer_mode {
        rows.push(("signer", signer_mode.as_str()));
    }
    render_pairs(stdout, "listing", rows.as_slice())?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;

    if let Some(job) = &view.job {
        writeln!(stdout)?;
        writeln!(stdout, "job preview")?;
        let job_json = serde_json::to_string_pretty(job)?;
        for line in job_json.lines() {
            writeln!(stdout, "  {line}")?;
        }
    }
    if let Some(event) = &view.event {
        writeln!(stdout)?;
        writeln!(stdout, "event preview")?;
        let event_json = serde_json::to_string_pretty(event)?;
        for line in event_json.lines() {
            writeln!(stdout, "  {line}")?;
        }
    }
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

fn render_rpc_status(stdout: &mut dyn Write, view: &RpcStatusView) -> Result<(), RuntimeError> {
    write_context(stdout, format!("rpc · {}", view.state).as_str())?;
    let mut rows = vec![("url", view.url.as_str()), ("status", view.state.as_str())];
    if let Some(auth_mode) = &view.auth_mode {
        rows.push(("auth mode", auth_mode.as_str()));
    }
    if let Some(signer_mode) = &view.signer_mode {
        rows.push(("signer mode", signer_mode.as_str()));
    }
    if let Some(default_signer_mode) = &view.default_signer_mode {
        rows.push(("default signer", default_signer_mode.as_str()));
    }
    render_pairs(stdout, "rpc", rows.as_slice())?;

    let mut bridge_rows = Vec::<(&str, String)>::new();
    if let Some(enabled) = view.bridge_enabled {
        bridge_rows.push(("bridge enabled", yes_no(enabled).to_owned()));
    }
    if let Some(ready) = view.bridge_ready {
        bridge_rows.push(("bridge ready", yes_no(ready).to_owned()));
    }
    if let Some(relay_count) = view.relay_count {
        bridge_rows.push(("relay count", relay_count.to_string()));
    }
    if let Some(retained_jobs) = view.retained_jobs {
        bridge_rows.push(("retained jobs", retained_jobs.to_string()));
    }
    if let Some(job_status_retention) = view.job_status_retention {
        bridge_rows.push(("job retention", job_status_retention.to_string()));
    }
    if !bridge_rows.is_empty() {
        render_owned_pairs(stdout, "bridge", bridge_rows.as_slice())?;
    }
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_rpc_sessions(stdout: &mut dyn Write, view: &RpcSessionsView) -> Result<(), RuntimeError> {
    let context = match view.state.as_str() {
        "ready" => format!(
            "rpc · {} session{}",
            view.count,
            if view.count == 1 { "" } else { "s" }
        ),
        "empty" => "rpc · no public sessions".to_owned(),
        "unconfigured" => "rpc · sessions unconfigured".to_owned(),
        "unavailable" => "rpc · sessions unavailable".to_owned(),
        _ => "rpc · sessions error".to_owned(),
    };
    write_context(stdout, context.as_str())?;
    if view.sessions.is_empty() {
        if let Some(reason) = &view.reason {
            writeln!(stdout, "{reason}")?;
            writeln!(stdout)?;
        }
    } else {
        let table = Table {
            headers: &["session", "role", "auth", "authorized", "relays", "expires"],
            rows: view
                .sessions
                .iter()
                .map(|session| {
                    vec![
                        session.session_id.clone(),
                        session.role.clone(),
                        yes_no(session.auth_required).to_owned(),
                        yes_no(session.authorized).to_owned(),
                        session.relay_count.to_string(),
                        session
                            .expires_in_secs
                            .map(|secs| format!("{secs}s"))
                            .unwrap_or_else(|| "n/a".to_owned()),
                    ]
                })
                .collect(),
        };
        render_table(stdout, &table)?;
        writeln!(stdout)?;
    }
    writeln!(stdout, "rpc url: {}", view.url)?;
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
    writeln!(stdout, "  backend: {}", local.backend)?;
    writeln!(stdout, "  used fallback: {}", yes_no(local.used_fallback))?;
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
        CommandView::JobGet(_) => "job get",
        CommandView::JobList(_) => "job ls",
        CommandView::JobWatch(_) => "job watch",
        CommandView::ListingGet(_) => "listing get",
        CommandView::ListingMutation(view) => match view.operation.as_str() {
            "publish" => "listing publish",
            "update" => "listing update",
            "archive" => "listing archive",
            _ => "listing publish",
        },
        CommandView::ListingNew(_) => "listing new",
        CommandView::ListingValidate(_) => "listing validate",
        CommandView::LocalBackup(_) => "local backup",
        CommandView::LocalExport(_) => "local export",
        CommandView::LocalInit(_) => "local init",
        CommandView::LocalStatus(_) => "local status",
        CommandView::MycStatus(_) => "myc status",
        CommandView::NetStatus(_) => "net status",
        CommandView::OrderCancel(_) => "order cancel",
        CommandView::OrderGet(_) => "order get",
        CommandView::OrderHistory(_) => "order history",
        CommandView::OrderList(_) => "order ls",
        CommandView::OrderNew(_) => "order new",
        CommandView::OrderSubmit(_) => "order submit",
        CommandView::OrderWatch(_) => "order watch",
        CommandView::RpcSessions(_) => "rpc sessions",
        CommandView::RpcStatus(_) => "rpc status",
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
        OutputFormat, PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig,
        RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime::logging::LoggingState;
    use radroots_secret_vault::RadrootsSecretBackend;

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
                    secret_backend: RadrootsSecretBackend::EncryptedFile,
                    secret_fallback: None,
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
                rpc: RpcConfig {
                    url: "http://127.0.0.1:7070".to_owned(),
                    bridge_bearer_token: None,
                },
            },
            &LoggingState {
                initialized: true,
                current_file: None,
            },
        )
        .expect("runtime show");
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
        let output = CommandOutput::success(CommandView::ConfigShow(
            runtime::show(
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
                        secret_backend: RadrootsSecretBackend::EncryptedFile,
                        secret_fallback: None,
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
                        replica_db_path:
                            "/home/tester/.local/share/radroots/replica/replica.sqlite".into(),
                        backups_dir: "/home/tester/.local/share/radroots/replica/backups".into(),
                        exports_dir: "/home/tester/.local/share/radroots/replica/exports".into(),
                    },
                    myc: MycConfig {
                        executable: "myc".into(),
                    },
                    rpc: RpcConfig {
                        url: "http://127.0.0.1:7070".to_owned(),
                        bridge_bearer_token: None,
                    },
                },
                &LoggingState {
                    initialized: true,
                    current_file: None,
                },
            )
            .expect("runtime show"),
        ));
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
