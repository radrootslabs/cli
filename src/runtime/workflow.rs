use crate::cli::{FarmScopedArgs, SetupRoleArg};
use crate::domain::runtime::{SetupView, StatusView};
use crate::runtime::RuntimeError;
use crate::runtime::accounts::{self, AccountRecordView};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::{farm, local};

const WORKFLOW_SOURCE: &str = "workflow summary · local first";
const RELAY_SETUP_ACTION: &str = "radroots relay list --relay wss://relay.example.com";

pub fn setup(config: &RuntimeConfig, role: SetupRoleArg) -> Result<SetupView, RuntimeError> {
    let account = ensure_selected_account(config)?;
    let local_status = ensure_local_status(config)?;
    let farm = inspect_farm(config)?;
    let relay_configured = relay_configured(config);
    let relay_count = config.relay.urls.len();

    let mut ready = vec![
        "Selected account".to_owned(),
        "Local market data".to_owned(),
    ];
    let mut needs_attention = Vec::new();
    let mut next = Vec::new();

    if relay_configured {
        ready.push("Relay configuration".to_owned());
    } else {
        needs_attention.push("Relay configuration".to_owned());
    }

    match role {
        SetupRoleArg::Seller | SetupRoleArg::Both => {
            apply_farm_attention(&mut ready, &mut needs_attention, &mut next, &farm);
            push_next(&mut next, farm.primary_next_action.as_deref());
        }
        SetupRoleArg::Buyer => {}
    }

    match role {
        SetupRoleArg::Buyer | SetupRoleArg::Both if relay_configured => {
            push_next(&mut next, Some("radroots market search tomatoes"));
        }
        _ => {}
    }

    if !relay_configured {
        push_next(&mut next, Some(RELAY_SETUP_ACTION));
    }

    push_next(&mut next, Some("radroots status"));

    Ok(SetupView {
        state: "saved".to_owned(),
        source: WORKFLOW_SOURCE.to_owned(),
        role: role_name(role).to_owned(),
        selected_account_id: account.record.account_id.to_string(),
        local_state: local_status.state,
        local_root: local_status.local_root,
        relay_state: relay_state(config).to_owned(),
        relay_count,
        farm_state: farm.state.to_owned(),
        ready,
        needs_attention,
        next,
    })
}

pub fn status(config: &RuntimeConfig) -> Result<StatusView, RuntimeError> {
    let account = accounts::resolve_account(config)?;
    let local_status = local::status(config)?;
    let farm = inspect_farm(config)?;
    let relay_configured = relay_configured(config);
    let relay_count = config.relay.urls.len();

    let mut ready = Vec::new();
    let mut needs_attention = Vec::new();
    let mut next = Vec::new();
    let mut state = "ready";

    if account.is_some() {
        ready.push("Selected account".to_owned());
    } else {
        state = "unconfigured";
        needs_attention.push("Selected account".to_owned());
    }

    if local_status.state == "ready" {
        ready.push("Local market data".to_owned());
    } else {
        state = "unconfigured";
        needs_attention.push("Local market data".to_owned());
    }

    if relay_configured {
        ready.push("Relay configuration".to_owned());
    } else {
        state = "unconfigured";
        needs_attention.push("Relay configuration".to_owned());
    }

    if state == "ready" {
        apply_farm_attention(&mut ready, &mut needs_attention, &mut next, &farm);

        if relay_configured {
            match farm.state {
                "draft" | "published" => push_next(&mut next, Some("radroots sell add tomatoes")),
                "missing" => push_next(&mut next, Some("radroots market search tomatoes")),
                _ => {}
            }
        }
    } else {
        push_next(&mut next, Some("radroots setup buyer"));
        push_next(&mut next, Some("radroots setup seller"));
        if account.is_some() && local_status.state == "ready" && !relay_configured {
            next.clear();
            push_next(&mut next, Some(RELAY_SETUP_ACTION));
            push_next(&mut next, Some("radroots status"));
        }
    }

    Ok(StatusView {
        state: state.to_owned(),
        source: WORKFLOW_SOURCE.to_owned(),
        selected_account_id: account.map(|account| account.record.account_id.to_string()),
        local_state: local_status.state,
        local_root: local_status.local_root,
        relay_state: relay_state(config).to_owned(),
        relay_count,
        farm_state: farm.state.to_owned(),
        ready,
        needs_attention,
        next,
    })
}

fn ensure_selected_account(config: &RuntimeConfig) -> Result<AccountRecordView, RuntimeError> {
    if let Some(account) = accounts::resolve_account(config)? {
        return Ok(account);
    }

    let snapshot = accounts::snapshot(config)?;
    if let Some(account) = snapshot.accounts.first() {
        return accounts::select_account(config, account.record.account_id.as_str());
    }

    Ok(accounts::create_or_migrate_selected_account(config)?.account)
}

fn ensure_local_status(
    config: &RuntimeConfig,
) -> Result<crate::domain::runtime::LocalStatusView, RuntimeError> {
    let _ = local::init(config)?;
    local::status(config)
}

#[derive(Debug, Clone)]
struct FarmWorkflowState {
    state: &'static str,
    primary_next_action: Option<String>,
}

fn inspect_farm(config: &RuntimeConfig) -> Result<FarmWorkflowState, RuntimeError> {
    let view = farm::status(config, &FarmScopedArgs::default())?;
    if !view.config_present {
        return Ok(FarmWorkflowState {
            state: "missing",
            primary_next_action: view.actions.into_iter().next(),
        });
    }

    if view.account_state != "ready" {
        return Ok(FarmWorkflowState {
            state: "account_missing",
            primary_next_action: view.actions.into_iter().next(),
        });
    }

    let Some(config_summary) = view.config else {
        return Ok(FarmWorkflowState {
            state: "missing",
            primary_next_action: view.actions.into_iter().next(),
        });
    };

    let published = config_summary.publication.profile_state == "published"
        && config_summary.publication.farm_state == "published";

    Ok(FarmWorkflowState {
        state: if published { "published" } else { "draft" },
        primary_next_action: (!published).then(|| "radroots farm publish".to_owned()),
    })
}

fn apply_farm_attention(
    ready: &mut Vec<String>,
    needs_attention: &mut Vec<String>,
    next: &mut Vec<String>,
    farm: &FarmWorkflowState,
) {
    match farm.state {
        "missing" => {
            needs_attention.push("Farm draft".to_owned());
        }
        "draft" => {
            needs_attention.push("Farm not yet published".to_owned());
            push_next(next, Some("radroots farm publish"));
        }
        "published" => {
            ready.push("Farm published".to_owned());
        }
        "account_missing" => {
            needs_attention.push("Farm draft account not available locally".to_owned());
        }
        _ => {}
    }
}

fn relay_configured(config: &RuntimeConfig) -> bool {
    !config.relay.urls.is_empty()
}

fn relay_state(config: &RuntimeConfig) -> &'static str {
    if relay_configured(config) {
        "configured"
    } else {
        "unconfigured"
    }
}

fn role_name(role: SetupRoleArg) -> &'static str {
    match role {
        SetupRoleArg::Seller => "seller",
        SetupRoleArg::Buyer => "buyer",
        SetupRoleArg::Both => "both",
    }
}

fn push_next(next: &mut Vec<String>, command: Option<&str>) {
    let Some(command) = command else {
        return;
    };
    if !next.iter().any(|existing| existing == command) {
        next.push(command.to_owned());
    }
}
