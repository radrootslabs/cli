use radroots_sdk::{
    PushOutboxEventState, PushOutboxReceipt, PushOutboxRequest, PushOutboxTargetOutcomeKind,
    SyncStatusRequest,
};
use radroots_transport::{
    RADROOTS_RETICULUM_ENDPOINT_URI, RADROOTS_RETICULUM_SCOPE_ID,
    RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE, RadrootsTransportCapabilityAvailability,
    RadrootsTransportCapabilityMaturity, RadrootsTransportImplementationState,
    RadrootsTransportKind, RadrootsTransportStatus,
};
use serde_json::Value as JsonValue;
use std::fs;
use toml::{Value, map::Map};

use crate::ops::OperationData;
use crate::runtime::RuntimeError;
use crate::runtime::config::{RuntimeConfig, TransportProfileKind};
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession, sdk_nostr_relay_url_policy};
use crate::view::runtime::{
    TransportDeliveryInspectView, TransportDeliveryRetryView, TransportOperationCapabilitiesView,
    TransportProfileSummaryView, TransportProfileView, TransportRuntimeStatusView,
    TransportStatusInspectView,
};

const TRANSPORT_SOURCE: &str = "transport profile config";
pub fn profile(config: &RuntimeConfig) -> TransportProfileView {
    active_profile_view(config)
}

pub fn set_profile(
    config: &RuntimeConfig,
    input: &OperationData,
) -> Result<TransportProfileView, RuntimeError> {
    let kind = string_input(input, "kind").unwrap_or("local_only");
    let mut transport = Map::new();
    transport.insert("profile".to_owned(), Value::String(kind.to_owned()));
    match kind {
        "local_only" => {}
        "nostr" => {
            let relays = string_array_input(input, "nostr_relays");
            if relays.is_empty() {
                return Err(RuntimeError::Config(
                    "transport profile `nostr` requires at least one --nostr-relay".to_owned(),
                ));
            }
            let mut nostr = Map::new();
            nostr.insert(
                "relay_urls".to_owned(),
                Value::Array(relays.into_iter().map(Value::String).collect()),
            );
            transport.insert("nostr".to_owned(), Value::Table(nostr));
        }
        "reticulum" => {
            let behavior =
                string_input(input, "reticulum_behavior").unwrap_or("reject_delivery_attempts");
            let scope =
                string_input(input, "reticulum_scope").unwrap_or(RADROOTS_RETICULUM_SCOPE_ID);
            let mut reticulum = Map::new();
            reticulum.insert("behavior".to_owned(), Value::String(behavior.to_owned()));
            reticulum.insert("scope".to_owned(), Value::String(scope.to_owned()));
            if let Some(agent_endpoint) = string_input(input, "reticulum_agent_endpoint") {
                reticulum.insert(
                    "agent_endpoint".to_owned(),
                    Value::String(agent_endpoint.to_owned()),
                );
            }
            transport.insert("reticulum".to_owned(), Value::Table(reticulum));
        }
        "multi_target" => {
            let relays = string_array_input(input, "nostr_relays");
            if relays.is_empty() {
                return Err(RuntimeError::Config(
                    "transport profile `multi_target` requires at least one --nostr-relay"
                        .to_owned(),
                ));
            }
            let behavior =
                string_input(input, "reticulum_behavior").unwrap_or("reject_delivery_attempts");
            let scope =
                string_input(input, "reticulum_scope").unwrap_or(RADROOTS_RETICULUM_SCOPE_ID);
            let mut nostr = Map::new();
            nostr.insert(
                "relay_urls".to_owned(),
                Value::Array(relays.into_iter().map(Value::String).collect()),
            );
            let mut reticulum = Map::new();
            reticulum.insert("behavior".to_owned(), Value::String(behavior.to_owned()));
            reticulum.insert("scope".to_owned(), Value::String(scope.to_owned()));
            if let Some(agent_endpoint) = string_input(input, "reticulum_agent_endpoint") {
                reticulum.insert(
                    "agent_endpoint".to_owned(),
                    Value::String(agent_endpoint.to_owned()),
                );
            }
            transport.insert("nostr".to_owned(), Value::Table(nostr));
            transport.insert("reticulum".to_owned(), Value::Table(reticulum));
        }
        other => {
            return Err(RuntimeError::Config(format!(
                "transport profile kind `{other}` is not supported"
            )));
        }
    }
    update_app_config_table(config, "transport", Value::Table(transport))?;
    Ok(profile_view_from_parts(
        kind,
        string_array_input(input, "nostr_relays"),
        string_input(input, "reticulum_behavior").map(str::to_owned),
        reticulum_scope_for_profile(kind, input),
        string_input(input, "reticulum_agent_endpoint").map(str::to_owned),
        "configured",
    ))
}

