use crate::cli::{OrderNewArgs, OrderSubmitArgs, OrderWatchArgs, RecordKeyArgs};
use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, OrderSubmitView, OrderSubmitWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::{
    CapabilityBindingTargetKind, OutputFormat, RuntimeConfig, WRITE_PLANE_TRADE_JSONRPC_CAPABILITY,
};

pub fn new(config: &RuntimeConfig, args: &OrderNewArgs) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::scaffold(config, args)?;
    rewrite_order_actions(&mut view.actions);
    Ok(command_output(
        view.disposition(),
        CommandView::OrderNew(view),
    ))
}

pub fn get(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::get(config, args)?;
    rewrite_order_actions(&mut view.actions);
    Ok(command_output(
        view.disposition(),
        CommandView::OrderGet(view),
    ))
}

pub fn list(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::list(config)?;
    rewrite_order_actions(&mut view.actions);
    Ok(command_output(
        view.disposition(),
        CommandView::OrderList(view),
    ))
}

pub fn submit(
    config: &RuntimeConfig,
    args: &OrderSubmitArgs,
) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::submit(config, args)?;
    rewrite_order_actions(&mut view.actions);

    if args.watch
        && config.output.format == OutputFormat::Human
        && should_watch_submitted_order(&view)
    {
        let watch_config = watch_runtime_config(config);
        let mut watch = crate::runtime::order::watch(
            &watch_config,
            &OrderWatchArgs {
                key: view.order_id.clone(),
                frames: None,
                interval_ms: 1_000,
            },
        )?;
        rewrite_order_actions(&mut watch.actions);
        let combined = OrderSubmitWatchView {
            submit: view,
            watch,
        };
        return Ok(command_output(
            combined.disposition(),
            CommandView::OrderSubmitWatch(combined),
        ));
    }

    Ok(command_output(
        view.disposition(),
        CommandView::OrderSubmit(view),
    ))
}

pub fn watch(config: &RuntimeConfig, args: &OrderWatchArgs) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::watch(config, args)?;
    rewrite_order_actions(&mut view.actions);
    Ok(command_output(
        view.disposition(),
        CommandView::OrderWatch(view),
    ))
}

pub fn cancel(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::cancel(config, args)?;
    rewrite_order_actions(&mut view.actions);
    Ok(command_output(
        view.disposition(),
        CommandView::OrderCancel(view),
    ))
}

pub fn history(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let mut view = crate::runtime::order::history(config)?;
    rewrite_order_actions(&mut view.actions);
    Ok(command_output(
        view.disposition(),
        CommandView::OrderHistory(view),
    ))
}

fn should_watch_submitted_order(view: &OrderSubmitView) -> bool {
    !matches!(
        view.state.as_str(),
        "dry_run" | "error" | "missing" | "unavailable" | "unconfigured"
    ) && view
        .job
        .as_ref()
        .is_some_and(|job| job.job_id.as_str() != "not_submitted")
}

fn watch_runtime_config(config: &RuntimeConfig) -> RuntimeConfig {
    let mut watch_config = config.clone();
    if let Some(binding) = config.capability_binding(WRITE_PLANE_TRADE_JSONRPC_CAPABILITY) {
        if binding.target_kind == CapabilityBindingTargetKind::ExplicitEndpoint {
            watch_config.rpc.url = binding.target.clone();
        }
    }
    watch_config
}

fn rewrite_order_actions(actions: &mut Vec<String>) {
    for action in actions {
        *action = rewrite_order_action(action.as_str());
    }
}

fn rewrite_order_action(action: &str) -> String {
    if action == "radroots order new" {
        return "radroots order create".to_owned();
    }
    if action == "radroots order ls" {
        return "radroots order list".to_owned();
    }
    if let Some(key) = action.strip_prefix("radroots order get ") {
        return format!("radroots order view {key}");
    }
    action.to_owned()
}

fn command_output(disposition: CommandDisposition, view: CommandView) -> CommandOutput {
    match disposition {
        CommandDisposition::Success => CommandOutput::success(view),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(view),
        CommandDisposition::ExternalUnavailable => CommandOutput::external_unavailable(view),
        CommandDisposition::Unsupported => CommandOutput::unsupported(view),
        CommandDisposition::InternalError => CommandOutput::internal_error(view),
    }
}
