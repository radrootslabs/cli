use crate::domain::runtime::{NetStatusView, RelayEntryView, RelayListView};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::RuntimeConfig;

pub fn relay_list(config: &RuntimeConfig) -> RelayListView {
    let relays = config
        .relay
        .urls
        .iter()
        .cloned()
        .map(|url| RelayEntryView {
            url,
            read: true,
            write: true,
        })
        .collect::<Vec<_>>();

    let state = if relays.is_empty() {
        "unconfigured"
    } else {
        "configured"
    };

    RelayListView {
        state: state.to_owned(),
        source: config.relay.source.as_str().to_owned(),
        publish_policy: config.relay.publish_policy.as_str().to_owned(),
        count: relays.len(),
        reason: relays
            .is_empty()
            .then_some("no relays are configured for this operator session".to_owned()),
        relays,
        actions: relay_actions(config),
    }
}

pub fn net_status(config: &RuntimeConfig) -> Result<NetStatusView, RuntimeError> {
    let active_account_id =
        accounts::resolve_account(config)?.map(|account| account.record.account_id.to_string());
    let relay_count = config.relay.urls.len();
    let configured = relay_count > 0;

    Ok(NetStatusView {
        state: if configured {
            "configured".to_owned()
        } else {
            "unconfigured".to_owned()
        },
        source: config.relay.source.as_str().to_owned(),
        session: if configured {
            "not_started".to_owned()
        } else {
            "not_configured".to_owned()
        },
        relay_count,
        publish_policy: config.relay.publish_policy.as_str().to_owned(),
        signer_mode: config.signer.backend.as_str().to_owned(),
        active_account_id,
        reason: (!configured)
            .then_some("no relays are configured for this operator session".to_owned()),
        actions: relay_actions(config),
    })
}

fn relay_actions(config: &RuntimeConfig) -> Vec<String> {
    if config.relay.urls.is_empty() {
        vec!["radroots relay ls --relay wss://relay.example.com".to_owned()]
    } else {
        Vec::new()
    }
}
