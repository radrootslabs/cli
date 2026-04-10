use crate::domain::runtime::{
    IdentityPublicView, LocalSignerStatusView, MycRemoteSessionView, MycStatusView,
    SignerBindingStatusView, SignerStatusView,
};
use crate::runtime::accounts::SHARED_ACCOUNT_STORE_SOURCE;
use crate::runtime::config::{
    CapabilityBindingConfig, CapabilityBindingTargetKind, RuntimeConfig,
    SIGNER_REMOTE_NIP46_CAPABILITY, SignerBackend,
};
use radroots_nostr_accounts::prelude::RadrootsNostrSelectedAccountStatus;
use radroots_nostr_signer::prelude::{
    RadrootsNostrLocalSignerAvailability, RadrootsNostrLocalSignerCapability,
    RadrootsNostrSignerCapability,
};
use serde::{Deserialize, Serialize};

const SIGNER_BINDING_PROVIDER_RUNTIME_ID: &str = "myc";
const SIGNER_BINDING_MODEL: &str = "session_authorized_remote_signer";

#[derive(Debug, Clone)]
struct MycBindingResolution {
    view: SignerBindingStatusView,
    resolved_account_id: Option<String>,
    resolved_signer_public_key_hex: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ActorWriteBindingError {
    Unconfigured(String),
    Unavailable(String),
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
    actor_role: &str,
    actor_pubkey: &str,
) -> Result<Option<ActorWriteSignerAuthority>, ActorWriteBindingError> {
    if !matches!(config.signer.backend, SignerBackend::Myc) {
        return Ok(None);
    }

    let myc = crate::runtime::myc::resolve_status(&config.myc);
    let resolution = resolve_myc_binding(config, &myc);
    match resolution.view.state.as_str() {
        "ready" => {}
        "unavailable" => {
            return Err(ActorWriteBindingError::Unavailable(
                resolution.view.reason.unwrap_or_else(|| {
                    "myc signer binding is unavailable for actor-authored writes".to_owned()
                }),
            ));
        }
        _ => {
            return Err(ActorWriteBindingError::Unconfigured(
                resolution.view.reason.unwrap_or_else(|| {
                    "myc signer binding is not ready for actor-authored writes".to_owned()
                }),
            ));
        }
    }

    let Some(resolved_signer_public_key_hex) = resolution.resolved_signer_public_key_hex else {
        return Err(ActorWriteBindingError::Unconfigured(
            "myc signer binding reported ready without a resolved signer identity".to_owned(),
        ));
    };

    if !resolved_signer_public_key_hex.eq_ignore_ascii_case(actor_pubkey) {
        return Err(ActorWriteBindingError::Unconfigured(format!(
            "configured myc signer binding resolves signer pubkey `{resolved_signer_public_key_hex}` instead of {actor_role} pubkey `{actor_pubkey}`"
        )));
    }

    let Some(resolved_account_id) = resolution.resolved_account_id else {
        return Err(ActorWriteBindingError::Unconfigured(
            "myc signer binding reported ready without a resolved account identity".to_owned(),
        ));
    };

    Ok(Some(ActorWriteSignerAuthority {
        provider_runtime_id: SIGNER_BINDING_PROVIDER_RUNTIME_ID.to_owned(),
        account_identity_id: resolved_account_id,
        provider_signer_session_id: resolution.view.resolved_signer_session_id.clone(),
    }))
}

fn resolve_local_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    let secret_backend = crate::runtime::accounts::secret_backend_status(config);
    if secret_backend.state == "unavailable" {
        return SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unavailable".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            account_id: None,
            reason: secret_backend.reason,
            binding: disabled_binding_status(),
            local: None,
            myc: None,
        };
    }

    if secret_backend.state == "error" {
        return SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "error".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            account_id: None,
            reason: secret_backend.reason,
            binding: disabled_binding_status(),
            local: None,
            myc: None,
        };
    }

    let backend = secret_backend
        .active_backend
        .unwrap_or_else(|| "unknown".to_owned());
    let used_fallback = secret_backend.used_fallback;

    match crate::runtime::accounts::selected_account_status(config) {
        Ok(RadrootsNostrSelectedAccountStatus::Ready { account }) => {
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
                account_id: Some(local.account_id.to_string()),
                reason: None,
                binding: disabled_binding_status(),
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
        Ok(RadrootsNostrSelectedAccountStatus::PublicOnly { account }) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            account_id: Some(account.account_id.to_string()),
            reason: Some(format!(
                "local account {} is present but not secret-backed",
                account.account_id
            )),
            binding: disabled_binding_status(),
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
        Ok(RadrootsNostrSelectedAccountStatus::NotConfigured) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "unconfigured".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            account_id: None,
            reason: Some(format!(
                "no local account is selected in {}",
                config.account.store_path.display()
            )),
            binding: disabled_binding_status(),
            local: None,
            myc: None,
        },
        Err(error) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "error".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            account_id: None,
            reason: Some(error.to_string()),
            binding: disabled_binding_status(),
            local: None,
            myc: None,
        },
    }
}

