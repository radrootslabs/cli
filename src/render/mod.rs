use std::io::{self, Write};

use crate::domain::runtime::{
    AccountClearDefaultView, AccountImportView, AccountListView, AccountRemoveView,
    AccountSummaryView, CommandOutput, CommandView, DoctorCheckView, DoctorView,
    FarmConfigSummaryView, FarmGetView, FarmPublishComponentView, FarmPublishView, FarmSetView,
    FarmSetupView, FarmStatusView, FindView, JobGetView, JobListView, JobWatchView, ListingGetView,
    ListingMutationView, ListingNewView, ListingValidateView, LocalBackupView, LocalExportView,
    LocalInitView, LocalStatusView, NetStatusView, OrderCancelView, OrderDraftItemView,
    OrderGetView, OrderHistoryView, OrderJobView, OrderListView, OrderNewView, OrderSubmitView,
    OrderSubmitWatchView, OrderWatchView, OrderWorkflowView, RelayListView, RpcSessionsView,
    RpcStatusView, RuntimeActionView, RuntimeLogsView, RuntimeManagedConfigView, RuntimeStatusView,
    SellAddView, SellCheckView, SellDraftMutationView, SellMutationView, SellShowView, SetupView,
    StatusView, SyncActionView, SyncStatusView, SyncWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::{OutputConfig, OutputFormat, Verbosity};

const THIN_RULE: &str = "────────────────────────────────────────────────────";

pub fn render_output(output: &CommandOutput, config: &OutputConfig) -> Result<(), RuntimeError> {
    match config.format {
        OutputFormat::Human => render_human(output, config),
        OutputFormat::Json => render_json(output),
        OutputFormat::Ndjson => render_ndjson(output),
    }
}

fn render_human(output: &CommandOutput, config: &OutputConfig) -> Result<(), RuntimeError> {
    let mut stdout = io::stdout().lock();
    render_human_with_config_to(&mut stdout, output, config)
}

#[cfg(test)]
fn render_human_to(stdout: &mut dyn Write, output: &CommandOutput) -> Result<(), RuntimeError> {
    render_human_with_config_to(stdout, output, &default_human_output_config())
}

fn render_human_with_config_to(
    stdout: &mut dyn Write,
    output: &CommandOutput,
    config: &OutputConfig,
) -> Result<(), RuntimeError> {
    if config.verbosity == Verbosity::Quiet {
        if let Some(quiet) = render_quiet_output(output) {
            writeln!(stdout, "{quiet}")?;
            return Ok(());
        }
    }

    let mut buffer = Vec::new();
    render_human_view_to(&mut buffer, output)?;
    let rendered = String::from_utf8(buffer).map_err(|error| {
        RuntimeError::Config(format!("human render output was not utf8: {error}"))
    })?;
    let finalized = finalize_human_output(output, rendered, config)?;
    write!(stdout, "{finalized}")?;
    Ok(())
}

#[cfg(test)]
fn default_human_output_config() -> OutputConfig {
    OutputConfig {
        format: OutputFormat::Human,
        verbosity: Verbosity::Normal,
        color: true,
        dry_run: false,
    }
}

fn render_human_view_to(
    stdout: &mut dyn Write,
    output: &CommandOutput,
) -> Result<(), RuntimeError> {
    match output.view() {
        CommandView::AccountClearDefault(view) => render_account_clear_default(stdout, view)?,
        CommandView::AccountImport(view) => render_account_import(stdout, view)?,
        CommandView::AccountList(view) => render_account_list(stdout, view)?,
        CommandView::AccountNew(view) => render_account_new(stdout, view)?,
        CommandView::AccountRemove(view) => render_account_remove(stdout, view)?,
        CommandView::AccountUse(view) => render_account_use(stdout, view)?,
        CommandView::AccountWhoami(view) => render_account_whoami(stdout, view)?,
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
        CommandView::OrderSubmitWatch(view) => {
            render_order_submit_watch(stdout, view)?;
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
        CommandView::FarmGet(view) => {
            render_farm_get(stdout, view)?;
        }
        CommandView::FarmPublish(view) => {
            render_farm_publish(stdout, view)?;
        }
        CommandView::FarmSet(view) => {
            render_farm_set(stdout, view)?;
        }
        CommandView::FarmSetup(view) => {
            render_farm_setup(stdout, view)?;
        }
        CommandView::FarmStatus(view) => {
            render_farm_status(stdout, view)?;
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
        CommandView::MarketSearch(view) => {
            render_market_search(stdout, view)?;
        }
        CommandView::MarketUpdate(view) => {
            render_market_update(stdout, view)?;
        }
        CommandView::MarketView(view) => {
            render_market_view(stdout, view)?;
        }
        CommandView::RelayList(view) => {
            render_relay_list(stdout, view)?;
        }
        CommandView::RuntimeAction(view) => {
            render_runtime_action(stdout, view)?;
        }
        CommandView::RuntimeConfigShow(view) => {
            render_runtime_config_show(stdout, view)?;
        }
        CommandView::RuntimeLogs(view) => {
            render_runtime_logs(stdout, view)?;
        }
        CommandView::RuntimeStatus(view) => {
            render_runtime_status(stdout, view)?;
        }
        CommandView::SellAdd(view) => {
            render_sell_add(stdout, view)?;
        }
        CommandView::SellCheck(view) => {
            render_sell_check(stdout, view)?;
        }
        CommandView::SellDraftMutation(view) => {
            render_sell_draft_mutation(stdout, view)?;
        }
        CommandView::SellMutation(view) => {
            render_sell_mutation(stdout, view)?;
        }
        CommandView::SellShow(view) => {
            render_sell_show(stdout, view)?;
        }
        CommandView::Setup(view) => {
            render_setup(stdout, view)?;
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
            if let Some(account_id) = &view.signer_account_id {
                signer_rows.push(("signer account id", account_id.as_str()));
            }
            render_pairs(stdout, "signer", signer_rows.as_slice())?;
            writeln!(stdout)?;
            render_account_resolution(stdout, &view.account_resolution)?;
            if let Some(reason) = &view.reason {
                writeln!(stdout, "reason: {reason}")?;
            }
            writeln!(stdout, "source: {}", view.source)?;
            writeln!(stdout)?;
            render_signer_binding(stdout, &view.binding)?;
            if let Some(local) = &view.local {
                writeln!(stdout)?;
                render_local_signer(stdout, "local account", local)?;
            }
            if let Some(myc) = &view.myc {
                writeln!(stdout)?;
                render_myc_status(stdout, myc, false)?;
            }
        }
        CommandView::Status(view) => {
            render_status_summary(stdout, view)?;
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
        CommandView::AccountClearDefault(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountImport(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountList(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountNew(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::AccountRemove(view) => {
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
        CommandView::OrderSubmitWatch(view) => {
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
        CommandView::FarmGet(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::FarmPublish(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::FarmSet(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::FarmSetup(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::FarmStatus(view) => {
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
        CommandView::MarketSearch(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::MarketUpdate(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::MarketView(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RelayList(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RuntimeAction(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RuntimeConfigShow(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RuntimeLogs(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::RuntimeStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SellAdd(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SellCheck(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SellDraftMutation(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SellMutation(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SellShow(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::Setup(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::SignerStatus(view) => {
            serde_json::to_writer_pretty(&mut *stdout, view)?;
            writeln!(stdout)?;
        }
        CommandView::Status(view) => {
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
        CommandView::MarketSearch(view) => {
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

fn render_quiet_output(output: &CommandOutput) -> Option<String> {
    match output.view() {
        CommandView::AccountClearDefault(view) => Some(match &view.cleared_account {
            Some(account) => format!("Default account cleared: {}", account.id),
            None => "No default account configured".to_owned(),
        }),
        CommandView::AccountImport(view) => Some(format!("Account imported: {}", view.account.id)),
        CommandView::AccountNew(view) => Some(format!(
            "{}: {}",
            match view.state.as_str() {
                "migrated" => "Account migrated",
                _ => "Account created",
            },
            view.account.id
        )),
        CommandView::AccountRemove(view) => {
            Some(format!("Account removed: {}", view.removed_account.id))
        }
        CommandView::Find(view) | CommandView::MarketSearch(view) => match view.state.as_str() {
            "ready" if !view.results.is_empty() => Some(
                view.results
                    .iter()
                    .map(|result| result.product_key.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            "empty" => Some("No listings found".to_owned()),
            _ => None,
        },
        CommandView::OrderSubmit(view) => match view.state.as_str() {
            "accepted" | "submitted" | "already_submitted" | "deduplicated" => {
                Some(format!("Order submitted: {}", view.order_id))
            }
            _ => None,
        },
        _ => None,
    }
}

fn finalize_human_output(
    output: &CommandOutput,
    rendered: String,
    config: &OutputConfig,
) -> Result<String, RuntimeError> {
    let mut cleaned_lines = Vec::new();
    let mut fallback_details = Vec::<(&'static str, String)>::new();

    for line in rendered.lines() {
        let trimmed = line.trim_end();
        if trimmed == THIN_RULE {
            continue;
        }
        if let Some(value) = trimmed.trim_start().strip_prefix("workflow source: ") {
            fallback_details.push(("Workflow source", value.to_owned()));
            continue;
        }
        if let Some(value) = trimmed.trim_start().strip_prefix("source: ") {
            fallback_details.push(("Source", value.to_owned()));
            continue;
        }
        if let Some(value) = trimmed.trim_start().strip_prefix("provenance: ") {
            fallback_details.push(("Provenance", value.to_owned()));
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("reason: ") {
            cleaned_lines.push(value.to_owned());
            continue;
        }
        if trimmed == "actions" {
            cleaned_lines.push("Next".to_owned());
            continue;
        }
        cleaned_lines.push(trimmed.to_owned());
    }

    let cleaned_lines = collapse_blank_lines(cleaned_lines);
    let mut finalized = cleaned_lines.join("\n");
    if !finalized.is_empty() && !finalized.ends_with('\n') {
        finalized.push('\n');
    }

    if matches!(config.verbosity, Verbosity::Verbose | Verbosity::Trace) {
        let mut details = verbose_details(output);
        for fallback in fallback_details {
            if !details.iter().any(|(label, _)| *label == fallback.0) {
                details.push(fallback);
            }
        }
        if !details.is_empty() {
            if !finalized.is_empty() {
                finalized.push('\n');
            }
            finalized.push_str("Details\n");
            finalized.push_str(render_field_rows_string(details.as_slice()).as_str());
        }
    }

    if config.verbosity == Verbosity::Trace {
        let mut trace_buffer = Vec::new();
        render_json_to(&mut trace_buffer, output)?;
        let trace_json = String::from_utf8(trace_buffer).map_err(|error| {
            RuntimeError::Config(format!("trace render output was not utf8: {error}"))
        })?;
        if !finalized.is_empty() {
            finalized.push('\n');
        }
        finalized.push_str("Trace\n");
        finalized.push_str(
            render_field_rows_string(&[("Command", human_command_name(output.view()).to_owned())])
                .as_str(),
        );
        for line in trace_json.trim_end().lines() {
            finalized.push_str("  ");
            finalized.push_str(line);
            finalized.push('\n');
        }
    }

    Ok(finalized)
}

fn collapse_blank_lines(lines: Vec<String>) -> Vec<String> {
    let mut collapsed = Vec::new();
    let mut previous_blank = true;
    for line in lines {
        let blank = line.trim().is_empty();
        if blank {
            if previous_blank {
                continue;
            }
            collapsed.push(String::new());
        } else {
            collapsed.push(line);
        }
        previous_blank = blank;
    }
    while collapsed.last().is_some_and(|line| line.trim().is_empty()) {
        collapsed.pop();
    }
    collapsed
}

fn render_field_rows_string(rows: &[(&str, String)]) -> String {
    let label_width = rows
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or_default();
    let mut rendered = String::new();
    for (label, value) in rows {
        rendered.push_str(
            format!(
                "  {label:label_width$}  {value}\n",
                label_width = label_width
            )
            .as_str(),
        );
    }
    rendered
}

fn verbose_details(output: &CommandOutput) -> Vec<(&'static str, String)> {
    match output.view() {
        CommandView::AccountClearDefault(view) => vec![
            ("Source", view.source.clone()),
            (
                "Remaining accounts",
                view.remaining_account_count.to_string(),
            ),
        ],
        CommandView::AccountImport(view) => vec![("Source", view.source.clone())],
        CommandView::AccountList(view) => vec![("Source", view.source.clone())],
        CommandView::AccountNew(view) => vec![("Source", view.source.clone())],
        CommandView::AccountRemove(view) => vec![
            ("Source", view.source.clone()),
            (
                "Remaining accounts",
                view.remaining_account_count.to_string(),
            ),
        ],
        CommandView::AccountUse(view) => vec![("Source", view.source.clone())],
        CommandView::AccountWhoami(view) => vec![("Source", view.source.clone())],
        CommandView::Doctor(view) => vec![("Source", view.source.clone())],
        CommandView::Find(view) | CommandView::MarketSearch(view) => vec![
            ("Source", view.source.clone()),
            ("Freshness", view.freshness.display.clone()),
            ("Relay count", view.relay_count.to_string()),
        ],
        CommandView::ListingGet(view) | CommandView::MarketView(view) => vec![
            ("Source", view.source.clone()),
            ("Freshness", view.provenance.freshness.clone()),
            ("Relay count", view.provenance.relay_count.to_string()),
        ],
        CommandView::OrderSubmit(view) => {
            let mut rows = vec![("Source", view.source.clone())];
            push_row(
                &mut rows,
                "Signer mode",
                view.signer_mode.as_deref().map(str::to_owned),
            );
            push_row(
                &mut rows,
                "Requested session",
                view.requested_signer_session_id
                    .as_deref()
                    .map(str::to_owned),
            );
            push_row(
                &mut rows,
                "Idempotency key",
                view.idempotency_key.as_deref().map(str::to_owned),
            );
            rows
        }
        CommandView::OrderSubmitWatch(view) => {
            let mut rows = vec![("Source", view.submit.source.clone())];
            push_row(
                &mut rows,
                "Signer mode",
                view.submit.signer_mode.as_deref().map(str::to_owned),
            );
            push_row(
                &mut rows,
                "Requested session",
                view.submit
                    .requested_signer_session_id
                    .as_deref()
                    .map(str::to_owned),
            );
            rows
        }
        CommandView::RelayList(view) => vec![
            ("Source", view.source.clone()),
            ("Relay count", view.count.to_string()),
            ("Publish policy", view.publish_policy.clone()),
        ],
        _ => Vec::new(),
    }
}

fn present_absent(value: bool) -> &'static str {
    if value { "present" } else { "absent" }
}

fn render_account_list(stdout: &mut dyn Write, view: &AccountListView) -> Result<(), RuntimeError> {
    if view.accounts.is_empty() {
        writeln!(stdout, "No accounts yet")?;
        if !view.actions.is_empty() {
            writeln!(stdout)?;
            render_item_section(stdout, "Next", &view.actions)?;
        }
        return Ok(());
    }

    writeln!(
        stdout,
        "{} account{}",
        view.count,
        if view.count == 1 { "" } else { "s" }
    )?;
    writeln!(stdout)?;
    for (index, account) in view.accounts.iter().enumerate() {
        writeln!(
            stdout,
            "{}",
            account
                .display_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(account.id.as_str())
        )?;
        let rows = vec![
            ("Account", account.id.clone()),
            ("Signer", humanize_machine_label(account.signer.as_str())),
            (
                "Default",
                if account.is_default {
                    "Yes".to_owned()
                } else {
                    "No".to_owned()
                },
            ),
        ];
        render_field_rows(stdout, rows.as_slice())?;
        if index + 1 < view.accounts.len() {
            writeln!(stdout)?;
        }
    }
    if !view.actions.is_empty() {
        writeln!(stdout)?;
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_account_import(
    stdout: &mut dyn Write,
    view: &AccountImportView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "Watch-only account imported")?;
    writeln!(stdout)?;
    render_account_section(stdout, &view.account)?;
    writeln!(stdout)?;
    writeln!(stdout, "Identity")?;
    render_field_rows(
        stdout,
        &[("npub", view.public_identity.public_key_npub.clone())],
    )?;
    if !view.actions.is_empty() {
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_account_new(
    stdout: &mut dyn Write,
    view: &crate::domain::runtime::AccountNewView,
) -> Result<(), RuntimeError> {
    writeln!(
        stdout,
        "{}",
        match view.state.as_str() {
            "migrated" => "Account migrated",
            _ => "Account created",
        }
    )?;
    writeln!(stdout)?;
    render_account_section(stdout, &view.account)?;
    writeln!(stdout)?;
    writeln!(stdout, "Identity")?;
    render_field_rows(
        stdout,
        &[("npub", view.public_identity.public_key_npub.clone())],
    )?;
    if !view.actions.is_empty() {
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_account_use(
    stdout: &mut dyn Write,
    view: &crate::domain::runtime::AccountUseView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "Default account selected")?;
    writeln!(stdout)?;
    render_account_section(stdout, &view.account)
}

fn render_account_clear_default(
    stdout: &mut dyn Write,
    view: &AccountClearDefaultView,
) -> Result<(), RuntimeError> {
    writeln!(
        stdout,
        "{}",
        match view.state.as_str() {
            "cleared" => "Default account cleared",
            _ => "No default account configured",
        }
    )?;
    if let Some(account) = &view.cleared_account {
        writeln!(stdout)?;
        render_account_section(stdout, account)?;
    }
    writeln!(stdout)?;
    render_field_rows(
        stdout,
        &[(
            "Remaining accounts",
            view.remaining_account_count.to_string(),
        )],
    )?;
    if !view.actions.is_empty() {
        writeln!(stdout)?;
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_account_remove(
    stdout: &mut dyn Write,
    view: &AccountRemoveView,
) -> Result<(), RuntimeError> {
    writeln!(
        stdout,
        "{}",
        if view.default_cleared {
            "Default account removed"
        } else {
            "Account removed"
        }
    )?;
    writeln!(stdout)?;
    render_account_section(stdout, &view.removed_account)?;
    writeln!(stdout)?;
    render_field_rows(
        stdout,
        &[(
            "Remaining accounts",
            view.remaining_account_count.to_string(),
        )],
    )?;
    if !view.actions.is_empty() {
        writeln!(stdout)?;
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_account_whoami(
    stdout: &mut dyn Write,
    view: &crate::domain::runtime::AccountWhoamiView,
) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "ready" => {
            writeln!(stdout, "Resolved account")?;
            writeln!(stdout)?;
            if let Some(account) = &view.account_resolution.resolved_account {
                render_account_section(stdout, account)?;
            }
            writeln!(stdout)?;
            render_account_resolution(stdout, &view.account_resolution)?;
            if let Some(identity) = &view.public_identity {
                writeln!(stdout)?;
                writeln!(stdout, "Identity")?;
                render_field_rows(stdout, &[("npub", identity.public_key_npub.clone())])?;
            }
        }
        _ => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            writeln!(stdout)?;
            render_account_resolution(stdout, &view.account_resolution)?;
            writeln!(stdout)?;
            render_item_section(stdout, "Missing", &["Resolved account".to_owned()])?;
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_account_section(
    stdout: &mut dyn Write,
    account: &AccountSummaryView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "Account")?;
    let mut rows = Vec::<(&str, String)>::new();
    push_row(&mut rows, "Name", account.display_name.clone());
    rows.push(("Account", account.id.clone()));
    rows.push(("Signer", humanize_machine_label(account.signer.as_str())));
    rows.push((
        "Default",
        if account.is_default {
            "Yes".to_owned()
        } else {
            "No".to_owned()
        },
    ));
    render_field_rows(stdout, rows.as_slice())
}

fn render_account_resolution(
    stdout: &mut dyn Write,
    resolution: &crate::domain::runtime::AccountResolutionView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "Account resolution")?;
    let mut rows = vec![("Source", humanize_machine_label(resolution.source.as_str()))];
    if let Some(account) = &resolution.resolved_account {
        rows.push(("Resolved account", account.id.clone()));
    }
    if let Some(account) = &resolution.default_account {
        rows.push(("Default account", account.id.clone()));
    }
    render_field_rows(stdout, rows.as_slice())
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
    render_pairs(
        stdout,
        "interaction",
        &[
            ("input enabled", yes_no(view.interaction.input_enabled)),
            ("assume yes", yes_no(view.interaction.assume_yes)),
            ("stdin tty", yes_no(view.interaction.stdin_tty)),
            ("stdout tty", yes_no(view.interaction.stdout_tty)),
            ("prompts allowed", yes_no(view.interaction.prompts_allowed)),
            (
                "confirmations allowed",
                yes_no(view.interaction.confirmations_allowed),
            ),
        ],
    )?;
    let user_config = format!(
        "{} · {}",
        present_absent(view.config_files.user_present),
        view.paths.app_config_path
    );
    let workspace_config = format!(
        "{} · {}",
        present_absent(view.config_files.workspace_present),
        view.paths.workspace_config_path
    );
    let allowed_profiles = view.paths.allowed_profiles.join(", ");
    render_pairs(
        stdout,
        "runtime roots",
        &[
            ("profile", view.paths.profile.as_str()),
            ("allowed profiles", allowed_profiles.as_str()),
            ("app namespace", view.paths.app_namespace.as_str()),
            (
                "shared accounts namespace",
                view.paths.shared_accounts_namespace.as_str(),
            ),
            (
                "shared identities namespace",
                view.paths.shared_identities_namespace.as_str(),
            ),
            ("app config", user_config.as_str()),
            ("workspace config", workspace_config.as_str()),
            ("app data root", view.paths.app_data_root.as_str()),
            ("app logs root", view.paths.app_logs_root.as_str()),
            (
                "shared accounts data",
                view.paths.shared_accounts_data_root.as_str(),
            ),
            (
                "shared accounts secrets",
                view.paths.shared_accounts_secrets_root.as_str(),
            ),
            (
                "default identity path",
                view.paths.default_identity_path.as_str(),
            ),
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
            "contract default secret backend",
            view.account
                .secret_backend
                .contract_default_backend
                .as_str(),
        ),
        (
            "configured secret backend",
            view.account.secret_backend.configured_primary.as_str(),
        ),
        ("identity path", view.account.identity_path.as_str()),
    ];
    if let Some(fallback) = &view.account.secret_backend.contract_default_fallback {
        account_rows.push(("contract default fallback", fallback.as_str()));
    }
    if let Some(fallback) = &view.account.secret_backend.configured_fallback {
        account_rows.push(("configured secret fallback", fallback.as_str()));
    }
    let allowed_backends = view.account.secret_backend.allowed_backends.join(", ");
    account_rows.push(("allowed secret backends", allowed_backends.as_str()));
    if let Some(policy) = &view.account.secret_backend.host_vault_policy {
        account_rows.push(("host vault policy", policy.as_str()));
    }
    account_rows.push((
        "uses protected store",
        yes_no(view.account.secret_backend.uses_protected_store),
    ));
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
    let write_plane_target = format_runtime_target(
        view.write_plane.target_kind.as_deref(),
        view.write_plane.target.as_deref(),
    );
    render_pairs(
        stdout,
        "write plane",
        &[
            ("provider", view.write_plane.provider_runtime_id.as_str()),
            ("binding model", view.write_plane.binding_model.as_str()),
            ("state", view.write_plane.state.as_str()),
            ("provenance", view.write_plane.provenance.as_str()),
            ("source", view.write_plane.source.as_str()),
            ("target", write_plane_target.as_str()),
            ("detail", view.write_plane.detail.as_str()),
            (
                "bridge auth configured",
                yes_no(view.write_plane.bridge_auth_configured),
            ),
        ],
    )?;
    let workflow_target = format_runtime_target(
        view.workflow.target_kind.as_deref(),
        view.workflow.target.as_deref(),
    );
    render_pairs(
        stdout,
        "workflow",
        &[
            ("provider", view.workflow.provider_runtime_id.as_str()),
            ("binding model", view.workflow.binding_model.as_str()),
            ("state", view.workflow.state.as_str()),
            ("provenance", view.workflow.provenance.as_str()),
            ("source", view.workflow.source.as_str()),
            ("target", workflow_target.as_str()),
            ("hyf helper", view.workflow.hyf_helper_state.as_str()),
            (
                "hyf helper detail",
                view.workflow.hyf_helper_detail.as_str(),
            ),
        ],
    )?;
    render_pairs(
        stdout,
        "hyf",
        &[
            ("enabled", yes_no(view.hyf.enabled)),
            ("executable", view.hyf.executable.as_str()),
        ],
    )?;
    let hyf_provider_target = format_runtime_target(
        view.hyf_provider.target_kind.as_deref(),
        view.hyf_provider.target.as_deref(),
    );
    let mut hyf_provider_rows = vec![
        ("provider", view.hyf_provider.provider_runtime_id.as_str()),
        ("binding model", view.hyf_provider.binding_model.as_str()),
        ("state", view.hyf_provider.state.as_str()),
        ("provenance", view.hyf_provider.provenance.as_str()),
        ("source", view.hyf_provider.source.as_str()),
        ("target", hyf_provider_target.as_str()),
        ("executable", view.hyf_provider.executable.as_str()),
    ];
    if let Some(reason) = &view.hyf_provider.reason {
        hyf_provider_rows.push(("reason", reason.as_str()));
    }
    render_pairs(stdout, "hyf provider", hyf_provider_rows.as_slice())?;
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
    writeln!(stdout)?;
    writeln!(stdout, "capability bindings")?;
    let table = Table {
        headers: &["capability", "provider", "state", "target"],
        rows: view
            .capability_bindings
            .iter()
            .map(|binding| {
                vec![
                    binding.capability_id.clone(),
                    binding.provider_runtime_id.clone(),
                    binding.state.clone(),
                    format_capability_binding_target(binding),
                ]
            })
            .collect(),
    };
    render_table(stdout, &table)?;
    writeln!(stdout)?;
    writeln!(stdout, "resolved providers")?;
    let resolved_table = Table {
        headers: &["capability", "provider", "state", "provenance", "target"],
        rows: view
            .resolved_providers
            .iter()
            .map(|provider| {
                vec![
                    provider.capability_id.clone(),
                    provider.provider_runtime_id.clone(),
                    provider.state.clone(),
                    provider.provenance.clone(),
                    format_runtime_target(
                        provider.target_kind.as_deref(),
                        provider.target.as_deref(),
                    ),
                ]
            })
            .collect(),
    };
    render_table(stdout, &resolved_table)?;
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn render_runtime_action(
    stdout: &mut dyn Write,
    view: &RuntimeActionView,
) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        format!(
            "runtime · {} · {}",
            view.runtime_id,
            view.action.replace('_', " ")
        )
        .as_str(),
    )?;
    render_pairs(
        stdout,
        "runtime",
        &[
            ("runtime", view.runtime_id.as_str()),
            ("instance", view.instance_id.as_str()),
            ("instance source", view.instance_source.as_str()),
            ("group", view.runtime_group.as_str()),
            ("state", view.state.as_str()),
            ("mutates bindings", yes_no(view.mutates_bindings)),
        ],
    )?;
    writeln!(stdout, "detail: {}", view.detail)?;
    if let Some(next_step) = &view.next_step {
        writeln!(stdout, "next step: {next_step}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn render_runtime_config_show(
    stdout: &mut dyn Write,
    view: &RuntimeManagedConfigView,
) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        format!("runtime · {} · config", view.runtime_id).as_str(),
    )?;
    let mut rows = vec![
        ("runtime", view.runtime_id.as_str()),
        ("instance", view.instance_id.as_str()),
        ("instance source", view.instance_source.as_str()),
        ("group", view.runtime_group.as_str()),
        ("state", view.state.as_str()),
        ("config present", yes_no(view.config_present)),
    ];
    if let Some(config_format) = &view.config_format {
        rows.push(("config format", config_format.as_str()));
    }
    if let Some(config_path) = &view.config_path {
        rows.push(("config path", config_path.as_str()));
    }
    if let Some(requires_bootstrap_secret) = view.requires_bootstrap_secret {
        rows.push((
            "requires bootstrap secret",
            yes_no(requires_bootstrap_secret),
        ));
    }
    if let Some(requires_config_bootstrap) = view.requires_config_bootstrap {
        rows.push((
            "requires config bootstrap",
            yes_no(requires_config_bootstrap),
        ));
    }
    if let Some(requires_signer_provider) = view.requires_signer_provider {
        rows.push(("requires signer provider", yes_no(requires_signer_provider)));
    }
    render_pairs(stdout, "runtime config", rows.as_slice())?;
    writeln!(stdout, "detail: {}", view.detail)?;
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn render_runtime_logs(stdout: &mut dyn Write, view: &RuntimeLogsView) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        format!("runtime · {} · logs", view.runtime_id).as_str(),
    )?;
    let mut rows = vec![
        ("runtime", view.runtime_id.as_str()),
        ("instance", view.instance_id.as_str()),
        ("instance source", view.instance_source.as_str()),
        ("group", view.runtime_group.as_str()),
        ("state", view.state.as_str()),
        ("stdout present", yes_no(view.stdout_log_present)),
        ("stderr present", yes_no(view.stderr_log_present)),
    ];
    if let Some(stdout_log_path) = &view.stdout_log_path {
        rows.push(("stdout log", stdout_log_path.as_str()));
    }
    if let Some(stderr_log_path) = &view.stderr_log_path {
        rows.push(("stderr log", stderr_log_path.as_str()));
    }
    render_pairs(stdout, "runtime logs", rows.as_slice())?;
    writeln!(stdout, "detail: {}", view.detail)?;
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn render_runtime_status(
    stdout: &mut dyn Write,
    view: &RuntimeStatusView,
) -> Result<(), RuntimeError> {
    write_context(
        stdout,
        format!("runtime · {} · status", view.runtime_id).as_str(),
    )?;
    let mut rows = vec![
        ("runtime", view.runtime_id.as_str()),
        ("instance", view.instance_id.as_str()),
        ("instance source", view.instance_source.as_str()),
        ("group", view.runtime_group.as_str()),
        ("posture", view.management_posture.as_str()),
        ("state", view.state.as_str()),
        ("install state", view.install_state.as_str()),
        ("health state", view.health_state.as_str()),
        ("health source", view.health_source.as_str()),
        ("registry", view.registry_path.as_str()),
    ];
    if let Some(mode) = &view.management_mode {
        rows.push(("management mode", mode.as_str()));
    }
    if let Some(service_manager_integration) = view.service_manager_integration {
        rows.push((
            "service manager integration",
            yes_no(service_manager_integration),
        ));
    }
    if let Some(uses_absolute_binary_paths) = view.uses_absolute_binary_paths {
        rows.push((
            "uses absolute binary paths",
            yes_no(uses_absolute_binary_paths),
        ));
    }
    if let Some(preferred_cli_binding) = view.preferred_cli_binding {
        rows.push(("preferred cli binding", yes_no(preferred_cli_binding)));
    }
    render_pairs(stdout, "runtime status", rows.as_slice())?;
    writeln!(stdout, "detail: {}", view.detail)?;
    if let Some(instance_paths) = &view.instance_paths {
        render_pairs(
            stdout,
            "instance paths",
            &[
                ("install dir", instance_paths.install_dir.as_str()),
                ("state dir", instance_paths.state_dir.as_str()),
                ("logs dir", instance_paths.logs_dir.as_str()),
                ("run dir", instance_paths.run_dir.as_str()),
                ("secrets dir", instance_paths.secrets_dir.as_str()),
                ("pid file", instance_paths.pid_file_path.as_str()),
                ("stdout log", instance_paths.stdout_log_path.as_str()),
                ("stderr log", instance_paths.stderr_log_path.as_str()),
                ("metadata", instance_paths.metadata_path.as_str()),
            ],
        )?;
    }
    if let Some(record) = &view.instance_record {
        let mut record_rows = vec![
            ("management mode", record.management_mode.as_str()),
            ("install state", record.install_state.as_str()),
            ("binary path", record.binary_path.as_str()),
            ("config path", record.config_path.as_str()),
            ("logs path", record.logs_path.as_str()),
            ("run path", record.run_path.as_str()),
            ("installed version", record.installed_version.as_str()),
        ];
        if let Some(health_endpoint) = &record.health_endpoint {
            record_rows.push(("health endpoint", health_endpoint.as_str()));
        }
        if let Some(secret_material_ref) = &record.secret_material_ref {
            record_rows.push(("secret material ref", secret_material_ref.as_str()));
        }
        if let Some(last_started_at) = &record.last_started_at {
            record_rows.push(("last started at", last_started_at.as_str()));
        }
        if let Some(last_stopped_at) = &record.last_stopped_at {
            record_rows.push(("last stopped at", last_stopped_at.as_str()));
        }
        if let Some(notes) = &record.notes {
            record_rows.push(("notes", notes.as_str()));
        }
        render_pairs(stdout, "instance record", record_rows.as_slice())?;
    }
    if !view.lifecycle_actions.is_empty() {
        writeln!(
            stdout,
            "lifecycle actions: {}",
            view.lifecycle_actions.join(", ")
        )?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    Ok(())
}

fn format_capability_binding_target(
    binding: &crate::domain::runtime::CapabilityBindingRuntimeView,
) -> String {
    let mut rendered =
        format_runtime_target(binding.target_kind.as_deref(), binding.target.as_deref());
    if rendered.is_empty() {
        return rendered;
    }
    if let Some(account_ref) = &binding.managed_account_ref {
        rendered.push_str(format!(" · account {account_ref}").as_str());
    }
    if let Some(session_ref) = &binding.signer_session_ref {
        rendered.push_str(format!(" · session {session_ref}").as_str());
    }
    rendered
}

fn format_runtime_target(target_kind: Option<&str>, target: Option<&str>) -> String {
    let Some(target) = target else {
        return String::new();
    };

    match target_kind {
        Some(kind) => format!("{kind} {target}"),
        None => target.to_owned(),
    }
}

fn render_doctor(stdout: &mut dyn Write, view: &DoctorView) -> Result<(), RuntimeError> {
    writeln!(stdout, "Readiness check")?;
    let ready = view
        .checks
        .iter()
        .filter(|check| matches!(check.status.as_str(), "ok" | "ready" | "healthy"))
        .map(doctor_item)
        .collect::<Vec<_>>();
    let needs_attention = view
        .checks
        .iter()
        .filter(|check| !matches!(check.status.as_str(), "ok" | "ready" | "healthy"))
        .map(doctor_item)
        .collect::<Vec<_>>();

    if !ready.is_empty() || !needs_attention.is_empty() || !view.actions.is_empty() {
        writeln!(stdout)?;
    }
    let mut wrote_section = false;
    if !ready.is_empty() {
        render_item_section(stdout, "Ready", &ready)?;
        wrote_section = true;
    }
    if !needs_attention.is_empty() {
        if wrote_section {
            writeln!(stdout)?;
        }
        render_item_section(stdout, "Needs attention", &needs_attention)?;
        wrote_section = true;
    }
    if !view.actions.is_empty() {
        if wrote_section {
            writeln!(stdout)?;
        }
        render_item_section(stdout, "Next", &view.actions)?;
    }
    writeln!(stdout)?;
    render_account_resolution(stdout, &view.account_resolution)?;
    Ok(())
}

fn render_find(stdout: &mut dyn Write, view: &FindView) -> Result<(), RuntimeError> {
    render_market_search(stdout, view)
}

fn render_market_search(stdout: &mut dyn Write, view: &FindView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            writeln!(stdout)?;
            render_item_section(stdout, "Missing", &["Local market data".to_owned()])?;
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "empty" => {
            writeln!(stdout, "No listings found")?;
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(stdout, "{}", market_search_headline(view))?;
            writeln!(stdout)?;
            for (index, result) in view.results.iter().enumerate() {
                render_market_search_card(stdout, result)?;
                if index + 1 < view.results.len() {
                    writeln!(stdout)?;
                }
            }
            if let Some(hyf) = &view.hyf {
                if hyf.rewritten_query.trim() != view.query.trim() {
                    writeln!(stdout)?;
                    render_item_section(stdout, "Also searched for", &[view.query.clone()])?;
                }
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_market_search_card(
    stdout: &mut dyn Write,
    result: &crate::domain::runtime::FindResultView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{}", result.title)?;
    let mut rows = vec![("Key", result.product_key.clone())];
    push_row(
        &mut rows,
        "Listing address",
        result
            .listing_addr
            .as_deref()
            .and_then(non_empty_str)
            .map(str::to_owned),
    );
    push_row(
        &mut rows,
        "Place",
        result
            .location_primary
            .as_deref()
            .and_then(non_empty_str)
            .map(str::to_owned),
    );
    push_row(&mut rows, "Offer", quantity_offer_text(&result.available));
    rows.push((
        "Price",
        format_price(
            result.price.amount,
            &result.price.currency,
            result.price.per_amount,
            &result.price.per_unit,
        ),
    ));
    rows.push((
        "Stock",
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
    ));
    render_field_rows(stdout, rows.as_slice())
}

fn market_search_headline(view: &FindView) -> String {
    let query = view
        .hyf
        .as_ref()
        .map(|hyf| hyf.rewritten_query.as_str())
        .unwrap_or(view.query.as_str());
    format!(
        "{} listing{} for {}",
        view.count,
        if view.count == 1 { "" } else { "s" },
        query
    )
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
            headers: &["job", "type", "state", "signer", "session", "updated"],
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
                        job.signer_session_id.clone().unwrap_or_default(),
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
                ("signer mode", job.signer.clone()),
                (
                    "signer session",
                    job.signer_session_id
                        .clone()
                        .unwrap_or_else(|| "-".to_owned()),
                ),
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
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unavailable" => {
            writeln!(stdout, "Unavailable right now")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "error" => {
            writeln!(stdout, "Could not complete the command")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(stdout, "Watching job {}", view.job_id)?;
            if view.frames.is_empty() {
                if let Some(reason) = &view.reason {
                    writeln!(stdout)?;
                    writeln!(stdout, "{reason}")?;
                }
            } else {
                for frame in &view.frames {
                    writeln!(stdout)?;
                    writeln!(
                        stdout,
                        "{}",
                        crate::runtime::job::format_clock(frame.observed_at_unix)
                    )?;
                    let mut rows = vec![
                        ("State", humanize_machine_label(frame.state.as_str())),
                        ("Summary", frame.summary.clone()),
                        ("Signer", humanize_machine_label(frame.signer.as_str())),
                    ];
                    push_row(&mut rows, "Session", frame.signer_session_id.clone());
                    if frame.terminal {
                        rows.push(("Terminal", "Yes".to_owned()));
                    }
                    render_field_rows(stdout, rows.as_slice())?;
                }
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
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
    if let Some(workflow) = &view.workflow {
        render_order_workflow(stdout, workflow)?;
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
    match view.state.as_str() {
        "dry_run" => {
            writeln!(stdout, "Dry run only")?;
            writeln!(stdout)?;
            writeln!(stdout, "Order would be submitted.")?;
            writeln!(stdout)?;
            render_order_submit_section(stdout, view)?;
            writeln!(stdout, "Nothing was written.")?;
        }
        "missing" => {
            writeln!(stdout, "Not found")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.issues.is_empty() {
                writeln!(stdout)?;
                writeln!(stdout, "Needs attention")?;
                let rows = view
                    .issues
                    .iter()
                    .map(|issue| (issue.field.as_str(), issue.message.clone()))
                    .collect::<Vec<_>>();
                render_field_rows(stdout, rows.as_slice())?;
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unavailable" => {
            writeln!(stdout, "Unavailable right now")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "error" => {
            writeln!(stdout, "Could not complete the command")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(
                stdout,
                "{}",
                match view.state.as_str() {
                    "already_submitted" => "Order already submitted",
                    "deduplicated" => "Order already in progress",
                    _ => "Order submitted",
                }
            )?;
            writeln!(stdout)?;
            render_order_submit_section(stdout, view)?;
            if let Some(job) = &view.job {
                writeln!(stdout)?;
                writeln!(stdout, "Job")?;
                let mut rows = vec![
                    ("Job", job.job_id.clone()),
                    ("State", humanize_machine_label(job.state.as_str())),
                ];
                push_row(&mut rows, "Event", job.event_id.clone());
                render_field_rows(stdout, rows.as_slice())?;
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_order_submit_section(
    stdout: &mut dyn Write,
    view: &OrderSubmitView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "Order")?;
    let mut rows = vec![("ID", view.order_id.clone())];
    push_row(
        &mut rows,
        "Listing",
        first_present([view.listing_lookup.as_deref(), view.listing_addr.as_deref()]),
    );
    push_row(
        &mut rows,
        "Buyer",
        first_present([
            view.buyer_account_id.as_deref(),
            view.buyer_pubkey.as_deref(),
        ]),
    );
    if !matches!(view.state.as_str(), "dry_run" | "missing" | "unconfigured") {
        rows.push(("State", humanize_machine_label(view.state.as_str())));
    }
    render_field_rows(stdout, rows.as_slice())
}

fn render_order_submit_watch(
    stdout: &mut dyn Write,
    view: &OrderSubmitWatchView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{}", order_submit_watch_headline(&view.submit))?;
    writeln!(stdout)?;

    writeln!(stdout, "Order")?;
    let mut order_rows = vec![
        ("ID", view.submit.order_id.clone()),
        ("State", humanize_machine_label(view.submit.state.as_str())),
    ];
    push_row(
        &mut order_rows,
        "Listing",
        first_present([
            view.submit.listing_lookup.as_deref(),
            view.submit.listing_addr.as_deref(),
        ]),
    );
    push_row(
        &mut order_rows,
        "Buyer",
        first_present([
            view.submit.buyer_account_id.as_deref(),
            view.submit.buyer_pubkey.as_deref(),
        ]),
    );
    render_field_rows(stdout, order_rows.as_slice())?;

    if let Some(job) = &view.submit.job {
        writeln!(stdout, "Job")?;
        let mut job_rows = vec![
            ("Job", job.job_id.clone()),
            ("State", humanize_machine_label(job.state.as_str())),
        ];
        push_row(&mut job_rows, "Event", job.event_id.clone());
        render_field_rows(stdout, job_rows.as_slice())?;
    }

    writeln!(stdout, "Watching order {}", view.watch.order_id)?;

    if view.watch.frames.is_empty() {
        if let Some(reason) = &view.watch.reason {
            writeln!(stdout)?;
            writeln!(stdout, "{reason}")?;
        }
    } else {
        for frame in &view.watch.frames {
            writeln!(stdout)?;
            writeln!(
                stdout,
                "{}",
                crate::runtime::job::format_clock(frame.observed_at_unix)
            )?;
            let rows = vec![
                ("State", humanize_machine_label(frame.state.as_str())),
                ("Summary", frame.summary.clone()),
            ];
            render_field_rows(stdout, rows.as_slice())?;
        }
    }

    if !view.watch.actions.is_empty() {
        render_item_section(stdout, "Next", &view.watch.actions)?;
    }
    Ok(())
}

fn render_order_watch(stdout: &mut dyn Write, view: &OrderWatchView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "missing" => {
            writeln!(stdout, "Not found")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "not_submitted" | "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unavailable" => {
            writeln!(stdout, "Unavailable right now")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "error" => {
            writeln!(stdout, "Could not complete the command")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(stdout, "Watching order {}", view.order_id)?;
            if view.frames.is_empty() {
                if let Some(reason) = &view.reason {
                    writeln!(stdout)?;
                    writeln!(stdout, "{reason}")?;
                }
            } else {
                for frame in &view.frames {
                    writeln!(stdout)?;
                    writeln!(
                        stdout,
                        "{}",
                        crate::runtime::job::format_clock(frame.observed_at_unix)
                    )?;
                    let mut rows = vec![
                        ("State", humanize_machine_label(frame.state.as_str())),
                        ("Summary", frame.summary.clone()),
                    ];
                    push_row(
                        &mut rows,
                        "Signer",
                        Some(humanize_machine_label(frame.signer_mode.as_str())),
                    );
                    push_row(&mut rows, "Session", frame.signer_session_id.clone());
                    if frame.terminal {
                        rows.push(("Terminal", "Yes".to_owned()));
                    }
                    render_field_rows(stdout, rows.as_slice())?;
                }
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
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
    if let Some(signer_mode) = &job.signer_mode {
        rows.push(("signer mode", signer_mode.as_str()));
    }
    if let Some(signer_session_id) = &job.signer_session_id {
        rows.push(("signer session", signer_session_id.as_str()));
    }
    if let Some(requested_signer_session_id) = &job.requested_signer_session_id {
        rows.push((
            "requested signer session",
            requested_signer_session_id.as_str(),
        ));
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

fn render_order_workflow(
    stdout: &mut dyn Write,
    workflow: &OrderWorkflowView,
) -> Result<(), RuntimeError> {
    let mut rows = vec![
        ("state", workflow.state.as_str()),
        ("order id", workflow.order_id.as_str()),
    ];
    if let Some(listing_addr) = &workflow.listing_addr {
        rows.push(("listing addr", listing_addr.as_str()));
    }
    if let Some(validated_listing_event_id) = &workflow.validated_listing_event_id {
        rows.push((
            "validated listing event",
            validated_listing_event_id.as_str(),
        ));
    }
    if let Some(root_event_id) = &workflow.root_event_id {
        rows.push(("root event id", root_event_id.as_str()));
    }
    if let Some(last_event_id) = &workflow.last_event_id {
        rows.push(("last event id", last_event_id.as_str()));
    }
    render_pairs(stdout, "workflow", rows.as_slice())?;
    if let Some(reason) = &workflow.reason {
        writeln!(stdout, "workflow reason: {reason}")?;
    }
    writeln!(stdout, "workflow source: {}", workflow.source)?;
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
    if let Some(delivery_method) = &view.delivery_method {
        rows.push(("delivery", delivery_method.as_str()));
    }
    if let Some(location_primary) = &view.location_primary {
        rows.push(("location", location_primary.as_str()));
    }
    render_pairs(stdout, "draft", rows.as_slice())?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
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
    render_market_view(stdout, view)
}

fn render_market_view(stdout: &mut dyn Write, view: &ListingGetView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "missing" => {
            writeln!(stdout, "Not found")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            let headline = view.title.as_deref().unwrap_or("Listing");
            writeln!(stdout, "{headline}")?;
            writeln!(stdout)?;
            let mut rows = Vec::<(&str, String)>::new();
            push_row(
                &mut rows,
                "Key",
                view.product_key
                    .as_deref()
                    .and_then(non_empty_str)
                    .map(str::to_owned),
            );
            push_row(
                &mut rows,
                "Listing address",
                view.listing_addr
                    .as_deref()
                    .and_then(non_empty_str)
                    .map(str::to_owned),
            );
            push_row(
                &mut rows,
                "Category",
                view.category
                    .as_deref()
                    .and_then(non_empty_str)
                    .map(str::to_owned),
            );
            push_row(
                &mut rows,
                "Place",
                view.location_primary
                    .as_deref()
                    .and_then(non_empty_str)
                    .map(str::to_owned),
            );
            if let Some(available) = &view.available {
                push_row(&mut rows, "Offer", quantity_offer_text(available));
                rows.push((
                    "Stock",
                    format_available(
                        available.available_amount.unwrap_or(available.total_amount),
                        available
                            .label
                            .as_deref()
                            .unwrap_or(available.total_unit.as_str()),
                    ),
                ));
            }
            if let Some(price) = &view.price {
                rows.push((
                    "Price",
                    format_price(
                        price.amount,
                        &price.currency,
                        price.per_amount,
                        &price.per_unit,
                    ),
                ));
            }
            render_owned_pairs(stdout, "Listing", rows.as_slice())?;
            let mut wrote_about = false;
            if let Some(description) = &view.description {
                render_item_section(stdout, "About", &[description.clone()])?;
                wrote_about = true;
            }
            if !view.actions.is_empty() {
                if wrote_about {
                    writeln!(stdout)?;
                }
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_sell_add(stdout: &mut dyn Write, view: &SellAddView) -> Result<(), RuntimeError> {
    writeln!(stdout, "Listing draft saved")?;
    writeln!(stdout)?;
    writeln!(stdout, "The draft is local until you publish it.")?;
    writeln!(stdout)?;

    let mut draft_rows = vec![("File", view.file.clone())];
    push_row(&mut draft_rows, "Listing", view.product_key.clone());
    push_row(&mut draft_rows, "Title", view.title.clone());
    push_row(&mut draft_rows, "Offer", view.offer.clone());
    push_row(&mut draft_rows, "Price", view.price.clone());
    push_row(&mut draft_rows, "Stock", view.stock.clone());
    render_owned_pairs(stdout, "Draft", draft_rows.as_slice())?;

    let mut default_rows = Vec::<(&str, String)>::new();
    push_row(&mut default_rows, "Farm", view.farm_name.clone());
    push_row(
        &mut default_rows,
        "Delivery",
        view.delivery_method
            .as_deref()
            .map(humanize_delivery_method),
    );
    push_row(&mut default_rows, "Place", view.location_primary.clone());
    if !default_rows.is_empty() {
        render_owned_pairs(stdout, "Defaults", default_rows.as_slice())?;
    }

    if let Some(reason) = &view.reason {
        writeln!(stdout, "{reason}")?;
        writeln!(stdout)?;
    }
    if !view.actions.is_empty() {
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_sell_show(stdout: &mut dyn Write, view: &SellShowView) -> Result<(), RuntimeError> {
    writeln!(stdout, "Listing draft")?;
    writeln!(stdout)?;

    let mut draft_rows = vec![("File", view.file.clone())];
    push_row(&mut draft_rows, "Listing", view.product_key.clone());
    push_row(&mut draft_rows, "Title", view.title.clone());
    push_row(&mut draft_rows, "Category", view.category.clone());
    push_row(&mut draft_rows, "Offer", view.offer.clone());
    push_row(&mut draft_rows, "Price", view.price.clone());
    push_row(&mut draft_rows, "Stock", view.stock.clone());
    push_row(
        &mut draft_rows,
        "Delivery",
        view.delivery_method
            .as_deref()
            .map(humanize_delivery_method),
    );
    push_row(&mut draft_rows, "Place", view.location_primary.clone());
    render_owned_pairs(stdout, "Draft", draft_rows.as_slice())?;

    if let Some(reason) = &view.reason {
        writeln!(stdout, "{reason}")?;
        writeln!(stdout)?;
    }
    if !view.actions.is_empty() {
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_sell_check(stdout: &mut dyn Write, view: &SellCheckView) -> Result<(), RuntimeError> {
    if view.valid {
        writeln!(stdout, "Draft looks ready")?;
        writeln!(stdout)?;
        let mut draft_rows = vec![("File", view.file.clone())];
        push_row(&mut draft_rows, "Listing", view.product_key.clone());
        push_row(&mut draft_rows, "Seller", view.seller_pubkey.clone());
        push_row(&mut draft_rows, "Farm", view.farm_ref.clone());
        render_owned_pairs(stdout, "Draft", draft_rows.as_slice())?;
    } else {
        writeln!(stdout, "Draft needs changes")?;
        writeln!(stdout)?;
        let rows = view
            .issues
            .iter()
            .map(|issue| (issue.field.as_str(), issue.message.clone()))
            .collect::<Vec<_>>();
        render_field_rows(stdout, rows.as_slice())?;
    }

    if !view.actions.is_empty() {
        render_item_section(stdout, "Next", &view.actions)?;
    }
    Ok(())
}

fn render_sell_mutation(
    stdout: &mut dyn Write,
    view: &SellMutationView,
) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "dry_run" => {
            writeln!(stdout, "Dry run only")?;
            writeln!(stdout)?;
            writeln!(
                stdout,
                "Listing would be {}.",
                match view.operation.as_str() {
                    "publish" => "published",
                    "update" => "updated",
                    "pause" => "paused",
                    _ => "changed",
                }
            )?;
            writeln!(stdout)?;
            let mut rows = vec![("File", view.file.clone())];
            push_row(&mut rows, "Listing", view.product_key.clone());
            rows.push(("Address", view.listing_addr.clone()));
            if view.operation == "publish" {
                push_row(
                    &mut rows,
                    "Publish mode",
                    view.publish_mode.as_deref().map(|mode| {
                        if mode == "runtime_bridge" {
                            "Runtime bridge".to_owned()
                        } else {
                            mode.to_owned()
                        }
                    }),
                );
            }
            render_owned_pairs(stdout, "Listing", rows.as_slice())?;
            writeln!(stdout, "Nothing was written.")?;
        }
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unavailable" => {
            writeln!(stdout, "Unavailable right now")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "error" => {
            writeln!(stdout, "Something went wrong")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(
                stdout,
                "{}",
                match view.operation.as_str() {
                    "publish" => "Listing published",
                    "update" => "Listing updated",
                    "pause" => "Listing paused",
                    _ => "Listing updated",
                }
            )?;
            writeln!(stdout)?;
            let mut listing_rows = vec![("File", view.file.clone())];
            push_row(&mut listing_rows, "Listing", view.product_key.clone());
            listing_rows.push(("Address", view.listing_addr.clone()));
            if view.operation == "publish" {
                push_row(
                    &mut listing_rows,
                    "Publish mode",
                    view.publish_mode.as_deref().map(|mode| {
                        if mode == "runtime_bridge" {
                            "Runtime bridge".to_owned()
                        } else {
                            mode.to_owned()
                        }
                    }),
                );
            }
            render_owned_pairs(stdout, "Listing", listing_rows.as_slice())?;

            let mut job_rows = Vec::<(&str, String)>::new();
            push_row(&mut job_rows, "State", view.job_status.clone());
            push_row(&mut job_rows, "Job", view.job_id.clone());
            push_row(&mut job_rows, "Event", view.event_id.clone());
            if !job_rows.is_empty() {
                render_owned_pairs(stdout, "Job", job_rows.as_slice())?;
            }

            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_sell_draft_mutation(
    stdout: &mut dyn Write,
    view: &SellDraftMutationView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "Draft updated")?;
    writeln!(stdout)?;
    render_owned_pairs(
        stdout,
        "Changed",
        &[(view.changed_label.as_str(), view.changed_value.clone())],
    )?;
    let mut draft_rows = vec![("File", view.file.clone())];
    push_row(&mut draft_rows, "Listing", view.product_key.clone());
    render_owned_pairs(stdout, "Draft", draft_rows.as_slice())?;
    if !view.actions.is_empty() {
        render_item_section(stdout, "Next", &view.actions)?;
    }
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
        rows.push(("signer mode", signer_mode.as_str()));
    }
    if let Some(signer_session_id) = &view.signer_session_id {
        rows.push(("signer session", signer_session_id.as_str()));
    }
    if let Some(requested_signer_session_id) = &view.requested_signer_session_id {
        rows.push((
            "requested signer session",
            requested_signer_session_id.as_str(),
        ));
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
    if view.relays.is_empty() {
        writeln!(stdout, "Not ready yet")?;
        if let Some(reason) = &view.reason {
            writeln!(stdout)?;
            writeln!(stdout, "{reason}")?;
        }
        writeln!(stdout)?;
        render_item_section(stdout, "Missing", &["Relay configuration".to_owned()])?;
        if !view.actions.is_empty() {
            writeln!(stdout)?;
            render_item_section(stdout, "Next", &view.actions)?;
        }
        return Ok(());
    }

    writeln!(
        stdout,
        "{} relay{}",
        view.count,
        if view.count == 1 { "" } else { "s" }
    )?;
    writeln!(stdout)?;
    for (index, relay) in view.relays.iter().enumerate() {
        writeln!(stdout, "{}", relay.url)?;
        let rows = vec![
            (
                "Read",
                if relay.read {
                    "Yes".to_owned()
                } else {
                    "No".to_owned()
                },
            ),
            (
                "Write",
                if relay.write {
                    "Yes".to_owned()
                } else {
                    "No".to_owned()
                },
            ),
        ];
        render_field_rows(stdout, rows.as_slice())?;
        if index + 1 < view.relays.len() {
            writeln!(stdout)?;
        }
    }
    if !view.actions.is_empty() {
        writeln!(stdout)?;
        render_item_section(stdout, "Next", &view.actions)?;
    }
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
    let rows = vec![
        ("status", view.state.as_str()),
        ("session", view.session.as_str()),
        ("relays configured", relay_count.as_str()),
        ("publish policy", view.publish_policy.as_str()),
        ("signer mode", view.signer_mode.as_str()),
    ];
    render_pairs(stdout, "network", rows.as_slice())?;
    writeln!(stdout)?;
    render_account_resolution(stdout, &view.account_resolution)?;
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

fn render_market_update(stdout: &mut dyn Write, view: &SyncActionView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            let mut missing = Vec::new();
            if view.replica_db == "missing" {
                missing.push("Local market data".to_owned());
            }
            if view.relay_count == 0 {
                missing.push("Relay configuration".to_owned());
            }
            if !missing.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Missing", &missing)?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unavailable" => {
            writeln!(stdout, "Unavailable right now")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(stdout, "Market data updated")?;
            writeln!(stdout)?;
            render_owned_pairs(
                stdout,
                "Local data",
                &[
                    ("State", view.state.clone()),
                    ("Updated", view.freshness.display.clone()),
                    ("Relays", view.relay_count.to_string()),
                ],
            )?;
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_sync_watch(stdout: &mut dyn Write, view: &SyncWatchView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "unavailable" => {
            writeln!(stdout, "Unavailable right now")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        "error" => {
            writeln!(stdout, "Could not complete the command")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
        _ => {
            writeln!(stdout, "Watching market sync")?;
            if view.frames.is_empty() {
                if let Some(reason) = &view.reason {
                    writeln!(stdout)?;
                    writeln!(stdout, "{reason}")?;
                }
            } else {
                for frame in &view.frames {
                    writeln!(stdout)?;
                    writeln!(
                        stdout,
                        "{}",
                        crate::runtime::job::format_clock(frame.observed_at)
                    )?;
                    let rows = vec![
                        ("State", humanize_machine_label(frame.state.as_str())),
                        ("Relays", frame.relay_count.to_string()),
                        ("Updated", frame.freshness.display.clone()),
                        ("Queue", format!("{} pending", frame.queue.pending_count)),
                    ];
                    render_field_rows(stdout, rows.as_slice())?;
                }
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
        }
    }
    Ok(())
}

fn render_farm_setup(stdout: &mut dyn Write, view: &FarmSetupView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Not ready yet")?;
            writeln!(stdout)?;
            render_item_section(stdout, "Missing", &["Resolved account".to_owned()])?;
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
            Ok(())
        }
        _ => {
            writeln!(stdout, "Farm draft saved")?;
            if let Some(reason) = &view.reason {
                writeln!(stdout)?;
                writeln!(stdout, "{reason}")?;
            }
            if let Some(config) = &view.config {
                writeln!(stdout)?;
                render_farm_summary(stdout, config)?;
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
            Ok(())
        }
    }
}

fn render_farm_set(stdout: &mut dyn Write, view: &FarmSetView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "unconfigured" => {
            writeln!(stdout, "Farm draft not found")?;
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
            Ok(())
        }
        _ => {
            writeln!(stdout, "Farm updated")?;
            writeln!(stdout)?;
            render_owned_pairs(
                stdout,
                "Changed",
                &[("Field", view.field.clone()), ("Value", view.value.clone())],
            )?;
            if let Some(config) = &view.config {
                render_farm_summary(stdout, config)?;
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
            Ok(())
        }
    }
}

fn render_farm_status(stdout: &mut dyn Write, view: &FarmStatusView) -> Result<(), RuntimeError> {
    match view.state.as_str() {
        "ready" => {
            writeln!(stdout, "Farm ready to publish")?;
            if let Some(config) = &view.config {
                writeln!(stdout)?;
                render_farm_summary(stdout, config)?;
            }
            if !view.actions.is_empty() {
                render_item_section(stdout, "Next", &view.actions)?;
            }
            Ok(())
        }
        _ => {
            writeln!(stdout, "Farm not ready yet")?;
            if !view.missing.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Missing", &view.missing)?;
            }
            if !view.actions.is_empty() {
                writeln!(stdout)?;
                render_item_section(stdout, "Next", &view.actions)?;
            }
            Ok(())
        }
    }
}

fn render_farm_get(stdout: &mut dyn Write, view: &FarmGetView) -> Result<(), RuntimeError> {
    if let Some(document) = &view.document {
        writeln!(stdout, "Farm draft")?;
        writeln!(stdout)?;
        render_farm_document(stdout, document)?;
    } else {
        writeln!(stdout, "Farm draft not found")?;
        if !view.actions.is_empty() {
            writeln!(stdout)?;
            render_item_section(stdout, "Next", &view.actions)?;
        }
    }
    Ok(())
}

fn render_farm_publish(stdout: &mut dyn Write, view: &FarmPublishView) -> Result<(), RuntimeError> {
    if view.state == "unconfigured" {
        writeln!(stdout, "Not ready yet")?;
        if !view.missing.is_empty() {
            writeln!(stdout)?;
            render_item_section(stdout, "Missing", &view.missing)?;
        }
        if !view.actions.is_empty() {
            writeln!(stdout)?;
            render_item_section(stdout, "Next", &view.actions)?;
        }
        return Ok(());
    }

    write_context(stdout, format!("farm publish · {}", view.state).as_str())?;
    render_owned_pairs(
        stdout,
        "farm",
        &[
            ("scope", view.scope.clone()),
            ("path", view.path.clone()),
            ("account id", view.selected_account_id.clone()),
            ("account pubkey", view.selected_account_pubkey.clone()),
            ("farm d_tag", view.farm_d_tag.clone()),
            ("dry run", yes_no(view.dry_run).to_owned()),
        ],
    )?;
    render_farm_publish_component(stdout, "profile", &view.profile)?;
    render_farm_publish_component(stdout, "farm record", &view.farm)?;
    if let Some(reason) = &view.reason {
        writeln!(stdout, "reason: {reason}")?;
    }
    writeln!(stdout, "source: {}", view.source)?;
    render_actions(stdout, &view.actions)?;
    Ok(())
}

fn render_farm_document(
    stdout: &mut dyn Write,
    document: &crate::domain::runtime::FarmConfigDocumentView,
) -> Result<(), RuntimeError> {
    let mut rows = Vec::new();
    push_row(
        &mut rows,
        "Name",
        first_present([
            Some(document.profile.name.as_str()),
            Some(document.farm.name.as_str()),
        ]),
    );
    push_row(
        &mut rows,
        "Display name",
        document.profile.display_name.as_deref().map(str::to_owned),
    );
    push_row(
        &mut rows,
        "About",
        first_present([
            document.profile.about.as_deref(),
            document.farm.about.as_deref(),
        ]),
    );
    push_row(
        &mut rows,
        "Website",
        first_present([
            document.profile.website.as_deref(),
            document.farm.website.as_deref(),
        ]),
    );
    push_row(
        &mut rows,
        "Place",
        first_present([
            Some(document.listing_defaults.location.primary.as_str()),
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.primary.as_deref()),
        ]),
    );
    push_row(
        &mut rows,
        "City",
        first_present([
            document.listing_defaults.location.city.as_deref(),
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.city.as_deref()),
        ]),
    );
    push_row(
        &mut rows,
        "Region",
        first_present([
            document.listing_defaults.location.region.as_deref(),
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.region.as_deref()),
        ]),
    );
    push_row(
        &mut rows,
        "Country",
        first_present([
            document.listing_defaults.location.country.as_deref(),
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.country.as_deref()),
        ]),
    );
    push_row(
        &mut rows,
        "Delivery",
        non_empty_str(document.listing_defaults.delivery_method.as_str())
            .map(humanize_delivery_method),
    );
    rows.push(("Scope", document.selection.scope.clone()));
    rows.push(("Farm tag", document.selection.farm_d_tag.clone()));
    render_owned_pairs(stdout, "Farm", rows.as_slice())
}

fn render_farm_publish_component(
    stdout: &mut dyn Write,
    label: &str,
    component: &FarmPublishComponentView,
) -> Result<(), RuntimeError> {
    let mut rows = vec![
        ("state", component.state.clone()),
        ("rpc method", component.rpc_method.clone()),
        ("event kind", component.event_kind.to_string()),
    ];
    if let Some(job_id) = &component.job_id {
        rows.push(("job id", job_id.clone()));
    }
    if let Some(job_status) = &component.job_status {
        rows.push(("job status", job_status.clone()));
    }
    if let Some(event_id) = &component.event_id {
        rows.push(("event id", event_id.clone()));
    }
    if let Some(event_addr) = &component.event_addr {
        rows.push(("event addr", event_addr.clone()));
    }
    if let Some(reason) = &component.reason {
        rows.push(("reason", reason.clone()));
    }
    render_owned_pairs(stdout, label, rows.as_slice())
}

fn render_farm_summary(
    stdout: &mut dyn Write,
    config: &FarmConfigSummaryView,
) -> Result<(), RuntimeError> {
    let mut rows = Vec::new();
    push_row(
        &mut rows,
        "Name",
        non_empty_str(config.name.as_str()).map(str::to_owned),
    );
    rows.push(("Scope", config.scope.clone()));
    push_row(
        &mut rows,
        "Place",
        config
            .location_primary
            .as_deref()
            .and_then(non_empty_str)
            .map(str::to_owned),
    );
    push_row(
        &mut rows,
        "Delivery",
        non_empty_str(config.delivery_method.as_str()).map(humanize_delivery_method),
    );
    render_owned_pairs(stdout, "Farm", rows.as_slice())
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

fn render_setup(stdout: &mut dyn Write, view: &SetupView) -> Result<(), RuntimeError> {
    render_checklist_summary(
        stdout,
        match view.state.as_str() {
            "unconfigured" => "Not ready yet",
            _ => "Setup saved",
        },
        &view.ready,
        &view.needs_attention,
        &view.next,
    )?;
    writeln!(stdout)?;
    render_account_resolution(stdout, &view.account_resolution)
}

fn render_status_summary(stdout: &mut dyn Write, view: &StatusView) -> Result<(), RuntimeError> {
    render_checklist_summary(
        stdout,
        match view.state.as_str() {
            "unconfigured" => "Not ready yet",
            _ => "Status",
        },
        &view.ready,
        &view.needs_attention,
        &view.next,
    )?;
    writeln!(stdout)?;
    render_account_resolution(stdout, &view.account_resolution)
}

fn render_checklist_summary(
    stdout: &mut dyn Write,
    headline: &str,
    ready: &[String],
    needs_attention: &[String],
    next: &[String],
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{headline}")?;

    let mut wrote_section = false;
    if !ready.is_empty() || !needs_attention.is_empty() || !next.is_empty() {
        writeln!(stdout)?;
    }

    if !ready.is_empty() {
        render_item_section(stdout, "Ready", ready)?;
        wrote_section = true;
    }

    if !needs_attention.is_empty() {
        if wrote_section {
            writeln!(stdout)?;
        }
        render_item_section(stdout, "Needs attention", needs_attention)?;
        wrote_section = true;
    }

    if !next.is_empty() {
        if wrote_section {
            writeln!(stdout)?;
        }
        render_item_section(stdout, "Next", next)?;
    }

    Ok(())
}

fn render_item_section(
    stdout: &mut dyn Write,
    title: &str,
    items: &[String],
) -> Result<(), RuntimeError> {
    writeln!(stdout, "{title}")?;
    for item in items {
        writeln!(stdout, "  {item}")?;
    }
    Ok(())
}

fn push_row(rows: &mut Vec<(&'static str, String)>, label: &'static str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        rows.push((label, value));
    }
}

fn first_present<const N: usize>(values: [Option<&str>; N]) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .find_map(|value| non_empty_str(value).map(str::to_owned))
}

fn non_empty_str(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn humanize_machine_label(value: &str) -> String {
    value
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(capitalize_ascii_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn order_submit_watch_headline(view: &OrderSubmitView) -> &'static str {
    match view.state.as_str() {
        "already_submitted" => "Order already submitted",
        "deduplicated" => "Order already in progress",
        "dry_run" => "Dry run only",
        "error" => "Order submit failed",
        "missing" => "Order draft not found",
        "unavailable" => "Order submit unavailable",
        "unconfigured" => "Not ready yet",
        _ => "Order submitted",
    }
}

fn humanize_delivery_method(value: &str) -> String {
    value
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(capitalize_ascii_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_ascii_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut rendered = String::new();
    rendered.push(first.to_ascii_uppercase());
    rendered.push_str(chars.as_str());
    rendered
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

fn doctor_item(check: &DoctorCheckView) -> String {
    let name = humanize_machine_label(check.name.as_str());
    match non_empty_str(check.detail.as_str()) {
        Some(detail) => format!("{name}: {detail}"),
        None => name,
    }
}

fn write_context(stdout: &mut dyn Write, line: &str) -> Result<(), RuntimeError> {
    writeln!(stdout, "{line}")?;
    writeln!(stdout)?;
    Ok(())
}

fn render_actions(stdout: &mut dyn Write, actions: &[String]) -> Result<(), RuntimeError> {
    if actions.is_empty() {
        return Ok(());
    }
    writeln!(stdout)?;
    writeln!(stdout, "Next")?;
    for action in actions {
        writeln!(stdout, "  {action}")?;
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

fn render_field_rows(stdout: &mut dyn Write, rows: &[(&str, String)]) -> Result<(), RuntimeError> {
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
    let remote_session_count = view.remote_session_count.to_string();
    rows.push(("remote session count", remote_session_count.as_str()));
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
    for session in &view.remote_sessions {
        writeln!(stdout)?;
        render_myc_remote_session(stdout, session)?;
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

fn render_signer_binding(
    stdout: &mut dyn Write,
    binding: &crate::domain::runtime::SignerBindingStatusView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "signer binding")?;
    writeln!(stdout, "  capability: {}", binding.capability_id)?;
    writeln!(stdout, "  provider: {}", binding.provider_runtime_id)?;
    writeln!(stdout, "  model: {}", binding.binding_model)?;
    writeln!(stdout, "  status: {}", binding.state)?;
    writeln!(stdout, "  source: {}", binding.source)?;
    if let Some(target_kind) = &binding.target_kind {
        writeln!(stdout, "  target kind: {target_kind}")?;
    }
    if let Some(target) = &binding.target {
        writeln!(stdout, "  target: {target}")?;
    }
    if let Some(account_ref) = &binding.managed_account_ref {
        writeln!(stdout, "  managed account ref: {account_ref}")?;
    }
    if let Some(session_ref) = &binding.signer_session_ref {
        writeln!(stdout, "  signer session ref: {session_ref}")?;
    }
    if let Some(session_id) = &binding.resolved_signer_session_id {
        writeln!(stdout, "  resolved signer session id: {session_id}")?;
    }
    if let Some(count) = binding.matched_session_count {
        writeln!(stdout, "  matched session count: {count}")?;
    }
    if let Some(reason) = &binding.reason {
        writeln!(stdout, "  reason: {reason}")?;
    }
    Ok(())
}

fn render_myc_remote_session(
    stdout: &mut dyn Write,
    session: &crate::domain::runtime::MycRemoteSessionView,
) -> Result<(), RuntimeError> {
    writeln!(stdout, "myc remote session {}", session.connection_id)?;
    writeln!(stdout, "  signer id: {}", session.signer_identity.id)?;
    writeln!(
        stdout,
        "  signer npub: {}",
        session.signer_identity.public_key_npub
    )?;
    writeln!(stdout, "  user id: {}", session.user_identity.id)?;
    writeln!(
        stdout,
        "  user npub: {}",
        session.user_identity.public_key_npub
    )?;
    writeln!(stdout, "  relay count: {}", session.relay_count)?;
    if !session.permissions.is_empty() {
        writeln!(stdout, "  permissions: {}", session.permissions.join(", "))?;
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

fn format_price(amount: f64, currency: &str, per_amount: u32, per_unit: &str) -> String {
    format!(
        "{} {currency}/{} {per_unit}",
        trim_decimal(amount),
        per_amount
    )
}

fn quantity_offer_text(quantity: &crate::domain::runtime::FindQuantityView) -> Option<String> {
    quantity
        .label
        .as_deref()
        .and_then(non_empty_str)
        .map(str::to_owned)
        .or_else(|| Some(format!("{} {}", quantity.total_amount, quantity.total_unit)))
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
        CommandView::AccountClearDefault(_) => "account clear-default",
        CommandView::AccountImport(_) => "account import",
        CommandView::AccountList(_) => "account list",
        CommandView::AccountNew(_) => "account create",
        CommandView::AccountRemove(_) => "account remove",
        CommandView::AccountUse(_) => "account select",
        CommandView::AccountWhoami(_) => "account view",
        CommandView::ConfigShow(_) => "config show",
        CommandView::Doctor(_) => "doctor",
        CommandView::FarmGet(_) => "farm show",
        CommandView::FarmPublish(_) => "farm publish",
        CommandView::FarmSet(_) => "farm set",
        CommandView::FarmSetup(view) => {
            if view.state == "saved" {
                "farm init"
            } else {
                "farm setup"
            }
        }
        CommandView::FarmStatus(_) => "farm check",
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
        CommandView::MarketSearch(_) => "market search",
        CommandView::MarketUpdate(_) => "market update",
        CommandView::MarketView(_) => "market view",
        CommandView::MycStatus(_) => "myc status",
        CommandView::NetStatus(_) => "net status",
        CommandView::OrderCancel(_) => "order cancel",
        CommandView::OrderGet(_) => "order view",
        CommandView::OrderHistory(_) => "order history",
        CommandView::OrderList(_) => "order list",
        CommandView::OrderNew(_) => "order create",
        CommandView::OrderSubmit(_) => "order submit",
        CommandView::OrderSubmitWatch(_) => "order submit --watch",
        CommandView::OrderWatch(_) => "order watch",
        CommandView::RpcSessions(_) => "rpc sessions",
        CommandView::RpcStatus(_) => "rpc status",
        CommandView::RelayList(_) => "relay ls",
        CommandView::RuntimeAction(view) => match view.action.as_str() {
            "install" => "runtime install",
            "uninstall" => "runtime uninstall",
            "start" => "runtime start",
            "stop" => "runtime stop",
            "restart" => "runtime restart",
            "config_set" => "runtime config set",
            _ => "runtime",
        },
        CommandView::RuntimeConfigShow(_) => "runtime config show",
        CommandView::RuntimeLogs(_) => "runtime logs",
        CommandView::RuntimeStatus(_) => "runtime status",
        CommandView::SellAdd(_) => "sell add",
        CommandView::SellCheck(_) => "sell check",
        CommandView::SellDraftMutation(view) => match view.operation.as_str() {
            "reprice" => "sell reprice",
            "restock" => "sell restock",
            _ => "sell",
        },
        CommandView::SellMutation(view) => match view.operation.as_str() {
            "publish" => "sell publish",
            "update" => "sell update",
            "pause" => "sell pause",
            _ => "sell",
        },
        CommandView::SellShow(_) => "sell show",
        CommandView::Setup(_) => "setup",
        CommandView::SignerStatus(_) => "signer status",
        CommandView::Status(_) => "status",
        CommandView::SyncPull(_) => "sync pull",
        CommandView::SyncPush(_) => "sync push",
        CommandView::SyncStatus(_) => "sync status",
        CommandView::SyncWatch(_) => "sync watch",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Table, render_human_to, render_human_with_config_to, render_ndjson_to, render_table,
    };
    use crate::commands::runtime;
    use crate::domain::runtime::{
        AccountListView, CommandOutput, CommandView, DoctorCheckView, DoctorView, MycStatusView,
        RelayEntryView, RelayListView,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };
    use crate::runtime::logging::LoggingState;
    use radroots_runtime_paths::RadrootsMigrationReport;
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
                interaction: InteractionConfig {
                    input_enabled: true,
                    assume_yes: false,
                    stdin_tty: true,
                    stdout_tty: true,
                    prompts_allowed: true,
                    confirmations_allowed: true,
                },
                paths: PathsConfig {
                    profile: "interactive_user".into(),
                    profile_source: "default".into(),
                    allowed_profiles: vec!["interactive_user".into(), "repo_local".into()],
                    root_source: "host_defaults".into(),
                    repo_local_root: None,
                    repo_local_root_source: None,
                    subordinate_path_override_source: "runtime_config".into(),
                    app_namespace: "apps/cli".into(),
                    shared_accounts_namespace: "shared/accounts".into(),
                    shared_identities_namespace: "shared/identities".into(),
                    app_config_path: "/home/tester/.radroots/config/apps/cli/config.toml".into(),
                    workspace_config_path: "/workspace/infra/local/runtime/radroots/config.toml"
                        .into(),
                    app_data_root: "/home/tester/.radroots/data/apps/cli".into(),
                    app_logs_root: "/home/tester/.radroots/logs/apps/cli".into(),
                    shared_accounts_data_root: "/home/tester/.radroots/data/shared/accounts".into(),
                    shared_accounts_secrets_root: "/home/tester/.radroots/secrets/shared/accounts"
                        .into(),
                    default_identity_path:
                        "/home/tester/.radroots/secrets/shared/identities/default.json".into(),
                },
                migration: MigrationConfig {
                    report: RadrootsMigrationReport::empty(),
                },
                logging: LoggingConfig {
                    filter: "info".to_owned(),
                    directory: None,
                    stdout: false,
                },
                account: AccountConfig {
                    selector: Some("acct_demo".into()),
                    store_path: "/home/tester/.radroots/data/shared/accounts/store.json".into(),
                    secrets_dir: "/home/tester/.radroots/secrets/shared/accounts".into(),
                    secret_backend: RadrootsSecretBackend::EncryptedFile,
                    secret_fallback: None,
                },
                account_secret_contract: AccountSecretContractConfig {
                    default_backend: "host_vault".into(),
                    default_fallback: Some("encrypted_file".into()),
                    allowed_backends: vec!["host_vault".into(), "encrypted_file".into()],
                    host_vault_policy: Some("desktop".into()),
                    uses_protected_store: true,
                },
                identity: IdentityConfig {
                    path: "/home/tester/.radroots/secrets/shared/identities/default.json".into(),
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
                    root: "/home/tester/.radroots/data/apps/cli/replica".into(),
                    replica_db_path: "/home/tester/.radroots/data/apps/cli/replica/replica.sqlite"
                        .into(),
                    backups_dir: "/home/tester/.radroots/data/apps/cli/replica/backups".into(),
                    exports_dir: "/home/tester/.radroots/data/apps/cli/replica/exports".into(),
                },
                myc: MycConfig {
                    executable: "myc".into(),
                },
                hyf: HyfConfig {
                    enabled: false,
                    executable: "hyfd".into(),
                },
                rpc: RpcConfig {
                    url: "http://127.0.0.1:7070".to_owned(),
                    bridge_bearer_token: None,
                },
                capability_bindings: Vec::new(),
            },
            &LoggingState {
                initialized: true,
                current_file: None,
            },
        )
        .expect("runtime show");
        assert_eq!(view.output.format, "human");
        assert!(view.interaction.input_enabled);
        assert!(view.interaction.prompts_allowed);
        assert_eq!(view.paths.profile, "interactive_user");
        assert_eq!(view.paths.app_namespace, "apps/cli");
        assert_eq!(view.paths.shared_accounts_namespace, "shared/accounts");
        assert_eq!(
            view.paths.workspace_config_path,
            "/workspace/infra/local/runtime/radroots/config.toml"
        );
        assert_eq!(view.account.selector.as_deref(), Some("acct_demo"));
        assert!(
            view.account
                .store_path
                .ends_with(".radroots/data/shared/accounts/store.json")
        );
        assert_eq!(view.relay.count, 2);
        assert_eq!(view.relay.publish_policy, "any");
        assert!(!view.hyf.enabled);
        assert_eq!(view.hyf.executable, "hyfd");
        assert_eq!(view.capability_bindings.len(), 4);
        assert_eq!(
            view.account.secret_backend.contract_default_backend,
            "host_vault"
        );
        assert!(
            view.local
                .replica_db_path
                .ends_with(".radroots/data/apps/cli/replica/replica.sqlite")
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
            remote_session_count: 0,
            local_signer: None,
            remote_sessions: Vec::new(),
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
                    interaction: InteractionConfig {
                        input_enabled: true,
                        assume_yes: false,
                        stdin_tty: true,
                        stdout_tty: true,
                        prompts_allowed: true,
                        confirmations_allowed: true,
                    },
                    paths: PathsConfig {
                        profile: "interactive_user".into(),
                        profile_source: "default".into(),
                        allowed_profiles: vec!["interactive_user".into(), "repo_local".into()],
                        root_source: "host_defaults".into(),
                        repo_local_root: None,
                        repo_local_root_source: None,
                        subordinate_path_override_source: "runtime_config".into(),
                        app_namespace: "apps/cli".into(),
                        shared_accounts_namespace: "shared/accounts".into(),
                        shared_identities_namespace: "shared/identities".into(),
                        app_config_path: "/home/tester/.radroots/config/apps/cli/config.toml"
                            .into(),
                        workspace_config_path:
                            "/workspace/infra/local/runtime/radroots/config.toml".into(),
                        app_data_root: "/home/tester/.radroots/data/apps/cli".into(),
                        app_logs_root: "/home/tester/.radroots/logs/apps/cli".into(),
                        shared_accounts_data_root: "/home/tester/.radroots/data/shared/accounts"
                            .into(),
                        shared_accounts_secrets_root:
                            "/home/tester/.radroots/secrets/shared/accounts".into(),
                        default_identity_path:
                            "/home/tester/.radroots/secrets/shared/identities/default.json".into(),
                    },
                    migration: MigrationConfig {
                        report: RadrootsMigrationReport::empty(),
                    },
                    logging: LoggingConfig {
                        filter: "info".to_owned(),
                        directory: None,
                        stdout: false,
                    },
                    account: AccountConfig {
                        selector: None,
                        store_path: "/home/tester/.radroots/data/shared/accounts/store.json".into(),
                        secrets_dir: "/home/tester/.radroots/secrets/shared/accounts".into(),
                        secret_backend: RadrootsSecretBackend::EncryptedFile,
                        secret_fallback: None,
                    },
                    account_secret_contract: AccountSecretContractConfig {
                        default_backend: "host_vault".into(),
                        default_fallback: Some("encrypted_file".into()),
                        allowed_backends: vec!["host_vault".into(), "encrypted_file".into()],
                        host_vault_policy: Some("desktop".into()),
                        uses_protected_store: true,
                    },
                    identity: IdentityConfig {
                        path: "/home/tester/.radroots/secrets/shared/identities/default.json"
                            .into(),
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
                        root: "/home/tester/.radroots/data/apps/cli/replica".into(),
                        replica_db_path:
                            "/home/tester/.radroots/data/apps/cli/replica/replica.sqlite".into(),
                        backups_dir: "/home/tester/.radroots/data/apps/cli/replica/backups".into(),
                        exports_dir: "/home/tester/.radroots/data/apps/cli/replica/exports".into(),
                    },
                    myc: MycConfig {
                        executable: "myc".into(),
                    },
                    hyf: HyfConfig {
                        enabled: false,
                        executable: "hyfd".into(),
                    },
                    rpc: RpcConfig {
                        url: "http://127.0.0.1:7070".to_owned(),
                        bridge_bearer_token: None,
                    },
                    capability_bindings: Vec::new(),
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
            source: "shared account store · local first".to_owned(),
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
    fn human_render_doctor_uses_readiness_sections() {
        let output = CommandOutput::unconfigured(CommandView::Doctor(DoctorView {
            ok: false,
            state: "warn".to_owned(),
            account_resolution: crate::domain::runtime::AccountResolutionView {
                source: "none".to_owned(),
                resolved_account: None,
                default_account: None,
            },
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
        assert!(rendered.contains("Readiness check"));
        assert!(rendered.contains("Ready"));
        assert!(rendered.contains("Config: defaults active"));
        assert!(rendered.contains("Needs attention"));
        assert!(rendered.contains("Account: no local account in store"));
        assert!(rendered.contains("Next"));
        assert!(rendered.contains("radroots account new"));
        assert!(!rendered.contains("source: local diagnostics"));
    }

    #[test]
    fn human_render_verbose_and_trace_append_diagnostics() {
        let output = CommandOutput::success(CommandView::Doctor(DoctorView {
            ok: true,
            state: "ok".to_owned(),
            account_resolution: crate::domain::runtime::AccountResolutionView {
                source: "default_account".to_owned(),
                resolved_account: None,
                default_account: None,
            },
            checks: vec![DoctorCheckView {
                name: "config".to_owned(),
                status: "ok".to_owned(),
                detail: "defaults active".to_owned(),
            }],
            source: "local diagnostics".to_owned(),
            actions: Vec::new(),
        }));

        let mut verbose_buffer = Vec::new();
        render_human_with_config_to(
            &mut verbose_buffer,
            &output,
            &OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Verbose,
                color: false,
                dry_run: false,
            },
        )
        .expect("render verbose");
        let verbose_rendered = String::from_utf8(verbose_buffer).expect("utf8");
        assert!(verbose_rendered.contains("Details"));
        assert!(verbose_rendered.contains("Source"));
        assert!(!verbose_rendered.contains("Trace"));

        let mut trace_buffer = Vec::new();
        render_human_with_config_to(
            &mut trace_buffer,
            &output,
            &OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Trace,
                color: false,
                dry_run: false,
            },
        )
        .expect("render trace");
        let trace_rendered = String::from_utf8(trace_buffer).expect("utf8");
        assert!(trace_rendered.contains("Details"));
        assert!(trace_rendered.contains("Trace"));
        assert!(trace_rendered.contains("\"source\": \"local diagnostics\""));
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