pub fn status(config: &RuntimeConfig) -> TransportStatusInspectView {
    let profile = active_profile_view(config);

    TransportStatusInspectView {
        state: "ready".to_owned(),
        source: TRANSPORT_SOURCE.to_owned(),
        active_profile: profile.summary(),
        transports: profile.transport_statuses,
    }
}

pub fn outbox_status(
    config: &RuntimeConfig,
) -> Result<TransportDeliveryInspectView, CliSdkAdapterError> {
    let profile = active_profile_view(config);
    let session = if profile.profile_delivery_usable {
        CliSdkSession::connect(config)?
    } else {
        CliSdkSession::connect_storage_status(config)?
    };
    let receipt = session.block_on(session.sdk().sync().status(SyncStatusRequest::new()))?;
    let state = if profile.configured_state == "configured" {
        "ready".to_owned()
    } else {
        profile.configured_state.clone()
    };
    Ok(TransportDeliveryInspectView {
        state,
        source: "SDK transport outbox".to_owned(),
        transport_profile: profile.profile_id,
        total_count: receipt.outbox.total_events,
        pending_count: receipt.outbox.pending_events,
        retryable_count: receipt.outbox.retryable_events,
        terminal_count: receipt.outbox.terminal_events,
        deferred_until_implemented_count: receipt.outbox.deferred_until_implemented_events,
        ready_signed_count: receipt.outbox.ready_signed_events,
        publishing_count: receipt.outbox.publishing_events,
        last_attempt_at_ms: receipt.outbox.last_attempt_at_ms,
        last_error: receipt.outbox.last_error,
        actions: vec!["radroots transport outbox push".to_owned()],
    })
}

pub fn outbox_push(
    config: &RuntimeConfig,
) -> Result<TransportDeliveryRetryView, CliSdkAdapterError> {
    if config.output.dry_run {
        let status = outbox_status(config)?;
        return Ok(TransportDeliveryRetryView {
            state: "dry_run".to_owned(),
            source: "SDK transport outbox".to_owned(),
            attempted_events: 0,
            published_events: 0,
            retryable_events: 0,
            terminal_events: 0,
            target_count: usize::try_from(status.ready_signed_count).unwrap_or_default(),
            reason: Some("dry run requested; transport outbox push skipped".to_owned()),
            actions: vec!["radroots transport outbox status".to_owned()],
        });
    }
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().sync().push_outbox(
        PushOutboxRequest::new().with_nostr_relay_url_policy(sdk_nostr_relay_url_policy(config)),
    ))?;
    let target_count = receipt
        .events
        .iter()
        .flat_map(|event| event.targets.iter())
        .count();
    let failed_count = receipt.retryable_events + receipt.terminal_events;
    let state = transport_outbox_push_state(&receipt, failed_count).to_owned();
    Ok(TransportDeliveryRetryView {
        state,
        source: "SDK transport outbox".to_owned(),
        attempted_events: receipt.attempted_events,
        published_events: receipt.published_events,
        retryable_events: receipt.retryable_events,
        terminal_events: receipt.terminal_events,
        target_count,
        reason: transport_outbox_push_reason(&receipt),
        actions: vec!["radroots transport outbox status".to_owned()],
    })
}

fn transport_outbox_push_state(receipt: &PushOutboxReceipt, failed_count: usize) -> &'static str {
    if receipt.attempted_events == 0 {
        return transport_outbox_reported_deferred_state(receipt).unwrap_or("ready");
    }
    if receipt.published_events > 0 && failed_count > 0 {
        "partial"
    } else if failed_count > 0 {
        "unavailable"
    } else if receipt.published_events > 0 {
        "published"
    } else {
        "ready"
    }
}

