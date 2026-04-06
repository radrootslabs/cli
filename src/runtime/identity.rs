use std::path::PathBuf;

use radroots_identity::{RadrootsIdentity, RadrootsIdentityPublic};

use crate::runtime::RuntimeError;
use crate::runtime::config::IdentityConfig;

#[derive(Debug, Clone)]
pub struct IdentityRecord {
    pub path: PathBuf,
    pub public_identity: RadrootsIdentityPublic,
    pub created: bool,
}

pub fn initialize_identity(config: &IdentityConfig) -> Result<IdentityRecord, RuntimeError> {
    let created = !config.path.exists();
    let identity = RadrootsIdentity::load_or_generate(Some(config.path.as_path()), true)?;
    Ok(IdentityRecord {
        path: config.path.clone(),
        public_identity: identity.to_public(),
        created,
    })
}

pub fn load_identity(config: &IdentityConfig) -> Result<IdentityRecord, RuntimeError> {
    let identity = RadrootsIdentity::load_from_path_auto(config.path.as_path())?;
    Ok(IdentityRecord {
        path: config.path.clone(),
        public_identity: identity.to_public(),
        created: false,
    })
}
