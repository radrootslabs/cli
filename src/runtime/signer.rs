use crate::domain::runtime::{IdentityPublicView, LocalSignerStatusView, SignerStatusView};
use crate::runtime::accounts::SHARED_ACCOUNT_STORE_SOURCE;
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use radroots_nostr_accounts::prelude::RadrootsNostrSelectedAccountStatus;
use radroots_nostr_signer::prelude::{
    RadrootsNostrLocalSignerAvailability, RadrootsNostrLocalSignerCapability,
    RadrootsNostrSignerCapability,
};

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
            local: None,
            myc: None,
        },
        Err(error) => SignerStatusView {
            mode: config.signer.backend.as_str().to_owned(),
            state: "error".to_owned(),
            source: SHARED_ACCOUNT_STORE_SOURCE.to_owned(),
            account_id: None,
            reason: Some(error.to_string()),
            local: None,
            myc: None,
        },
    }
}

fn resolve_myc_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    let myc = crate::runtime::myc::resolve_status(&config.myc);
    SignerStatusView {
        mode: config.signer.backend.as_str().to_owned(),
        state: myc.state.clone(),
        source: "myc status command · local first".to_owned(),
        account_id: myc
            .local_signer
            .as_ref()
            .map(|local| local.account_id.clone()),
        reason: myc.reason.clone(),
        local: None,
        myc: Some(myc),
    }
}

fn local_availability(value: RadrootsNostrLocalSignerAvailability) -> &'static str {
    match value {
        RadrootsNostrLocalSignerAvailability::PublicOnly => "public_only",
        RadrootsNostrLocalSignerAvailability::SecretBacked => "secret_backed",
    }
}