fn transport_outbox_reported_deferred_state(receipt: &PushOutboxReceipt) -> Option<&'static str> {
    let mut deferred = false;
    for event in &receipt.events {
        if event.final_state == PushOutboxEventState::DeferredUntilImplemented {
            deferred = true;
        }
        for target in &event.targets {
            if target.outcome_kind == PushOutboxTargetOutcomeKind::DeferredUntilImplemented {
                deferred = true;
            }
        }
    }
    deferred.then_some("deferred_until_implemented")
}

fn transport_outbox_push_reason(receipt: &PushOutboxReceipt) -> Option<String> {
    if receipt.attempted_events == 0 {
        if let Some(state) = transport_outbox_reported_deferred_state(receipt) {
            return Some(match state {
                "deferred_until_implemented" => {
                    "SDK outbox push reported Reticulum work as deferred until implemented without network delivery"
                }
                _ => "SDK outbox push reported Reticulum work without network delivery",
            }
            .to_owned());
        }
        return Some("SDK outbox had no ready signed events to push".to_owned());
    }
    None
}

fn active_profile_view(config: &RuntimeConfig) -> TransportProfileView {
    match config.transport.profile {
        TransportProfileKind::LocalOnly => profile_view_from_parts(
            "local_only",
            Vec::new(),
            None,
            None,
            None,
            "configured",
        ),
        TransportProfileKind::Nostr => profile_view_from_parts(
            "nostr",
            config.transport.nostr_relay_urls.clone(),
            None,
            None,
            None,
            if config.transport.nostr_relay_urls.is_empty() {
                "unconfigured"
            } else {
                "configured"
            },
        ),
        TransportProfileKind::Reticulum => profile_view_from_parts(
            "reticulum",
            Vec::new(),
            Some(config.transport.reticulum_behavior.as_str().to_owned()),
            Some(config.transport.reticulum_scope.clone()),
            config.transport.reticulum_agent_endpoint.clone(),
            "configured",
        ),
        TransportProfileKind::MultiTarget => profile_view_from_parts(
            "multi_target",
            config.transport.nostr_relay_urls.clone(),
            Some(config.transport.reticulum_behavior.as_str().to_owned()),
            Some(config.transport.reticulum_scope.clone()),
            config.transport.reticulum_agent_endpoint.clone(),
            if config.transport.nostr_relay_urls.is_empty() {
                "unconfigured"
            } else {
                "configured"
            },
        )
        .with_message(
            "Multi-target transport publishes through configured Nostr relays and reports Reticulum availability"
                .to_owned(),
        ),
    }
}

fn profile_view_from_parts(
    profile_id: &str,
    nostr_relays: Vec<String>,
    reticulum_behavior: Option<String>,
    reticulum_scope: Option<String>,
    reticulum_agent_endpoint: Option<String>,
    configured_state: &str,
) -> TransportProfileView {
    let transport_statuses = transport_statuses_from_parts(profile_id, nostr_relays.as_slice());
    let profile_delivery_usable = transport_statuses
        .iter()
        .any(|status| status.usable_for_delivery);
    let message = match profile_id {
        "nostr" if profile_delivery_usable => "Nostr relay transport is configured for delivery",
        "nostr" => "Nostr transport requires configured Nostr relay targets",
        "reticulum" => RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE,
        "multi_target" if profile_delivery_usable => {
            "Multi-target transport publishes through configured Nostr relays and reports Reticulum availability"
        }
        "multi_target" => "Multi-target transport requires configured Nostr relay targets",
        _ => "Local-only profile does not deliver to network transports",
    };
    TransportProfileView {
        state: configured_state.to_owned(),
        source: TRANSPORT_SOURCE.to_owned(),
        profile_id: profile_id.to_owned(),
        profile_kind: profile_id.to_owned(),
        configured_state: configured_state.to_owned(),
        profile_delivery_usable,
        message: message.to_owned(),
        nostr_relays,
        reticulum_behavior,
        reticulum_scope,
        reticulum_agent_endpoint,
        transport_statuses,
        actions: profile_actions(profile_id, profile_delivery_usable),
    }
}

