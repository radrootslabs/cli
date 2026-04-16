use crate::cli::{
    ListingFileArgs, ListingMutationArgs, SellAddArgs, SellRepriceArgs, SellRestockArgs,
    SellShowArgs,
};
use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, SellAddView, SellCheckView,
    SellDraftMutationView, SellMutationView, SellShowView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn add(config: &RuntimeConfig, args: &SellAddArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_add(config, args)?;
    Ok(sell_add_output(view))
}

pub fn show(config: &RuntimeConfig, args: &SellShowArgs) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_show(config, args)?;
    Ok(sell_show_output(view))
}

pub fn check(
    config: &RuntimeConfig,
    args: &ListingFileArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_check(config, args)?;
    Ok(sell_check_output(view))
}

pub fn publish(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_publish(config, args)?;
    Ok(sell_mutation_output(view))
}

pub fn update(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_update(config, args)?;
    Ok(sell_mutation_output(view))
}

pub fn pause(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_pause(config, args)?;
    Ok(sell_mutation_output(view))
}

pub fn reprice(
    config: &RuntimeConfig,
    args: &SellRepriceArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_reprice(config, args)?;
    Ok(sell_draft_mutation_output(view))
}

pub fn restock(
    config: &RuntimeConfig,
    args: &SellRestockArgs,
) -> Result<CommandOutput, RuntimeError> {
    let view = crate::runtime::listing::sell_restock(config, args)?;
    Ok(sell_draft_mutation_output(view))
}

fn sell_add_output(view: SellAddView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SellAdd(view)),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(CommandView::SellAdd(view)),
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SellAdd(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::SellAdd(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SellAdd(view))
        }
    }
}

fn sell_show_output(view: SellShowView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SellShow(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::SellShow(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SellShow(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::SellShow(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SellShow(view))
        }
    }
}

fn sell_check_output(view: SellCheckView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SellCheck(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::SellCheck(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SellCheck(view))
        }
        CommandDisposition::Unsupported => CommandOutput::unsupported(CommandView::SellCheck(view)),
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SellCheck(view))
        }
    }
}

fn sell_mutation_output(view: SellMutationView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SellMutation(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::SellMutation(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SellMutation(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::SellMutation(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SellMutation(view))
        }
    }
}

fn sell_draft_mutation_output(view: SellDraftMutationView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SellDraftMutation(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::SellDraftMutation(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SellDraftMutation(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::SellDraftMutation(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SellDraftMutation(view))
        }
    }
}
