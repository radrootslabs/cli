use std::fs;
use std::path::PathBuf;

use radroots_sdk::{
    PushOutboxEventState, PushOutboxReceipt, PushOutboxRequest, PushOutboxTargetOutcomeKind,
    SyncStatusRequest,
};
use radroots_transport::{
    RADROOTS_RETICULUM_PREVIEW_ENDPOINT_URI, RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE,
    RadrootsTransportImplementationState, RadrootsTransportKind, RadrootsTransportReadinessState,
    RadrootsTransportStatus,
};
use serde_json::Value as JsonValue;
use toml::{Value, map::Map};

use crate::ops::OperationData;
use crate::runtime::RuntimeError;
use crate::runtime::config::{RuntimeConfig, TransportProfileKind};
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession, sdk_nostr_relay_url_policy};
use crate::view::runtime::{
    TransportOutboxPushView, TransportOutboxStatusView, TransportProfileSummaryView,
    TransportProfileView, TransportRuntimeStatusView, TransportStatusView,
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
        "reticulum_preview" => {
            let behavior = string_input(input, "reticulum_preview_behavior")
                .unwrap_or("reject_delivery_attempts");
            let mut preview = Map::new();
            preview.insert("behavior".to_owned(), Value::String(behavior.to_owned()));
            transport.insert("reticulum_preview".to_owned(), Value::Table(preview));
        }
        "hybrid" => {
            let relays = string_array_input(input, "nostr_relays");
            if relays.is_empty() {
                return Err(RuntimeError::Config(
                    "transport profile `hybrid` requires at least one --nostr-relay".to_owned(),
                ));
            }
            let behavior = string_input(input, "reticulum_preview_behavior")
                .unwrap_or("reject_delivery_attempts");
            let mut nostr = Map::new();
            nostr.insert(
                "relay_urls".to_owned(),
                Value::Array(relays.into_iter().map(Value::String).collect()),
            );
            let mut preview = Map::new();
            preview.insert("behavior".to_owned(), Value::String(behavior.to_owned()));
            transport.insert("nostr".to_owned(), Value::Table(nostr));
            transport.insert("reticulum_preview".to_owned(), Value::Table(preview));
        }
        "proxy" => {
            let Some(url) = string_input(input, "proxy_url") else {
                return Err(RuntimeError::Config(
                    "transport profile `proxy` requires --proxy-url".to_owned(),
                ));
            };
            let token_file = string_input(input, "proxy_token_file").map(str::to_owned);
            let token_secret_id = string_input(input, "proxy_token_secret_id").map(str::to_owned);
            validate_proxy_token_source(token_file.as_deref(), token_secret_id.as_deref())?;
            validate_proxy_token_material(
                config,
                url,
                token_file.as_deref(),
                token_secret_id.as_deref(),
            )?;
            let mut proxy = Map::new();
            proxy.insert("url".to_owned(), Value::String(url.to_owned()));
            if let Some(token_file) = token_file.as_ref() {
                proxy.insert(
                    "token_file".to_owned(),
                    Value::String(token_file.to_owned()),
                );
            }
            if let Some(token_secret_id) = token_secret_id.as_ref() {
                proxy.insert(
                    "token_secret_id".to_owned(),
                    Value::String(token_secret_id.to_owned()),
                );
            }
            transport.insert("proxy".to_owned(), Value::Table(proxy));
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
        string_input(input, "reticulum_preview_behavior").map(str::to_owned),
        string_input(input, "proxy_url").map(str::to_owned),
        string_input(input, "proxy_token_file").map(str::to_owned),
        string_input(input, "proxy_token_secret_id").map(str::to_owned),
        "configured",
    ))
}

pub fn status(config: &RuntimeConfig) -> TransportStatusView {
    let profile = active_profile_view(config);

    TransportStatusView {
        state: "ready".to_owned(),
        source: TRANSPORT_SOURCE.to_owned(),
        active_profile: profile.summary(),
        transports: profile.transport_statuses,
    }
}

