use std::fs;
use std::path::PathBuf;

use radroots_sdk::{PushOutboxRequest, SyncStatusRequest};
use radroots_transport::RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE;
use serde_json::Value as JsonValue;
use toml::{Value, map::Map};

use crate::ops::OperationData;
use crate::runtime::RuntimeError;
use crate::runtime::config::{RuntimeConfig, TransportProfileKind};
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession, sdk_nostr_relay_url_policy};
use crate::view::runtime::{
    TransportOutboxPushView, TransportOutboxStatusView, TransportProfileView, TransportStatusView,
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
    let mut transports = vec![active_profile_view(config)];
    if config.transport.profile != TransportProfileKind::ReticulumPreview {
        transports.push(profile_view_from_parts(
            "reticulum_preview",
            Vec::new(),
            Some("reject_delivery_attempts".to_owned()),
            None,
            None,
            None,
            "preview_unavailable",
        ));
    }

    TransportStatusView {
        state: "ready".to_owned(),
        source: TRANSPORT_SOURCE.to_owned(),
        transports,
    }
}

pub fn outbox_status(
    config: &RuntimeConfig,
) -> Result<TransportOutboxStatusView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let receipt = session.block_on(session.sdk().sync().status(SyncStatusRequest::new()))?;
    Ok(TransportOutboxStatusView {
        state: "ready".to_owned(),
        source: "SDK transport outbox".to_owned(),
        transport_profile: receipt.transport_profile.transport_profile_id,
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
    let state = if receipt.attempted_events == 0 {
        "ready"
    } else if receipt.published_events > 0 && failed_count > 0 {
        "partial"
    } else if failed_count > 0 {
        "unavailable"
    } else if receipt.published_events > 0 {
        "published"
    } else {
        "ready"
    }
    .to_owned();
    Ok(TransportOutboxPushView {
        state,
        source: "SDK transport outbox".to_owned(),
        attempted_events: receipt.attempted_events,
        published_events: receipt.published_events,
        retryable_events: receipt.retryable_events,
        terminal_events: receipt.terminal_events,
        target_count,
        reason: (receipt.attempted_events == 0)
            .then_some("SDK outbox had no ready signed events to push".to_owned()),
        actions: vec!["radroots transport outbox status".to_owned()],
    })
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
    let transport_kind = match profile_id {
        "nostr" => "nostr",
        "reticulum_preview" => "reticulum",
        "proxy" => "proxy",
        _ => "local",
    };
    let implementation_state = match profile_id {
        "nostr" => "available",
        "reticulum_preview" => "preview_unavailable",
        "proxy" => "delegated",
        _ => "local_only",
    };
    let usable_for_delivery =
        matches!(profile_id, "nostr" | "proxy") && configured_state == "configured";
    let message = match profile_id {
        "nostr" if usable_for_delivery => "Nostr relay transport is configured for delivery",
        "nostr" => "Nostr transport requires configured Nostr relay targets",
        "reticulum_preview" => RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE,
        "proxy" if usable_for_delivery => {
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
        transport_kind: transport_kind.to_owned(),
        configured_state: configured_state.to_owned(),
        implementation_state: implementation_state.to_owned(),
        usable_for_delivery,
        message: message.to_owned(),
        nostr_relays,
        reticulum_preview_behavior,
        proxy_url,
        proxy_token_source,
        proxy_token_file,
        proxy_token_secret_id,
        actions: profile_actions(profile_id, usable_for_delivery),
    }
}

fn profile_actions(profile_id: &str, usable_for_delivery: bool) -> Vec<String> {
    if usable_for_delivery {
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
}

impl TransportProfileViewMessage for TransportProfileView {
    fn with_message(mut self, message: String) -> Self {
        self.message = message;
        self
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
