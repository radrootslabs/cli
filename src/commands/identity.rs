use crate::domain::runtime::{IdentityInitView, IdentityPublicView, IdentityShowView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::identity::{initialize_identity, load_identity};

pub fn init(config: &RuntimeConfig) -> Result<IdentityInitView, RuntimeError> {
    let identity = initialize_identity(&config.identity)?;
    Ok(IdentityInitView {
        path: identity.path.display().to_string(),
        created: identity.created,
        public_identity: IdentityPublicView::from_public_identity(&identity.public_identity),
    })
}

pub fn show(config: &RuntimeConfig) -> Result<IdentityShowView, RuntimeError> {
    let identity = load_identity(&config.identity)?;
    Ok(IdentityShowView {
        path: identity.path.display().to_string(),
        public_identity: IdentityPublicView::from_public_identity(&identity.public_identity),
    })
}
