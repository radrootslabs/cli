use crate::domain::runtime::{
    IdentityPublicView, LocalSignerStatusView, SignerBindingStatusView, SignerStatusView,
    SignerWriteKindReadinessView,
};
use crate::runtime::accounts::{SHARED_ACCOUNT_STORE_SOURCE, empty_account_resolution_view};
use crate::runtime::config::{RuntimeConfig, SIGNER_REMOTE_NIP46_CAPABILITY, SignerBackend};
use radroots_events::kinds::{KIND_FARM, KIND_LISTING, KIND_PROFILE, KIND_TRADE_ORDER_REQUEST};
use radroots_nostr_accounts::prelude::RadrootsNostrAccountStatus;
use radroots_nostr_signer::prelude::{
    RadrootsNostrLocalSignerAvailability, RadrootsNostrLocalSignerCapability,
    RadrootsNostrSignerCapability,
};
use serde::{Deserialize, Serialize};

const SIGNER_BINDING_PROVIDER_RUNTIME_ID: &str = "myc";
const SIGNER_BINDING_MODEL: &str = "session_authorized_remote_signer";
const MYC_DEFERRED_REASON: &str = "signer mode `myc` is deferred; use signer mode `local`";

#[derive(Debug, Clone, Copy)]
struct CliWriteKind {
    command: &'static str,
    event_kind: u32,
}

#[derive(Debug, Clone)]
pub enum ActorWriteBindingError {
    Unconfigured(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorWriteSignerAuthority {
    pub provider_runtime_id: String,
    pub account_identity_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_signer_session_id: Option<String>,
}

pub fn resolve_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    match config.signer.backend {
        SignerBackend::Local => resolve_local_signer_status(config),
        SignerBackend::Myc => resolve_myc_signer_status(config),
    }
}

pub fn resolve_actor_write_authority(
    config: &RuntimeConfig,
    _actor_role: &str,
    _actor_pubkey: &str,
) -> Result<Option<ActorWriteSignerAuthority>, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Myc) {
        return Ok(None);
    }

    Err(ActorWriteBindingError::Unconfigured(
        MYC_DEFERRED_REASON.to_owned(),
    ))
}

fn resolve_local_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    let (account_resolution, resolved_account_id) =
        match crate::runtime::accounts::resolve_account_resolution(config) {
            Ok(resolution) => (
                crate::runtime::accounts::account_resolution_view(&resolution),
                resolution
                    .resolved_account
                    .as_ref()
                    .map(|account| account.record.account_id.to_string()),
            ),
            Err(error) => {
                let reason = error.to_string();
                return SignerStatusView {
                    mode: config.signer.backend.as_str().to_owned(),
                    state: "error".to_owned(),
                    source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
                    signer_account_id: None,
                    account_resolution: empty_account_resolution_view(),
                    reason: Some(reason.clone()),
                    binding: disabled_binding_status(),
                    write_kinds: local_write_kind_readiness(false, Some(reason)),
                    local: None,
                    myc: None,
                };
            }
        };
    let secret_backend = crate::runtime::accounts::secret_backend_status(config);
    if secret_backend.state == "unavailable" {
        let reason = secret_backend.reason.clone();
        return SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unavailable".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            signer_account_id: resolved_account_id.clone(),
            account_resolution: account_resolution.clone(),
            reason: reason.clone(),
            binding: disabled_binding_status(),
            write_kinds: local_write_kind_readiness(false, reason),
            local: None,
            myc: None,
        };
    }

    if secret_backend.state == "error" {
        let reason = secret_backend.reason.clone();
        return SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "error".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            signer_account_id: resolved_account_id.clone(),
            account_resolution: account_resolution.clone(),
            reason: reason.clone(),
            binding: disabled_binding_status(),
            write_kinds: local_write_kind_readiness(false, reason),
            local: None,
            myc: None,
        };
    }

    let backend = secret_backend
        .active_backend
        .unwrap_or_else(|| "unknown".to_owned());
    let used_fallback = secret_backend.used_fallback;

    match crate::runtime::accounts::resolved_account_signing_status(config) {
        Ok(RadrootsNostrAccountStatus::Ready { account }) => {
            let capability = RadrootsNostrSignerCapability::LocalAccount(
                RadrootsNostrLocalSignerCapability::new(
                    account.account_id.clone(),
                    account.public_identity.clone(),
                    RadrootsNostrLocalSignerAvailability::SecretBacked,
                ),
            );
            let local = capability
                .local_account()
                .expect("local signer capability")
                .clone();
            SignerStatusView {
                mode: config.signer.backend.as_str().to_owned(),
                state: "ready".to_owned(),
                source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
                signer_account_id: Some(local.account_id.to_string()),
                account_resolution: account_resolution.clone(),
                reason: None,
                binding: disabled_binding_status(),
                write_kinds: local_write_kind_readiness(true, None),
                local: Some(LocalSignerStatusView {
                    account_id: local.account_id.to_string(),
                    public_identity: IdentityPublicView::from_public_identity(
                        &local.public_identity,
                    ),
                    availability: local_availability(local.availability).to_owned(),
                    secret_backed: local.is_secret_backed(),
                    backend: backend.clone(),
                    used_fallback,
                }),
                myc: None,
            }
        }
        Ok(RadrootsNostrAccountStatus::PublicOnly { account }) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            signer_account_id: Some(account.account_id.to_string()),
            account_resolution: account_resolution.clone(),
            reason: Some(format!(
                "local account {} is present but not secret-backed",
                account.account_id
            )),
            binding: disabled_binding_status(),
            write_kinds: local_write_kind_readiness(
                false,
                Some(format!(
                    "local account {} is present but not secret-backed",
                    account.account_id
                )),
            ),
            local: Some(LocalSignerStatusView {
                account_id: account.account_id.to_string(),
                public_identity: IdentityPublicView::from_public_identity(&account.public_identity),
                availability: local_availability(RadrootsNostrLocalSignerAvailability::PublicOnly)
                    .to_owned(),
                secret_backed: false,
                backend: backend.clone(),
                used_fallback,
            }),
            myc: None,
        },
        Ok(RadrootsNostrAccountStatus::NotConfigured) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            signer_account_id: None,
            account_resolution: account_resolution.clone(),
            reason: crate::runtime::accounts::unresolved_account_reason(config).ok(),
            binding: disabled_binding_status(),
            write_kinds: local_write_kind_readiness(
                false,
                crate::runtime::accounts::unresolved_account_reason(config).ok(),
            ),
            local: None,
            myc: None,
        },
        Err(error) => {
            let reason = error.to_string();
            SignerStatusView {
                mode: config.signer.backend.as_str().to_owned(),
                state: "error".to_owned(),
                source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
                signer_account_id: resolved_account_id,
                account_resolution,
                reason: Some(reason.clone()),
                binding: disabled_binding_status(),
                write_kinds: local_write_kind_readiness(false, Some(reason)),
                local: None,
                myc: None,
            }
        }
    }
}