fn resolve_myc_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    let myc = crate::runtime::myc::resolve_status(&config.myc);
    let resolution = resolve_myc_binding(config, &myc);
    let binding = resolution.view;
    let state = myc_signer_state(&myc, &binding).to_owned();
    SignerStatusView {
        mode: config.signer.backend.as_str().to_owned(),
        state,
        source: if myc.state == "ready" {
            binding.source.clone()
        } else {
            myc.source.clone()
        },
        account_id: resolution.resolved_account_id,
        reason: if myc.state == "ready" {
            binding.reason.clone()
        } else {
            myc.reason.clone().or_else(|| binding.reason.clone())
        },
        binding,
        local: None,
        myc: Some(myc),
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

fn resolve_myc_binding(config: &RuntimeConfig, myc: &MycStatusView) -> MycBindingResolution {
    let Some(binding) = config.capability_binding(SIGNER_REMOTE_NIP46_CAPABILITY) else {
        return MycBindingResolution {
            view: SignerBindingStatusView {
                capability_id: SIGNER_REMOTE_NIP46_CAPABILITY.to_owned(),
                provider_runtime_id: SIGNER_BINDING_PROVIDER_RUNTIME_ID.to_owned(),
                binding_model: SIGNER_BINDING_MODEL.to_owned(),
                state: "unconfigured".to_owned(),
                source: "no explicit capability binding".to_owned(),
                target_kind: None,
                target: None,
                managed_account_ref: None,
                signer_session_ref: None,
                resolved_signer_session_id: None,
                matched_session_count: None,
                reason: Some(
                    "configure [[capability_binding]] for `signer.remote_nip46` before using `--signer myc`"
                        .to_owned(),
                ),
            },
            resolved_account_id: None,
            resolved_signer_public_key_hex: None,
        };
    };

    if !matches!(
        binding.target_kind,
        CapabilityBindingTargetKind::ManagedInstance
    ) {
        return binding_status(
            binding,
            "unsupported",
            None,
            None,
            None,
            format!(
                "signer.remote_nip46 only supports target_kind `managed_instance`; got `{}`",
                binding.target_kind.as_str()
            ),
        );
    }

    if binding.target != "default" {
        return binding_status(
            binding,
            "unsupported",
            None,
            None,
            None,
            format!(
                "managed myc target `{}` is not supported yet; use target `default`",
                binding.target
            ),
        );
    }

    match myc.state.as_str() {
        "ready" => {}
        "unconfigured" => {
            return binding_status(
                binding,
                "unconfigured",
                None,
                None,
                None,
                myc.reason.clone().unwrap_or_else(|| {
                    "myc is not configured for composed signer bindings".to_owned()
                }),
            );
        }
        _ => {
            return binding_status(
                binding,
                "unavailable",
                None,
                None,
                None,
                myc.reason
                    .clone()
                    .unwrap_or_else(|| "myc is not ready for remote signer bindings".to_owned()),
            );
        }
    }

    let signing_sessions = myc
        .remote_sessions
        .iter()
        .filter(|session| session_supports_signing(session))
        .collect::<Vec<_>>();

    if let Some(session_ref) = binding.signer_session_ref.as_deref() {
        let Some(session) = myc
            .remote_sessions
            .iter()
            .find(|session| session.connection_id == session_ref)
        else {
            return binding_status(
                binding,
                "unavailable",
                None,
                Some(0),
                None,
                format!("configured signer session `{session_ref}` is not currently available"),
            );
        };

        if !session_supports_signing(session) {
            return binding_status(
                binding,
                "unauthorized",
                None,
                Some(1),
                None,
                format!(
                    "configured signer session `{session_ref}` is not approved for `sign_event`"
                ),
            );
        }

        if let Some(account_ref) = binding.managed_account_ref.as_deref() {
            if session.signer_identity.id != account_ref {
                return binding_status(
                    binding,
                    "unauthorized",
                    None,
                    Some(1),
                    None,
                    format!(
                        "configured signer session `{session_ref}` resolves signer `{}` instead of managed account `{account_ref}`",
                        session.signer_identity.id
                    ),
                );
            }
        }

        return binding_status(
            binding,
            "ready",
            Some(session.connection_id.clone()),
            Some(1),
            Some(session),
            String::new(),
        );
    }

    if let Some(account_ref) = binding.managed_account_ref.as_deref() {
        let matching_sessions = signing_sessions
            .into_iter()
            .filter(|session| session.signer_identity.id == account_ref)
            .collect::<Vec<_>>();
        return resolve_matching_sessions(binding, account_ref, matching_sessions);
    }

    if signing_sessions.is_empty() {
        return binding_status(
            binding,
            "unavailable",
            None,
            Some(0),
            None,
            "no authorized remote signer session currently exposes `sign_event`".to_owned(),
        );
    }

    if signing_sessions.len() > 1 {
        return binding_status(
            binding,
            "ambiguous",
            None,
            Some(signing_sessions.len()),
            None,
            "multiple authorized remote signer sessions expose `sign_event`; set managed_account_ref or signer_session_ref".to_owned(),
        );
    }

    let session = signing_sessions
        .into_iter()
        .next()
        .expect("single matching signer session");
    binding_status(
        binding,
        "ready",
        Some(session.connection_id.clone()),
        Some(1),
        Some(session),
        String::new(),
    )
}

fn resolve_matching_sessions(
    binding: &CapabilityBindingConfig,
    account_ref: &str,
    matching_sessions: Vec<&MycRemoteSessionView>,
) -> MycBindingResolution {
    if matching_sessions.is_empty() {
        return binding_status(
            binding,
            "unavailable",
            None,
            Some(0),
            None,
            format!(
                "no authorized remote signer session currently matches managed account `{account_ref}`"
            ),
        );
    }

    if matching_sessions.len() > 1 {
        return binding_status(
            binding,
            "ambiguous",
            None,
            Some(matching_sessions.len()),
            None,
            format!(
                "multiple authorized remote signer sessions currently match managed account `{account_ref}`; set signer_session_ref"
            ),
        );
    }

    let session = matching_sessions
        .into_iter()
        .next()
        .expect("single matching signer session");
    binding_status(
        binding,
        "ready",
        Some(session.connection_id.clone()),
        Some(1),
        Some(session),
        String::new(),
    )
}

fn binding_status(
    binding: &CapabilityBindingConfig,
    state: &str,
    resolved_signer_session_id: Option<String>,
    matched_session_count: Option<usize>,
    resolved_session: Option<&MycRemoteSessionView>,
    reason: String,
) -> MycBindingResolution {
    MycBindingResolution {
        view: SignerBindingStatusView {
            capability_id: binding.capability_id.clone(),
            provider_runtime_id: binding.provider_runtime_id.clone(),
            binding_model: binding.binding_model.clone(),
            state: state.to_owned(),
            source: binding.source.as_str().to_owned(),
            target_kind: Some(binding.target_kind.as_str().to_owned()),
            target: Some(binding.target.clone()),
            managed_account_ref: binding.managed_account_ref.clone(),
            signer_session_ref: binding.signer_session_ref.clone(),
            resolved_signer_session_id,
            matched_session_count,
            reason: if reason.is_empty() {
                None
            } else {
                Some(reason)
            },
        },
        resolved_account_id: resolved_session.map(|session| session.signer_identity.id.clone()),
        resolved_signer_public_key_hex: resolved_session
            .map(|session| session.signer_identity.public_key_hex.clone()),
    }
}

fn myc_signer_state(myc: &MycStatusView, binding: &SignerBindingStatusView) -> &'static str {
    match myc.state.as_str() {
        "degraded" => "degraded",
        "unavailable" => "unavailable",
        "unconfigured" => "unconfigured",
        _ => match binding.state.as_str() {
            "ready" => "ready",
            "unavailable" => "unavailable",
            _ => "unconfigured",
        },
    }
}

fn session_supports_signing(session: &MycRemoteSessionView) -> bool {
    session
        .permissions
        .iter()
        .any(|permission| permission == "sign_event" || permission.starts_with("sign_event:"))
}

fn local_availability(value: RadrootsNostrLocalSignerAvailability) -> &'static str {
    match value {
        RadrootsNostrLocalSignerAvailability::PublicOnly => "public_only",
        RadrootsNostrLocalSignerAvailability::SecretBacked => "secret_backed",
    }
}
