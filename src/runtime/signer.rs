use crate::runtime::RuntimeError;
use crate::runtime::account::AccountRuntimeFailure;
use crate::runtime::account::{SHARED_ACCOUNT_STORE_SOURCE, empty_account_resolution_view};
use crate::runtime::config::{
    CapabilityBindingTargetKind, RuntimeConfig, SIGNER_REMOTE_NIP46_CAPABILITY, SignerBackend,
};
use crate::runtime::sdk::MYC_NIP46_SESSION_SECRET_SERVICE;
use crate::view::runtime::{
    IdentityPublicView, LocalSignerStatusView, MycStatusView, SignerBindingStatusView,
    SignerStatusView, SignerWriteKindReadinessView,
};
use radroots_events::kinds::{
    KIND_FARM, KIND_LISTING, KIND_ORDER_CANCELLATION, KIND_ORDER_DECISION, KIND_ORDER_REQUEST,
    KIND_ORDER_REVISION_DECISION, KIND_ORDER_REVISION_PROPOSAL, KIND_PROFILE,
};
use radroots_nostr_accounts::prelude::RadrootsNostrAccountStatus;
use radroots_nostr_signer::prelude::{
    RadrootsNostrLocalSignerAvailability, RadrootsNostrLocalSignerCapability,
    RadrootsNostrSignerCapability,
};
use radroots_sdk::radroots_sdk_myc_nip46_product_permission_strings;
use url::Url;

const SIGNER_BINDING_PROVIDER_RUNTIME_ID: &str = "myc";
const SIGNER_BINDING_MODEL: &str = "session_authorized_remote_signer";

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

pub fn resolve_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    match config.signer.backend {
        SignerBackend::Local => resolve_local_signer_status(config),
        SignerBackend::Myc => resolve_myc_signer_status(config),
    }
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
    let readiness = myc_binding_readiness(config, None);
    SignerStatusView {
        mode: config.signer.backend.as_str().to_owned(),
        state: if readiness.ready {
            "ready"
        } else {
            "unconfigured"
        }
        .to_owned(),
        source: readiness.source.clone(),
        signer_account_id: None,
        account_resolution,
        reason: readiness.reason.clone(),
        binding: readiness.binding,
        write_kinds: myc_write_kind_readiness(readiness.ready, readiness.reason.clone()),
        local: None,
        myc: Some(MycStatusView {
            executable: config.myc.executable.display().to_string(),
            state: if readiness.ready {
                "ready"
            } else {
                "unconfigured"
            }
            .to_owned(),
            source: readiness.source,
            service_status: None,
            ready: readiness.ready,
            reason: readiness.reason,
            reasons: readiness.reasons,
            remote_session_count: usize::from(readiness.signer_session_ref.is_some()),
            local_signer: None,
            remote_sessions: Vec::new(),
            custody: None,
        }),
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
        resolved_session_ref: None,
        matched_session_count: None,
        reason: Some(
            "remote myc signer binding is disabled while cli signer mode is `local`".to_owned(),
        ),
    }
}

fn cli_write_kinds() -> [CliWriteKind; 12] {
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

fn local_availability(value: RadrootsNostrLocalSignerAvailability) -> &'static str {
    match value {
        RadrootsNostrLocalSignerAvailability::PublicOnly => "public_only",
        RadrootsNostrLocalSignerAvailability::SecretBacked => "secret_backed",
    }
}

#[derive(Debug, Clone)]
struct MycBindingReadiness {
    binding: SignerBindingStatusView,
    ready: bool,
    source: String,
    reason: Option<String>,
    reasons: Vec<String>,
    signer_session_ref: Option<String>,
}

fn myc_binding_readiness(
    config: &RuntimeConfig,
    actor_pubkey: Option<&str>,
) -> MycBindingReadiness {
    let Some(binding) = config.capability_binding(SIGNER_REMOTE_NIP46_CAPABILITY) else {
        let reason = "signer.remote_nip46 binding is missing".to_owned();
        return MycBindingReadiness {
            binding: missing_myc_binding_status(reason.clone()),
            ready: false,
            source: "no explicit capability binding".to_owned(),
            reason: Some(reason.clone()),
            reasons: vec![reason],
            signer_session_ref: None,
        };
    };

    let mut reasons = Vec::new();
    if binding.target_kind != CapabilityBindingTargetKind::ExplicitEndpoint {
        reasons.push(format!(
            "signer.remote_nip46 binding target_kind `{}` is not supported for CLI Myc signing; use `explicit_endpoint`",
            binding.target_kind.as_str()
        ));
    }
    if let Err(reason) = validate_myc_target(binding.target.as_str()) {
        reasons.push(reason);
    }
    if let (Some(managed_account_ref), Some(actor_pubkey)) =
        (binding.managed_account_ref.as_deref(), actor_pubkey)
    {
        if managed_account_ref != actor_pubkey {
            reasons.push(format!(
                "signer.remote_nip46 managed_account_ref `{managed_account_ref}` does not match actor pubkey `{actor_pubkey}`"
            ));
        }
    }
    let signer_session_ref = binding.signer_session_ref.clone();
    if let Some(session_ref) = signer_session_ref.as_deref() {
        match crate::runtime::account::load_secret_backend_secret(
            config,
            session_ref,
            MYC_NIP46_SESSION_SECRET_SERVICE,
        ) {
            Ok(Some(secret)) if secret.trim().is_empty() => {
                reasons.push(format!(
                    "signer.remote_nip46 signer_session_ref `{session_ref}` resolved to an empty client secret"
                ));
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                reasons.push(format!(
                    "signer.remote_nip46 signer_session_ref `{session_ref}` was not found in the account secret backend"
                ));
            }
            Err(error) => reasons.push(error.to_string()),
        }
    } else {
        reasons.push("signer.remote_nip46 signer_session_ref is missing".to_owned());
    }

    let ready = reasons.is_empty();
    let reason = reasons.first().cloned();
    let source = binding.source.as_str().to_owned();
    MycBindingReadiness {
        binding: SignerBindingStatusView {
            capability_id: binding.capability_id.clone(),
            provider_runtime_id: binding.provider_runtime_id.clone(),
            binding_model: binding.binding_model.clone(),
            state: if ready { "ready" } else { "unconfigured" }.to_owned(),
            source: source.clone(),
            target_kind: Some(binding.target_kind.as_str().to_owned()),
            target: Some(binding.target.clone()),
            managed_account_ref: binding.managed_account_ref.clone(),
            signer_session_ref: binding.signer_session_ref.clone(),
            resolved_session_ref: binding.signer_session_ref.clone().filter(|_| ready),
            matched_session_count: Some(usize::from(ready)),
            reason: reason.clone(),
        },
        ready,
        source,
        reason,
        reasons,
        signer_session_ref,
    }
}