fn resolve_myc_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    let account_resolution = match crate::runtime::accounts::resolve_account_resolution(config) {
        Ok(resolution) => crate::runtime::accounts::account_resolution_view(&resolution),
        Err(_) => empty_account_resolution_view(),
    };
    SignerStatusView {
        mode: config.signer.backend.as_str().to_owned(),
        state: "unconfigured".to_owned(),
        source: "target cli signer mode contract".to_owned(),
        signer_account_id: None,
        account_resolution,
        reason: Some(MYC_DEFERRED_REASON.to_owned()),
        binding: deferred_myc_binding_status(),
        write_kinds: deferred_write_kind_readiness(),
        local: None,
        myc: None,
    }
}

fn disabled_binding_status() -> SignerBindingStatusView {
    SignerBindingStatusView {
        capability_id: SIGNER_REMOTE_NIP46_CAPABILITY.to_owned(),
        provider_runtime_id: SIGNER_BINDING_PROVIDER_RUNTIME_ID.to_owned(),
        binding_model: SIGNER_BINDING_MODEL.to_owned(),
        state: "disabled".to_owned(),
        source: "independent local signer mode".to_owned(),
        target_kind: None,
        target: None,
        managed_account_ref: None,
        signer_session_ref: None,
        resolved_signer_session_id: None,
        matched_session_count: None,
        reason: Some(
            "remote myc signer binding is disabled while cli signer mode is `local`".to_owned(),
        ),
    }
}

fn deferred_myc_binding_status() -> SignerBindingStatusView {
    SignerBindingStatusView {
        capability_id: SIGNER_REMOTE_NIP46_CAPABILITY.to_owned(),
        provider_runtime_id: SIGNER_BINDING_PROVIDER_RUNTIME_ID.to_owned(),
        binding_model: SIGNER_BINDING_MODEL.to_owned(),
        state: "deferred".to_owned(),
        source: "target cli signer mode contract".to_owned(),
        target_kind: None,
        target: None,
        managed_account_ref: None,
        signer_session_ref: None,
        resolved_signer_session_id: None,
        matched_session_count: None,
        reason: Some(MYC_DEFERRED_REASON.to_owned()),
    }
}

fn cli_write_kinds() -> [CliWriteKind; 4] {
    [
        CliWriteKind {
            command: "farm profile publish",
            event_kind: KIND_PROFILE,
        },
        CliWriteKind {
            command: "farm publish",
            event_kind: KIND_FARM,
        },
        CliWriteKind {
            command: "listing publish",
            event_kind: KIND_LISTING,
        },
        CliWriteKind {
            command: "order submit",
            event_kind: KIND_TRADE_ORDER_REQUEST,
        },
    ]
}

fn local_write_kind_readiness(
    ready: bool,
    reason: Option<String>,
) -> Vec<SignerWriteKindReadinessView> {
    cli_write_kinds()
        .iter()
        .map(|kind| SignerWriteKindReadinessView {
            command: kind.command.to_owned(),
            event_kind: kind.event_kind,
            permission: "local_account_secret".to_owned(),
            ready,
            reason: if ready { None } else { reason.clone() },
        })
        .collect()
}

fn deferred_write_kind_readiness() -> Vec<SignerWriteKindReadinessView> {
    cli_write_kinds()
        .iter()
        .map(|kind| SignerWriteKindReadinessView {
            command: kind.command.to_owned(),
            event_kind: kind.event_kind,
            permission: "signer_mode_local_required".to_owned(),
            ready: false,
            reason: Some(MYC_DEFERRED_REASON.to_owned()),
        })
        .collect()
}

fn local_availability(value: RadrootsNostrLocalSignerAvailability) -> &'static str {
    match value {
        RadrootsNostrLocalSignerAvailability::PublicOnly => "public_only",
        RadrootsNostrLocalSignerAvailability::SecretBacked => "secret_backed",
    }
}

#[cfg(test)]
mod tests {
    use radroots_events::kinds::KIND_TRADE_DISCOUNT_DECLINE;

    use super::{KIND_TRADE_ORDER_REQUEST, cli_write_kinds};

    #[test]
    fn order_submit_readiness_uses_active_order_request_kind() {
        let write_kind = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order submit")
            .expect("order submit readiness");

        assert_eq!(write_kind.event_kind, KIND_TRADE_ORDER_REQUEST);
        assert_ne!(write_kind.event_kind, KIND_TRADE_DISCOUNT_DECLINE);
    }
}
