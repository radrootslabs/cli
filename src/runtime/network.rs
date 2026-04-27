use crate::domain::runtime::{RelayEntryView, RelayListView};
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

fn relay_actions(config: &RuntimeConfig) -> Vec<String> {
    if config.relay.urls.is_empty() {
        vec!["radroots --relay wss://relay.example.com relay list".to_owned()]
    } else {
        Vec::new()
    }
}
