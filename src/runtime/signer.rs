use crate::domain::runtime::{IdentityPublicView, LocalSignerStatusView, SignerStatusView};
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use radroots_identity::IdentityError;
use radroots_nostr_signer::prelude::{
    RadrootsNostrLocalSignerAvailability, RadrootsNostrLocalSignerCapability,
    RadrootsNostrSignerCapability,
};

pub fn resolve_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    match config.signer.backend {
        SignerBackend::Local => resolve_local_signer_status(config),
        SignerBackend::Myc => SignerStatusView {
            backend: config.signer.backend.as_str().to_owned(),
            state: "unconfigured".to_owned(),
            reason: Some("myc backend is not bootstrapped in this slice".to_owned()),
            local: None,
        },
    }
}

fn resolve_local_signer_status(config: &RuntimeConfig) -> SignerStatusView {
    match crate::runtime::identity::load_identity(&config.identity) {
        Ok(identity) => {
            let capability = RadrootsNostrSignerCapability::LocalAccount(
                RadrootsNostrLocalSignerCapability::new(
                    identity.public_identity.id.clone(),
                    identity.public_identity.clone(),
                    RadrootsNostrLocalSignerAvailability::SecretBacked,
                ),
            );
            let local = capability
                .local_account()
                .expect("local signer capability")
                .clone();
            SignerStatusView {
                backend: config.signer.backend.as_str().to_owned(),
                state: "ready".to_owned(),
                reason: None,
                local: Some(LocalSignerStatusView {
                    account_id: local.account_id.to_string(),
                    public_identity: IdentityPublicView::from_public_identity(
                        &local.public_identity,
                    ),
                    availability: local_availability(local.availability).to_owned(),
                    secret_backed: local.is_secret_backed(),
                }),
            }
        }
        Err(crate::runtime::RuntimeError::Identity(IdentityError::NotFound(path))) => {
            SignerStatusView {
                backend: config.signer.backend.as_str().to_owned(),
                state: "unconfigured".to_owned(),
                reason: Some(format!(
                    "local identity file was not found at {}",
                    path.display()
                )),
                local: None,
            }
        }
        Err(error) => SignerStatusView {
            backend: config.signer.backend.as_str().to_owned(),
            state: "error".to_owned(),
            reason: Some(error.to_string()),
            local: None,
        },
    }
}

fn local_availability(value: RadrootsNostrLocalSignerAvailability) -> &'static str {
    match value {
        RadrootsNostrLocalSignerAvailability::PublicOnly => "public_only",
        RadrootsNostrLocalSignerAvailability::SecretBacked => "secret_backed",
    }
}