pub fn outbox_status(
    config: &RuntimeConfig,
) -> Result<TransportOutboxStatusView, CliSdkAdapterError> {
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
    Ok(TransportOutboxStatusView {
        state,
        source: "SDK transport outbox".to_owned(),
        transport_profile: profile.profile_id,
        total_count: receipt.outbox.total_events,
        pending_count: receipt.outbox.pending_events,
        retryable_count: receipt.outbox.retryable_events,
        terminal_count: receipt.outbox.terminal_events,
        preview_unavailable_count: receipt.outbox.preview_unavailable_events,
        deferred_until_implemented_count: receipt.outbox.deferred_until_implemented_events,
        ready_signed_count: receipt.outbox.ready_signed_events,
        publishing_count: receipt.outbox.publishing_events,
        last_attempt_at_ms: receipt.outbox.last_attempt_at_ms,
        last_error: receipt.outbox.last_error,
        actions: vec!["radroots transport outbox push".to_owned()],
    })
}

pub fn outbox_push(config: &RuntimeConfig) -> Result<TransportOutboxPushView, CliSdkAdapterError> {
    if config.output.dry_run {
        let status = outbox_status(config)?;
        return Ok(TransportOutboxPushView {
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
    Ok(TransportOutboxPushView {
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
        return transport_outbox_reported_preview_state(receipt).unwrap_or("ready");
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

fn transport_outbox_reported_preview_state(receipt: &PushOutboxReceipt) -> Option<&'static str> {
    let mut deferred = false;
    for event in &receipt.events {
        match event.final_state {
            PushOutboxEventState::PreviewUnavailable => return Some("preview_unavailable"),
            PushOutboxEventState::DeferredUntilImplemented => deferred = true,
            _ => {}
        }
        for target in &event.targets {
            match target.outcome_kind {
                PushOutboxTargetOutcomeKind::PreviewUnavailable => {
                    return Some("preview_unavailable");
                }
                PushOutboxTargetOutcomeKind::DeferredUntilImplemented => deferred = true,
                _ => {}
            }
        }
    }
    deferred.then_some("deferred_until_implemented")
}

fn transport_outbox_push_reason(receipt: &PushOutboxReceipt) -> Option<String> {
    if receipt.attempted_events == 0 {
        if let Some(state) = transport_outbox_reported_preview_state(receipt) {
            return Some(match state {
                "preview_unavailable" => {
                    "SDK outbox push reported Reticulum preview work as preview unavailable without network delivery"
                }
                "deferred_until_implemented" => {
                    "SDK outbox push reported Reticulum preview work as deferred until implemented without network delivery"
                }
                _ => "SDK outbox push reported Reticulum preview work without network delivery",
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
            None,
            "configured",
        ),
        TransportProfileKind::Nostr => profile_view_from_parts(
            "nostr",
            config.transport.nostr_relay_urls.clone(),
            None,
            None,
            None,
            None,
            if config.transport.nostr_relay_urls.is_empty() {
                "unconfigured"
            } else {
                "configured"
            },
        ),
        TransportProfileKind::ReticulumPreview => profile_view_from_parts(
            "reticulum_preview",
            Vec::new(),
            Some(
                config
                    .transport
                    .reticulum_preview_behavior
                    .as_str()
                    .to_owned(),
            ),
            None,
            None,
            None,
            "preview_unavailable",
        ),
        TransportProfileKind::Hybrid => profile_view_from_parts(
            "hybrid",
            config.transport.nostr_relay_urls.clone(),
            Some(
                config
                    .transport
                    .reticulum_preview_behavior
                    .as_str()
                    .to_owned(),
            ),
            None,
            None,
            None,
            if config.transport.nostr_relay_urls.is_empty() {
                "unconfigured"
            } else {
                "configured"
            },
        )
        .with_message(
            "Hybrid transport publishes through configured Nostr relays and reports Reticulum preview status"
                .to_owned(),
        ),
        TransportProfileKind::Proxy => {
            let proxy_readiness = proxy_token_ready(config);
            profile_view_from_parts(
                "proxy",
                Vec::new(),
                None,
                Some(config.transport.proxy.url.clone()),
                config
                    .transport
                    .proxy
                    .token_file
                    .as_ref()
                    .map(|path| path.display().to_string()),
                config.transport.proxy.token_secret_id.clone(),
                if proxy_readiness.is_ok() {
                    "configured"
                } else {
                    "unconfigured"
                },
            )
            .with_message(proxy_readiness.err().map_or_else(
                || "Proxy transport delegates delivery to the configured endpoint".to_owned(),
                |error| error.to_string(),
            ))
        }
    }
}

fn profile_view_from_parts(
    profile_id: &str,
    nostr_relays: Vec<String>,
    reticulum_preview_behavior: Option<String>,
    proxy_url: Option<String>,
    proxy_token_file: Option<String>,
    proxy_token_secret_id: Option<String>,
    configured_state: &str,
) -> TransportProfileView {
    let transport_statuses = transport_statuses_from_parts(
        profile_id,
        nostr_relays.as_slice(),
        proxy_url.as_deref(),
        configured_state,
    );
    let profile_delivery_usable = transport_statuses
        .iter()
        .any(|status| status.usable_for_delivery);
    let message = match profile_id {
        "nostr" if profile_delivery_usable => "Nostr relay transport is configured for delivery",
        "nostr" => "Nostr transport requires configured Nostr relay targets",
        "reticulum_preview" => RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE,
        "hybrid" if profile_delivery_usable => {
            "Hybrid transport publishes through configured Nostr relays and reports Reticulum preview status"
        }
        "hybrid" => "Hybrid transport requires configured Nostr relay targets",
        "proxy" if profile_delivery_usable => {
            "Proxy transport delegates delivery to the configured endpoint"
        }
        "proxy" => "Proxy transport requires a configured token file or token secret id",
        _ => "Local-only profile does not deliver to network transports",
    };
    let proxy_token_source = match (
        proxy_token_file.as_ref().filter(|value| !value.is_empty()),
        proxy_token_secret_id
            .as_ref()
            .filter(|value| !value.is_empty()),
    ) {
        (Some(_), None) => Some("token_file".to_owned()),
        (None, Some(_)) => Some("token_secret_id".to_owned()),
        _ => None,
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
        reticulum_preview_behavior,
        proxy_url,
        proxy_token_source,
        proxy_token_file,
        proxy_token_secret_id,
        transport_statuses,
        actions: profile_actions(profile_id, profile_delivery_usable),
    }
}

fn transport_statuses_from_parts(
    profile_id: &str,
    nostr_relays: &[String],
    proxy_url: Option<&str>,
    configured_state: &str,
) -> Vec<TransportRuntimeStatusView> {
    match profile_id {
        "nostr" => vec![transport_runtime_status_view(nostr_transport_status(
            profile_id,
            !nostr_relays.is_empty(),
        ))],
        "reticulum_preview" => {
            vec![transport_runtime_status_view(
                reticulum_preview_transport_status(profile_id),
            )]
        }
        "hybrid" => vec![
            transport_runtime_status_view(nostr_transport_status(
                profile_id,
                !nostr_relays.is_empty(),
            )),
            transport_runtime_status_view(reticulum_preview_transport_status(profile_id)),
        ],
        "proxy" => vec![transport_runtime_status_view(proxy_transport_status(
            profile_id,
            proxy_url,
            configured_state == "configured",
        ))],
        _ => vec![transport_runtime_status_view(local_transport_status(
            profile_id,
        ))],
    }
}

fn local_transport_status(profile_id: &str) -> RadrootsTransportStatus {
    RadrootsTransportStatus::new(
        RadrootsTransportKind::Local,
        RadrootsTransportImplementationState::Available,
        RadrootsTransportReadinessState::Ready,
    )
    .with_profile_id(profile_id)
    .with_redacted_message("Local-only profile writes only to local state")
}

fn nostr_transport_status(profile_id: &str, targets_configured: bool) -> RadrootsTransportStatus {
    RadrootsTransportStatus::new(
        RadrootsTransportKind::Nostr,
        if targets_configured {
            RadrootsTransportImplementationState::Available
        } else {
            RadrootsTransportImplementationState::Misconfigured
        },
        if targets_configured {
            RadrootsTransportReadinessState::Ready
        } else {
            RadrootsTransportReadinessState::Misconfigured
        },
    )
    .with_profile_id(profile_id)
    .with_publish_usable(targets_configured)
    .with_fetch_usable(targets_configured)
}

fn reticulum_preview_transport_status(profile_id: &str) -> RadrootsTransportStatus {
    RadrootsTransportStatus::new(
        RadrootsTransportKind::Reticulum,
        RadrootsTransportImplementationState::PreviewUnavailable,
        RadrootsTransportReadinessState::PreviewUnavailable,
    )
    .with_profile_id(profile_id)
    .with_endpoint_uri(RADROOTS_RETICULUM_PREVIEW_ENDPOINT_URI)
    .with_redacted_message(RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE)
}

fn proxy_transport_status(
    profile_id: &str,
    proxy_url: Option<&str>,
    token_ready: bool,
) -> RadrootsTransportStatus {
    let mut status = RadrootsTransportStatus::new(
        RadrootsTransportKind::Proxy,
        if token_ready {
            RadrootsTransportImplementationState::Available
        } else {
            RadrootsTransportImplementationState::Misconfigured
        },
        if token_ready {
            RadrootsTransportReadinessState::Ready
        } else {
            RadrootsTransportReadinessState::Misconfigured
        },
    )
    .with_profile_id(profile_id)
    .with_publish_usable(token_ready);
    if let Some(proxy_url) = proxy_url {
        status = status.with_endpoint_uri(proxy_url);
    }
    status
}

fn transport_runtime_status_view(status: RadrootsTransportStatus) -> TransportRuntimeStatusView {
    let configured = transport_status_configured(&status);
    let usable_for_delivery = status.publish_usable;
    let message = transport_status_message(&status);
    TransportRuntimeStatusView {
        transport_kind: status.kind.canonical_label(),
        profile_id: status.profile_id,
        endpoint_uri: status.endpoint_uri,
        configured,
        implementation: transport_implementation_state_label(status.implementation_state)
            .to_owned(),
        usable_for_delivery,
        message,
    }
}

fn transport_status_configured(status: &RadrootsTransportStatus) -> bool {
    matches!(
        status.readiness,
        RadrootsTransportReadinessState::Ready
            | RadrootsTransportReadinessState::PreviewUnavailable
    )
}

fn transport_status_message(status: &RadrootsTransportStatus) -> String {
    if let Some(message) = &status.redacted_message {
        return message.clone();
    }
    match status.kind {
        RadrootsTransportKind::Local => "Local-only profile writes only to local state".to_owned(),
        RadrootsTransportKind::Nostr if status.publish_usable => {
            "Nostr relay transport is configured for delivery".to_owned()
        }
        RadrootsTransportKind::Nostr => {
            "Nostr transport requires configured Nostr relay targets".to_owned()
        }
        RadrootsTransportKind::Reticulum => RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE.to_owned(),
        RadrootsTransportKind::Mesh => "Mesh transport status is not available".to_owned(),
        RadrootsTransportKind::Proxy if status.publish_usable => {
            "Proxy transport delegates delivery to the configured endpoint".to_owned()
        }
        RadrootsTransportKind::Proxy => {
            "Proxy transport requires a configured token file or token secret id".to_owned()
        }
        RadrootsTransportKind::Custom(_) => "Custom transport status is not available".to_owned(),
    }
}

fn transport_implementation_state_label(
    state: RadrootsTransportImplementationState,
) -> &'static str {
    match state {
        RadrootsTransportImplementationState::Available => "available",
        RadrootsTransportImplementationState::Disabled => "disabled",
        RadrootsTransportImplementationState::Misconfigured => "misconfigured",
        RadrootsTransportImplementationState::PreviewUnavailable => "preview_unavailable",
    }
}

fn profile_actions(profile_id: &str, profile_delivery_usable: bool) -> Vec<String> {
    if profile_delivery_usable {
        return Vec::new();
    }
    match profile_id {
        "reticulum_preview" => vec![
            "radroots mesh status".to_owned(),
            "radroots transport profile get".to_owned(),
        ],
        "nostr" => {
            vec![
                "radroots transport profile set --kind nostr --nostr-relay wss://relay.example.com"
                    .to_owned(),
            ]
        }
        "hybrid" => {
            vec![
                "radroots transport profile set --kind hybrid --nostr-relay wss://relay.example.com"
                    .to_owned(),
            ]
        }
        "proxy" => vec![
            "radroots transport profile set --kind proxy --proxy-url http://127.0.0.1:7070 --proxy-token-file <path>"
                .to_owned(),
        ],
        _ => vec!["radroots transport profile get".to_owned()],
    }
}

fn validate_proxy_token_source(
    token_file: Option<&str>,
    token_secret_id: Option<&str>,
) -> Result<(), RuntimeError> {
    match (token_file, token_secret_id) {
        (None, None) => Err(RuntimeError::Config(
            "transport profile `proxy` requires --proxy-token-file or --proxy-token-secret-id"
                .to_owned(),
        )),
        (Some(file), None) if file.trim().is_empty() => Err(RuntimeError::Config(
            "transport profile `proxy` requires a non-empty --proxy-token-file".to_owned(),
        )),
        (None, Some(secret_id)) if secret_id.trim().is_empty() => Err(RuntimeError::Config(
            "transport profile `proxy` requires a non-empty --proxy-token-secret-id".to_owned(),
        )),
        (Some(_), Some(_)) => Err(RuntimeError::Config(
            "transport profile `proxy` cannot set both --proxy-token-file and --proxy-token-secret-id"
                .to_owned(),
        )),
        _ => Ok(()),
    }
}

pub fn proxy_token_ready(config: &RuntimeConfig) -> Result<(), RuntimeError> {
    crate::runtime::sdk::validate_proxy_bearer_token(config)
}

fn validate_proxy_token_material(
    config: &RuntimeConfig,
    url: &str,
    token_file: Option<&str>,
    token_secret_id: Option<&str>,
) -> Result<(), RuntimeError> {
    let mut validation_config = config.clone();
    validation_config.transport.profile = TransportProfileKind::Proxy;
    validation_config.transport.proxy.url = url.to_owned();
    validation_config.transport.proxy.token_file = token_file.map(PathBuf::from);
    validation_config.transport.proxy.token_secret_id = token_secret_id.map(str::to_owned);
    proxy_token_ready(&validation_config)
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
    use radroots_events::ids::RadrootsEventId;
    use radroots_sdk::{PushOutboxEventReceipt, PushOutboxTargetReceipt};

    #[test]
    fn transport_outbox_push_reports_reticulum_preview_without_attempts() {
        let cases = [
            (
                PushOutboxEventState::PreviewUnavailable,
                PushOutboxTargetOutcomeKind::PreviewUnavailable,
                "preview_unavailable",
                "SDK outbox push reported Reticulum preview work as preview unavailable without network delivery",
            ),
            (
                PushOutboxEventState::DeferredUntilImplemented,
                PushOutboxTargetOutcomeKind::DeferredUntilImplemented,
                "deferred_until_implemented",
                "SDK outbox push reported Reticulum preview work as deferred until implemented without network delivery",
            ),
        ];

        for (final_state, outcome_kind, expected_state, expected_reason) in cases {
            let receipt = reticulum_preview_receipt(final_state, outcome_kind);

            assert_eq!(transport_outbox_push_state(&receipt, 0), expected_state);
            assert_eq!(
                transport_outbox_push_reason(&receipt).as_deref(),
                Some(expected_reason)
            );
        }
    }

    fn reticulum_preview_receipt(
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
                    endpoint_uri: RADROOTS_RETICULUM_PREVIEW_ENDPOINT_URI.to_owned(),
                    outcome_kind,
                    attempted: false,
                    message: Some(RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE.to_owned()),
                }],
            }],
        }
    }
}
