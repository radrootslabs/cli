use std::time::Duration;

use radroots_events_codec::wire::WireEventParts;
use radroots_identity::RadrootsIdentity;
use radroots_nostr::prelude::{
    RadrootsNostrClient, RadrootsNostrError, RadrootsNostrEvent, RadrootsNostrOutput,
    radroots_nostr_build_event,
};

const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectRelayFailure {
    pub relay: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct DirectRelayPublishReceipt {
    pub event: RadrootsNostrEvent,
    pub event_id: String,
    pub target_relays: Vec<String>,
    pub acknowledged_relays: Vec<String>,
    pub failed_relays: Vec<DirectRelayFailure>,
}

#[derive(Debug, thiserror::Error)]
pub enum DirectRelayPublishError {
    #[error("direct relay publish requires at least one configured relay")]
    MissingRelays,
    #[error("failed to build async runtime for direct relay publish: {0}")]
    Runtime(String),
    #[error("failed to build Nostr event for direct relay publish: {0}")]
    Build(#[source] RadrootsNostrError),
    #[error("failed to sign Nostr event for direct relay publish: {0}")]
    Sign(#[source] RadrootsNostrError),
    #[error("failed to configure relay `{relay}` for direct relay publish: {source}")]
    RelayConfig {
        relay: String,
        #[source]
        source: RadrootsNostrError,
    },
    #[error("direct relay connection failed: {0}")]
    Connect(String),
    #[error("direct relay publish failed for event `{event_id}`: {reason}")]
    Publish { event_id: String, reason: String },
}

pub fn publish_parts_with_identity(
    identity: &RadrootsIdentity,
    relay_urls: &[String],
    parts: WireEventParts,
) -> Result<DirectRelayPublishReceipt, DirectRelayPublishError> {
    if relay_urls.is_empty() {
        return Err(DirectRelayPublishError::MissingRelays);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| DirectRelayPublishError::Runtime(error.to_string()))?;

    runtime.block_on(publish_parts_with_identity_async(
        identity, relay_urls, parts,
    ))
}

async fn publish_parts_with_identity_async(
    identity: &RadrootsIdentity,
    relay_urls: &[String],
    parts: WireEventParts,
) -> Result<DirectRelayPublishReceipt, DirectRelayPublishError> {
    let builder = radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
        .map_err(DirectRelayPublishError::Build)?;
    let event = builder
        .sign_with_keys(identity.keys())
        .map_err(|error| DirectRelayPublishError::Sign(error.into()))?;
    let event_id = event.id.to_hex();
    let client = RadrootsNostrClient::from_identity(identity);

    for relay_url in relay_urls {
        client.add_write_relay(relay_url).await.map_err(|source| {
            DirectRelayPublishError::RelayConfig {
                relay: relay_url.clone(),
                source,
            }
        })?;
    }

    let connection_output = client.try_connect(RELAY_CONNECT_TIMEOUT).await;
    if connection_output.success.is_empty() {
        return Err(DirectRelayPublishError::Connect(summarize_failures(
            &relay_failures_from_output(&connection_output),
        )));
    }

    let publish_output =
        client
            .send_event(&event)
            .await
            .map_err(|source| DirectRelayPublishError::Publish {
                event_id: event_id.clone(),
                reason: source.to_string(),
            })?;
    let failed_relays = relay_failures_from_output(&publish_output);
    if publish_output.success.is_empty() {
        return Err(DirectRelayPublishError::Publish {
            event_id: event_id.clone(),
            reason: summarize_failures(&failed_relays),
        });
    }

    Ok(DirectRelayPublishReceipt {
        event,
        event_id,
        target_relays: relay_urls.to_vec(),
        acknowledged_relays: publish_output
            .success
            .iter()
            .map(ToString::to_string)
            .collect(),
        failed_relays,
    })
}

fn relay_failures_from_output<T: std::fmt::Debug>(
    output: &RadrootsNostrOutput<T>,
) -> Vec<DirectRelayFailure> {
    output
        .failed
        .iter()
        .map(|(relay, reason)| DirectRelayFailure {
            relay: relay.to_string(),
            reason: reason.to_string(),
        })
        .collect()
}

fn summarize_failures(failed_relays: &[DirectRelayFailure]) -> String {
    if failed_relays.is_empty() {
        return "no relay acknowledged the operation".to_owned();
    }

    failed_relays
        .iter()
        .map(|failure| format!("{}: {}", failure.relay, failure.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn event_created_at_u32(event: &RadrootsNostrEvent) -> u32 {
    u32::try_from(event.created_at.as_secs()).unwrap_or(u32::MAX)
}

pub fn event_signature(event: &RadrootsNostrEvent) -> String {
    event.sig.to_string()
}

#[cfg(test)]
mod tests {
    use radroots_events_codec::wire::WireEventParts;
    use radroots_identity::RadrootsIdentity;

    use super::{DirectRelayPublishError, publish_parts_with_identity};

    #[test]
    fn publish_parts_requires_relays_before_runtime_work() {
        let identity = RadrootsIdentity::generate();
        let err = publish_parts_with_identity(
            &identity,
            &[],
            WireEventParts {
                kind: 30402,
                content: "listing".to_owned(),
                tags: Vec::new(),
            },
        )
        .expect_err("missing relay error");

        assert!(matches!(err, DirectRelayPublishError::MissingRelays));
    }
}