fn transport_statuses_from_parts(
    profile_id: &str,
    nostr_relays: &[String],
) -> Vec<TransportRuntimeStatusView> {
    match profile_id {
        "nostr" => vec![transport_runtime_status_view(nostr_transport_status(
            profile_id,
            !nostr_relays.is_empty(),
        ))],
        "reticulum" => {
            vec![transport_runtime_status_view(reticulum_transport_status(
                profile_id,
            ))]
        }
        "multi_target" => vec![
            transport_runtime_status_view(nostr_transport_status(
                profile_id,
                !nostr_relays.is_empty(),
            )),
            transport_runtime_status_view(reticulum_transport_status(profile_id)),
        ],
        _ => vec![transport_runtime_status_view(local_transport_status(
            profile_id,
        ))],
    }
}

fn local_transport_status(profile_id: &str) -> RadrootsTransportStatus {
    RadrootsTransportStatus::new(
        RadrootsTransportKind::Local,
        true,
        RadrootsTransportImplementationState::Real,
        false,
        "Local-only profile writes only to local state",
    )
    .with_profile_id(profile_id)
}

fn nostr_transport_status(profile_id: &str, targets_configured: bool) -> RadrootsTransportStatus {
    RadrootsTransportStatus::new(
        RadrootsTransportKind::Nostr,
        targets_configured,
        RadrootsTransportImplementationState::Real,
        targets_configured,
        if targets_configured {
            "Nostr relay transport is configured for delivery"
        } else {
            "Nostr transport requires configured Nostr relay targets"
        },
    )
    .with_profile_id(profile_id)
}

fn reticulum_transport_status(profile_id: &str) -> RadrootsTransportStatus {
    RadrootsTransportStatus::new(
        RadrootsTransportKind::Reticulum,
        true,
        RadrootsTransportImplementationState::Real,
        false,
        RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE,
    )
    .with_profile_id(profile_id)
    .with_endpoint_uri(RADROOTS_RETICULUM_ENDPOINT_URI)
    .with_maturity(RadrootsTransportCapabilityMaturity::Preview)
    .with_availability(RadrootsTransportCapabilityAvailability::Unavailable)
}

fn transport_runtime_status_view(status: RadrootsTransportStatus) -> TransportRuntimeStatusView {
    TransportRuntimeStatusView {
        transport: status.kind.canonical_label(),
        profile_id: status.profile_id,
        endpoint_uri: status.endpoint_uri,
        configured: status.configured,
        implementation: transport_implementation_label(status.implementation).to_owned(),
        maturity: transport_maturity_label(status.maturity).to_owned(),
        availability: transport_availability_label(status.availability).to_owned(),
        usable_for_delivery: status.usable_for_delivery,
        capabilities: TransportOperationCapabilitiesView {
            deliver: status.capabilities.deliver,
            fetch: status.capabilities.fetch,
        },
        message: status.message,
    }
}

fn transport_implementation_label(state: RadrootsTransportImplementationState) -> &'static str {
    match state {
        RadrootsTransportImplementationState::Real => "real",
        RadrootsTransportImplementationState::Mock => "mock",
    }
}

fn transport_maturity_label(maturity: RadrootsTransportCapabilityMaturity) -> &'static str {
    match maturity {
        RadrootsTransportCapabilityMaturity::Preview => "preview",
        RadrootsTransportCapabilityMaturity::Stable => "stable",
    }
}

fn transport_availability_label(
    availability: RadrootsTransportCapabilityAvailability,
) -> &'static str {
    match availability {
        RadrootsTransportCapabilityAvailability::Available => "available",
        RadrootsTransportCapabilityAvailability::Degraded => "degraded",
        RadrootsTransportCapabilityAvailability::Unavailable => "unavailable",
    }
}

fn reticulum_scope_for_profile(profile_id: &str, input: &OperationData) -> Option<String> {
    match profile_id {
        "reticulum" | "multi_target" => Some(
            string_input(input, "reticulum_scope")
                .unwrap_or(RADROOTS_RETICULUM_SCOPE_ID)
                .to_owned(),
        ),
        _ => None,
    }
}

fn profile_actions(profile_id: &str, profile_delivery_usable: bool) -> Vec<String> {
    if profile_delivery_usable {
        return Vec::new();
    }
    match profile_id {
        "reticulum" => vec![
            "radroots transport status inspect".to_owned(),
            "radroots transport config inspect".to_owned(),
        ],
        "nostr" => {
            vec![
                "radroots transport config update --kind nostr --nostr-relay wss://relay.example.com"
                    .to_owned(),
            ]
        }
        "multi_target" => {
            vec![
                "radroots transport config update --kind multi-target --nostr-relay wss://relay.example.com"
                    .to_owned(),
            ]
        }
        _ => vec!["radroots transport config inspect".to_owned()],
    }
}

