use std::time::Duration;

use radroots_events_codec::wire::WireEventParts;
use radroots_identity::RadrootsIdentity;
use radroots_nostr::prelude::{
    RadrootsNostrClient, RadrootsNostrError, RadrootsNostrEvent, RadrootsNostrFilter,
    RadrootsNostrOutput, radroots_nostr_build_event,
};

const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RELAY_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectRelayFailure {
    pub relay: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct DirectRelayPublishReceipt {
    pub event_id: String,
    pub created_at: u32,
    pub signature: String,
    pub target_relays: Vec<String>,
    pub acknowledged_relays: Vec<String>,
    pub failed_relays: Vec<DirectRelayFailure>,
}

#[derive(Debug, Clone)]
pub struct DirectRelayFetchReceipt {
    pub target_relays: Vec<String>,
    pub connected_relays: Vec<String>,
    pub failed_relays: Vec<DirectRelayFailure>,
    pub events: Vec<RadrootsNostrEvent>,
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

#[derive(Debug, thiserror::Error)]
pub enum DirectRelayFetchError {
    #[error("direct relay fetch requires at least one configured relay")]
    MissingRelays,
    #[error("failed to build async runtime for direct relay fetch: {0}")]
    Runtime(String),
    #[error("failed to configure relay `{relay}` for direct relay fetch: {source}")]
    RelayConfig {
        relay: String,
        #[source]
        source: RadrootsNostrError,
    },
    #[error("direct relay connection failed: {0}")]
    Connect(String),
    #[error("direct relay fetch failed: {0}")]
    Fetch(#[source] RadrootsNostrError),
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

pub fn fetch_events_from_relays(
    relay_urls: &[String],
    filter: RadrootsNostrFilter,
) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError> {
    fetch_events_from_relays_with_timeout(relay_urls, filter, RELAY_FETCH_TIMEOUT)
}

pub fn fetch_events_from_relays_with_timeout(
    relay_urls: &[String],
    filter: RadrootsNostrFilter,
    fetch_timeout: Duration,
) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError> {
    if relay_urls.is_empty() {
        return Err(DirectRelayFetchError::MissingRelays);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| DirectRelayFetchError::Runtime(error.to_string()))?;

    runtime.block_on(fetch_events_from_relays_async(
        relay_urls,
        filter,
        fetch_timeout,
        RELAY_CONNECT_TIMEOUT,
    ))
}

async fn fetch_events_from_relays_async(
    relay_urls: &[String],
    filter: RadrootsNostrFilter,
    fetch_timeout: Duration,
    connect_timeout: Duration,
) -> Result<DirectRelayFetchReceipt, DirectRelayFetchError> {
    let client = RadrootsNostrClient::new_signerless();

    for relay_url in relay_urls {
        client.add_read_relay(relay_url).await.map_err(|source| {
            DirectRelayFetchError::RelayConfig {
                relay: relay_url.clone(),
                source,
            }
        })?;
    }

    let connection_output = client.try_connect(connect_timeout).await;
    let failed_relays = relay_failures_from_output(&connection_output);
    if connection_output.success.is_empty() {
        return Err(DirectRelayFetchError::Connect(summarize_failures(
            &failed_relays,
        )));
    }

    let events = client
        .fetch_events(filter, fetch_timeout)
        .await
        .map_err(DirectRelayFetchError::Fetch)?;

    Ok(DirectRelayFetchReceipt {
        target_relays: relay_urls.to_vec(),
        connected_relays: connection_output
            .success
            .iter()
            .map(ToString::to_string)
            .collect(),
        failed_relays,
        events,
    })
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
    let created_at = event_created_at_u32(&event);
    let signature = event.sig.to_string();
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
        event_id,
        created_at,
        signature,
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

fn event_created_at_u32(event: &radroots_nostr::prelude::RadrootsNostrEvent) -> u32 {
    u32::try_from(event.created_at.as_secs()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use radroots_events_codec::wire::WireEventParts;
    use radroots_identity::RadrootsIdentity;
    use radroots_nostr::prelude::RadrootsNostrFilter;

    use super::{
        DirectRelayFetchError, DirectRelayPublishError, fetch_events_from_relays_async,
        fetch_events_from_relays_with_timeout, publish_parts_with_identity,
    };

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

    #[test]
    fn fetch_events_requires_relays_before_runtime_work() {
        let err = fetch_events_from_relays_with_timeout(
            &[],
            RadrootsNostrFilter::new(),
            Duration::from_millis(1),
        )
        .expect_err("missing relay error");

        assert!(matches!(err, DirectRelayFetchError::MissingRelays));
    }

    #[test]
    fn fetch_events_rejects_invalid_relay_urls() {
        let err = fetch_events_from_relays_with_timeout(
            &["not-a-relay".to_owned()],
            RadrootsNostrFilter::new(),
            Duration::from_millis(1),
        )
        .expect_err("relay config error");

        assert!(matches!(err, DirectRelayFetchError::RelayConfig { .. }));
    }

    #[tokio::test]
    async fn fetch_events_reports_connection_failure() {
        let err = fetch_events_from_relays_async(
            &["ws://127.0.0.1:9".to_owned()],
            RadrootsNostrFilter::new(),
            Duration::from_millis(1),
            Duration::from_millis(50),
        )
        .await
        .expect_err("connection failure");

        assert!(matches!(err, DirectRelayFetchError::Connect(_)));
    }
}
