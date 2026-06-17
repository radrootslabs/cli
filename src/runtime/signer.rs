use crate::runtime::RuntimeError;
use crate::runtime::account::AccountRuntimeFailure;
use crate::runtime::account::{SHARED_ACCOUNT_STORE_SOURCE, empty_account_resolution_view};
use crate::runtime::config::{RuntimeConfig, SIGNER_REMOTE_NIP46_CAPABILITY, SignerBackend};
use crate::view::runtime::{
    IdentityPublicView, LocalSignerStatusView, SignerBindingStatusView, SignerStatusView,
    SignerWriteKindReadinessView,
};
use radroots_events::kinds::{
    KIND_FARM, KIND_LISTING, KIND_ORDER_CANCELLATION, KIND_ORDER_DECISION,
    KIND_ORDER_FULFILLMENT_UPDATE, KIND_ORDER_RECEIPT, KIND_ORDER_REQUEST,
    KIND_ORDER_REVISION_DECISION, KIND_ORDER_REVISION_PROPOSAL, KIND_PROFILE,
};
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
    Account(AccountRuntimeFailure),
}

impl ActorWriteBindingError {
    pub fn from_runtime(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Account(failure) => Self::Account(failure),
            other => Self::Unconfigured(other.to_string()),
        }
    }

    pub fn reason(self) -> String {
        match self {
            Self::Unconfigured(reason) => reason,
            Self::Account(failure) => failure.to_string(),
        }
    }
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
        match crate::runtime::account::resolve_account_resolution(config) {
            Ok(resolution) => (
                crate::runtime::account::account_resolution_view(&resolution),
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
    let secret_backend = crate::runtime::account::secret_backend_status(config);
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

    match crate::runtime::account::resolved_account_signing_status(config) {
        Ok(RadrootsNostrAccountStatus::Ready { account }) => {
            let capability = RadrootsNostrSignerCapability::LocalAccount(Box::new(
                RadrootsNostrLocalSignerCapability::new(
                    account.account_id.clone(),
                    account.public_identity.clone(),
                    RadrootsNostrLocalSignerAvailability::SecretBacked,
                ),
            ));
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
        Ok(RadrootsNostrAccountStatus::PublicOnly { account }) => {
            let reason = AccountRuntimeFailure::watch_only(&account.account_id).to_string();
            SignerStatusView {
                mode: config.signer.backend.as_str().to_owned(),
                state: "unconfigured".to_owned(),
                source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
                signer_account_id: Some(account.account_id.to_string()),
                account_resolution: account_resolution.clone(),
                reason: Some(reason.clone()),
                binding: disabled_binding_status(),
                write_kinds: local_write_kind_readiness(false, Some(reason)),
                local: Some(LocalSignerStatusView {
                    account_id: account.account_id.to_string(),
                    public_identity: IdentityPublicView::from_public_identity(
                        &account.public_identity,
                    ),
                    availability: local_availability(
                        RadrootsNostrLocalSignerAvailability::PublicOnly,
                    )
                    .to_owned(),
                    secret_backed: false,
                    backend: backend.clone(),
                    used_fallback,
                }),
                myc: None,
            }
        }
        Ok(RadrootsNostrAccountStatus::NotConfigured) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            signer_account_id: None,
            account_resolution: account_resolution.clone(),
            reason: crate::runtime::account::unresolved_account_reason(config).ok(),
            binding: disabled_binding_status(),
            write_kinds: local_write_kind_readiness(
                false,
                crate::runtime::account::unresolved_account_reason(config).ok(),
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
    let account_resolution = match crate::runtime::account::resolve_account_resolution(config) {
        Ok(resolution) => crate::runtime::account::account_resolution_view(&resolution),
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

fn cli_write_kinds() -> [CliWriteKind; 14] {
    [
        CliWriteKind {
            command: "sync.push",
            event_kind: KIND_PROFILE,
        },
        CliWriteKind {
            command: "farm.publish",
            event_kind: KIND_FARM,
        },
        CliWriteKind {
            command: "listing.publish",
            event_kind: KIND_LISTING,
        },
        CliWriteKind {
            command: "listing.update",
            event_kind: KIND_LISTING,
        },
        CliWriteKind {
            command: "listing.archive",
            event_kind: KIND_LISTING,
        },
        CliWriteKind {
            command: "order.submit",
            event_kind: KIND_ORDER_REQUEST,
        },
        CliWriteKind {
            command: "order.accept",
            event_kind: KIND_ORDER_DECISION,
        },
        CliWriteKind {
            command: "order.decline",
            event_kind: KIND_ORDER_DECISION,
        },
        CliWriteKind {
            command: "order.cancel",
            event_kind: KIND_ORDER_CANCELLATION,
        },
        CliWriteKind {
            command: "order.revision.propose",
            event_kind: KIND_ORDER_REVISION_PROPOSAL,
        },
        CliWriteKind {
            command: "order.revision.accept",
            event_kind: KIND_ORDER_REVISION_DECISION,
        },
        CliWriteKind {
            command: "order.revision.decline",
            event_kind: KIND_ORDER_REVISION_DECISION,
        },
        CliWriteKind {
            command: "order.fulfillment.update",
            event_kind: KIND_ORDER_FULFILLMENT_UPDATE,
        },
        CliWriteKind {
            command: "order.receipt.record",
            event_kind: KIND_ORDER_RECEIPT,
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
    use super::{
        KIND_ORDER_CANCELLATION, KIND_ORDER_DECISION, KIND_ORDER_FULFILLMENT_UPDATE,
        KIND_ORDER_RECEIPT, KIND_ORDER_REQUEST, KIND_ORDER_REVISION_DECISION,
        KIND_ORDER_REVISION_PROPOSAL, cli_write_kinds,
    };

    const RESERVED_ORDER_KIND_3431: u32 = 3431;

    #[test]
    fn write_kind_readiness_matches_active_signed_mutations() {
        let commands: Vec<&str> = cli_write_kinds()
            .iter()
            .map(|write_kind| write_kind.command)
            .collect();

        assert_eq!(
            commands,
            [
                "sync.push",
                "farm.publish",
                "listing.publish",
                "listing.update",
                "listing.archive",
                "order.submit",
                "order.accept",
                "order.decline",
                "order.cancel",
                "order.revision.propose",
                "order.revision.accept",
                "order.revision.decline",
                "order.fulfillment.update",
                "order.receipt.record",
            ]
        );
        assert!(!commands.contains(&"signer.status.get"));
    }

    #[test]
    fn order_submit_readiness_uses_active_order_request_kind() {
        let write_kind = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order.submit")
            .expect("order submit readiness");

        assert_eq!(write_kind.event_kind, KIND_ORDER_REQUEST);
        assert_ne!(write_kind.event_kind, RESERVED_ORDER_KIND_3431);
    }

    #[test]
    fn order_decision_readiness_uses_active_order_decision_kind() {
        for command in ["order.accept", "order.decline"] {
            let write_kind = cli_write_kinds()
                .into_iter()
                .find(|kind| kind.command == command)
                .expect("order decision readiness");

            assert_eq!(write_kind.event_kind, KIND_ORDER_DECISION);
            assert_ne!(write_kind.event_kind, RESERVED_ORDER_KIND_3431);
        }
    }

    #[test]
    fn order_revision_readiness_uses_active_revision_kinds() {
        let proposal = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order.revision.propose")
            .expect("order revision propose readiness");

        assert_eq!(proposal.event_kind, KIND_ORDER_REVISION_PROPOSAL);
        assert_ne!(proposal.event_kind, RESERVED_ORDER_KIND_3431);

        for command in ["order.revision.accept", "order.revision.decline"] {
            let write_kind = cli_write_kinds()
                .into_iter()
                .find(|kind| kind.command == command)
                .expect("order revision decision readiness");

            assert_eq!(write_kind.event_kind, KIND_ORDER_REVISION_DECISION);
            assert_ne!(write_kind.event_kind, RESERVED_ORDER_KIND_3431);
        }
    }

    #[test]
    fn order_follow_on_readiness_uses_order_kinds() {
        let cancel = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order.cancel")
            .expect("order cancel readiness");
        assert_eq!(cancel.event_kind, KIND_ORDER_CANCELLATION);
        assert_ne!(cancel.event_kind, RESERVED_ORDER_KIND_3431);

        let fulfillment = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order.fulfillment.update")
            .expect("order fulfillment readiness");
        assert_eq!(fulfillment.event_kind, KIND_ORDER_FULFILLMENT_UPDATE);
        assert_ne!(fulfillment.event_kind, RESERVED_ORDER_KIND_3431);

        let receipt = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order.receipt.record")
            .expect("order receipt readiness");
        assert_eq!(receipt.event_kind, KIND_ORDER_RECEIPT);
        assert_ne!(receipt.event_kind, RESERVED_ORDER_KIND_3431);
    }
}
