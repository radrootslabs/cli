use serde::Serialize;

#[derive(Debug, Clone)]
pub enum CommandOutput {
    IdentityInit(IdentityInitView),
    IdentityShow(IdentityShowView),
    RuntimeShow(RuntimeShowView),
    SignerStatus(SignerStatusView),
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeShowView {
    pub output_format: String,
    pub logging: LoggingRuntimeView,
    pub identity: IdentityRuntimeView,
    pub signer: SignerRuntimeView,
    pub myc: MycRuntimeView,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoggingRuntimeView {
    pub initialized: bool,
    pub filter: String,
    pub stdout: bool,
    pub directory: Option<String>,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityRuntimeView {
    pub path: String,
    pub allow_generate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerRuntimeView {
    pub backend: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MycRuntimeView {
    pub executable: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityPublicView {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
}

impl IdentityPublicView {
    pub fn from_public_identity(identity: &radroots_identity::RadrootsIdentityPublic) -> Self {
        Self {
            id: identity.id.to_string(),
            public_key_hex: identity.public_key_hex.clone(),
            public_key_npub: identity.public_key_npub.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityShowView {
    pub path: String,
    pub public_identity: IdentityPublicView,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityInitView {
    pub path: String,
    pub created: bool,
    pub public_identity: IdentityPublicView,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerStatusView {
    pub backend: String,
    pub state: String,
    pub reason: Option<String>,
    pub local: Option<LocalSignerStatusView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalSignerStatusView {
    pub account_id: String,
    pub public_identity: IdentityPublicView,
    pub availability: String,
    pub secret_backed: bool,
}