trait TransportProfileViewMessage {
    fn with_message(self, message: String) -> Self;
    fn summary(&self) -> TransportProfileSummaryView;
}

impl TransportProfileViewMessage for TransportProfileView {
    fn with_message(mut self, message: String) -> Self {
        self.message = message;
        self
    }

    fn summary(&self) -> TransportProfileSummaryView {
        TransportProfileSummaryView {
            profile_id: self.profile_id.clone(),
            profile_kind: self.profile_kind.clone(),
            configured_state: self.configured_state.clone(),
            profile_delivery_usable: self.profile_delivery_usable,
            message: self.message.clone(),
        }
    }
}

pub(crate) fn update_app_config_table(
    config: &RuntimeConfig,
    key: &str,
    value: Value,
) -> Result<(), RuntimeError> {
    let path = &config.paths.app_config_path;
    let mut document = if path.exists() {
        let raw = fs::read_to_string(path)?;
        toml::from_str::<Value>(&raw)
            .map_err(|error| RuntimeError::Config(format!("failed to parse app config: {error}")))?
    } else {
        Value::Table(Map::new())
    };
    let Some(table) = document.as_table_mut() else {
        return Err(RuntimeError::Config(
            "app config root must be a TOML table".to_owned(),
        ));
    };
    table.remove("publish");
    table.remove("relays");
    table.insert(key.to_owned(), value);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let rendered = toml::to_string_pretty(&document)
        .map_err(|error| RuntimeError::Config(format!("failed to render app config: {error}")))?;
    fs::write(path, rendered)?;
    Ok(())
}

fn string_input<'a>(input: &'a OperationData, key: &str) -> Option<&'a str> {
    input.get(key).and_then(JsonValue::as_str)
}

fn string_array_input(input: &OperationData, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use radroots_event::id::RadrootsEventId;
    use radroots_sdk::{
        PushOutboxEventReceipt, PushOutboxTargetReceipt, PushOutboxTransportOutcomeKind,
    };

    #[test]
    fn transport_outbox_push_reports_reticulum_deferred_without_attempts() {
        let cases = [(
            PushOutboxEventState::DeferredUntilImplemented,
            PushOutboxTargetOutcomeKind::DeferredUntilImplemented,
            "deferred_until_implemented",
            "SDK outbox push reported Reticulum work as deferred until implemented without network delivery",
        )];

        for (final_state, outcome_kind, expected_state, expected_reason) in cases {
            let receipt = reticulum_receipt(final_state, outcome_kind);

            assert_eq!(transport_outbox_push_state(&receipt, 0), expected_state);
            assert_eq!(
                transport_outbox_push_reason(&receipt).as_deref(),
                Some(expected_reason)
            );
        }
    }

    fn reticulum_receipt(
        final_state: PushOutboxEventState,
        outcome_kind: PushOutboxTargetOutcomeKind,
    ) -> PushOutboxReceipt {
        PushOutboxReceipt {
            attempted_events: 0,
            published_events: 0,
            retryable_events: 0,
            terminal_events: 0,
            events: vec![PushOutboxEventReceipt {
                event_id: RadrootsEventId::parse("d".repeat(64).as_str()).expect("event id"),
                outbox_event_id: 11,
                final_state,
                attempted_count: 0,
                accepted_count: 0,
                retryable_count: 0,
                terminal_count: 0,
                quorum: 1,
                quorum_met: false,
                targets: vec![PushOutboxTargetReceipt {
                    transport_kind: "reticulum".to_owned(),
                    endpoint_uri: RADROOTS_RETICULUM_ENDPOINT_URI.to_owned(),
                    target_scope: Some(RADROOTS_RETICULUM_SCOPE_ID.to_owned()),
                    target_label: None,
                    outcome_kind,
                    transport_outcome_kind: Some(match outcome_kind {
                        PushOutboxTargetOutcomeKind::DeferredUntilImplemented => {
                            PushOutboxTransportOutcomeKind::DeferredUntilImplemented
                        }
                        _ => PushOutboxTransportOutcomeKind::TransportUnavailable,
                    }),
                    attempted: false,
                    message: Some(RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE.to_owned()),
                }],
            }],
        }
    }
}
