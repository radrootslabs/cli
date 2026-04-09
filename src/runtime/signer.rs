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

const SIGNER_BINDING_PROVIDER_RUNTIME_ID: &str = "myc";
const SIGNER_BINDING_MODEL: &str = "session_authorized_remote_signer";

pub fn resolve_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    match config.signer.backend {
        SignerBackend::Local => resolve_local_signer_status(config),
        SignerBackend::Myc => resolve_myc_signer_status(config),
    }
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
    let binding = resolve_myc_binding_status(config, &myc);
    let state = myc_signer_state(&myc, &binding).to_owned();
    SignerStatusView {
        mode: config.signer.backend.as_str().to_owned(),
        state,
        source: if myc.state == "ready" {
            binding.source.clone()
        } else {
            myc.source.clone()
        },
        account_id: resolve_myc_account_id(&binding, &myc),
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

fn resolve_myc_binding_status(
    config: &RuntimeConfig,
    myc: &MycStatusView,
) -> SignerBindingStatusView {
    let Some(binding) = config.capability_binding(SIGNER_REMOTE_NIP46_CAPABILITY) else {
        return SignerBindingStatusView {
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
                format!("configured signer session `{session_ref}` is not currently available"),
            );
        };

        if !session_supports_signing(session) {
            return binding_status(
                binding,
                "unauthorized",
                None,
                Some(1),
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
            "no authorized remote signer session currently exposes `sign_event`".to_owned(),
        );
    }

    if signing_sessions.len() > 1 {
        return binding_status(
            binding,
            "ambiguous",
            None,
            Some(signing_sessions.len()),
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
        String::new(),
    )
}

fn resolve_matching_sessions(
    binding: &CapabilityBindingConfig,
    account_ref: &str,
    matching_sessions: Vec<&MycRemoteSessionView>,
) -> SignerBindingStatusView {
    if matching_sessions.is_empty() {
        return binding_status(
            binding,
            "unavailable",
            None,
            Some(0),
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
        String::new(),
    )
}

fn binding_status(
    binding: &CapabilityBindingConfig,
    state: &str,
    resolved_signer_session_id: Option<String>,
    matched_session_count: Option<usize>,
    reason: String,
) -> SignerBindingStatusView {
    SignerBindingStatusView {
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

fn resolve_myc_account_id(
    binding: &SignerBindingStatusView,
    myc: &MycStatusView,
) -> Option<String> {
    if let Some(account_ref) = &binding.managed_account_ref {
        return Some(account_ref.clone());
    }

    binding
        .resolved_signer_session_id
        .as_deref()
        .or(binding.signer_session_ref.as_deref())
        .and_then(|session_id| {
            myc.remote_sessions
                .iter()
                .find(|session| session.connection_id == session_id)
                .map(|session| session.signer_identity.id.clone())
        })
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