fn missing_myc_binding_status(reason: String) -> SignerBindingStatusView {
    SignerBindingStatusView {
        capability_id: SIGNER_REMOTE_NIP46_CAPABILITY.to_owned(),
        provider_runtime_id: SIGNER_BINDING_PROVIDER_RUNTIME_ID.to_owned(),
        binding_model: SIGNER_BINDING_MODEL.to_owned(),
        state: "unconfigured".to_owned(),
        source: "no explicit capability binding".to_owned(),
        target_kind: None,
        target: None,
        managed_account_ref: None,
        signer_session_ref: None,
        resolved_session_ref: None,
        matched_session_count: Some(0),
        reason: Some(reason),
    }
}

fn validate_myc_target(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.starts_with("nostrconnect://") {
        return Err(
            "signer.remote_nip46 target must be a bunker URI or discovery URL; raw nostrconnect client URIs are signer-side only"
                .to_owned(),
        );
    }
    let bunker_uri = if trimmed.starts_with("bunker://") {
        trimmed.to_owned()
    } else {
        let url = Url::parse(trimmed)
            .map_err(|error| format!("signer.remote_nip46 target is invalid: {error}"))?;
        url.query_pairs()
            .find(|(key, _)| key == "uri")
            .map(|(_, uri)| uri.into_owned())
            .ok_or_else(|| {
                "signer.remote_nip46 discovery target is missing `uri` query parameter".to_owned()
            })?
    };
    match radroots_nostr_connect::prelude::RadrootsNostrConnectUri::parse(bunker_uri.as_str())
        .map_err(|error| format!("signer.remote_nip46 target is invalid: {error}"))?
    {
        radroots_nostr_connect::prelude::RadrootsNostrConnectUri::Bunker(_) => Ok(()),
        radroots_nostr_connect::prelude::RadrootsNostrConnectUri::Client(_) => Err(
            "signer.remote_nip46 target must resolve to a bunker URI; raw nostrconnect client URIs are signer-side only"
                .to_owned(),
        ),
    }
}

fn myc_write_kind_readiness(
    ready: bool,
    reason: Option<String>,
) -> Vec<SignerWriteKindReadinessView> {
    let permissions = radroots_sdk_myc_nip46_product_permission_strings();
    cli_write_kinds()
        .iter()
        .map(|kind| {
            let event_kind = kind.event_kind.to_string();
            let permission = permissions
                .iter()
                .find(|permission| permission.contains(event_kind.as_str()))
                .cloned()
                .unwrap_or_else(|| format!("sign_event:{event_kind}"));
            let permission_ready = ready
                && permissions
                    .iter()
                    .any(|permission| permission.contains(event_kind.as_str()));
            SignerWriteKindReadinessView {
                command: kind.command.to_owned(),
                event_kind: kind.event_kind,
                permission,
                ready: permission_ready,
                reason: if permission_ready {
                    None
                } else {
                    reason.clone().or_else(|| {
                        Some(
                            "SDK Myc signer permission is not configured for this event kind"
                                .to_owned(),
                        )
                    })
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        KIND_ORDER_CANCELLATION, KIND_ORDER_DECISION, KIND_ORDER_REQUEST,
        KIND_ORDER_REVISION_DECISION, KIND_ORDER_REVISION_PROPOSAL, cli_write_kinds,
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
    fn order_cancel_readiness_uses_order_cancellation_kind() {
        let cancel = cli_write_kinds()
            .into_iter()
            .find(|kind| kind.command == "order.cancel")
            .expect("order cancel readiness");
        assert_eq!(cancel.event_kind, KIND_ORDER_CANCELLATION);
        assert_ne!(cancel.event_kind, RESERVED_ORDER_KIND_3431);
    }
}
