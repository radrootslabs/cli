use crate::cli::{FindArgs, RecordKeyArgs};
use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, FindView, ListingGetView, SyncActionView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub fn update(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = market_update_view(crate::runtime::sync::pull(config)?);
    Ok(market_update_output(view))
}

pub fn search(config: &RuntimeConfig, args: &FindArgs) -> Result<CommandOutput, RuntimeError> {
    let view = market_search_view(crate::runtime::find::search(config, args)?);
    Ok(market_search_output(view))
}

pub fn view(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<CommandOutput, RuntimeError> {
    let view = market_view_view(crate::runtime::listing::get(config, args)?);
    Ok(market_view_output(view))
}

fn market_update_view(mut view: SyncActionView) -> SyncActionView {
    view.actions = match view.state.as_str() {
        "ready" => vec!["radroots market search tomatoes".to_owned()],
        "unavailable" => vec![
            "radroots rpc status".to_owned(),
            "radroots runtime status radrootsd".to_owned(),
            "radroots sync status".to_owned(),
        ],
        "unconfigured" => {
            let mut actions = Vec::new();
            if view.replica_db == "missing" {
                actions.push("radroots local init".to_owned());
            }
            if view.relay_count == 0 {
                actions.push("radroots relay list --relay wss://relay.example.com".to_owned());
            }
            if actions.is_empty() {
                actions.extend(view.actions.clone());
            }
            actions
        }
        _ => view.actions.clone(),
    };
    view
}

fn market_search_view(mut view: FindView) -> FindView {
    view.actions = match view.state.as_str() {
        "ready" => view
            .results
            .first()
            .map(|result| {
                vec![
                    format!("radroots market view {}", result.product_key),
                    format!("radroots order create --listing {}", result.product_key),
                ]
            })
            .unwrap_or_default(),
        "empty" => vec![
            "radroots market update".to_owned(),
            "radroots market search eggs".to_owned(),
        ],
        _ => view.actions.clone(),
    };
    view
}

fn market_view_view(mut view: ListingGetView) -> ListingGetView {
    view.actions = match view.state.as_str() {
        "ready" => {
            let listing_key = view
                .product_key
                .as_deref()
                .unwrap_or(view.lookup.as_str())
                .to_owned();
            vec![format!("radroots order create --listing {listing_key}")]
        }
        "missing" => vec![
            "radroots market search tomatoes".to_owned(),
            "radroots market update".to_owned(),
        ],
        "unconfigured" => vec![
            "radroots local init".to_owned(),
            "radroots market update".to_owned(),
        ],
        _ => view.actions.clone(),
    };
    view
}

fn market_update_output(view: SyncActionView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::MarketUpdate(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::MarketUpdate(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::MarketUpdate(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::MarketUpdate(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::MarketUpdate(view))
        }
    }
}

fn market_search_output(view: FindView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::MarketSearch(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::MarketSearch(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::MarketSearch(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::MarketSearch(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::MarketSearch(view))
        }
    }
}

fn market_view_output(view: ListingGetView) -> CommandOutput {
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::MarketView(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::MarketView(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::MarketView(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::MarketView(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::MarketView(view))
        }
    }
}
