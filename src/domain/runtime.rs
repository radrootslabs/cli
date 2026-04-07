use std::process::ExitCode;

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    disposition: CommandDisposition,
    view: CommandView,
}

impl CommandOutput {
    pub fn success(view: CommandView) -> Self {
        Self {
            disposition: CommandDisposition::Success,
            view,
        }
    }

    pub fn unconfigured(view: CommandView) -> Self {
        Self {
            disposition: CommandDisposition::Unconfigured,
            view,
        }
    }

    pub fn external_unavailable(view: CommandView) -> Self {
        Self {
            disposition: CommandDisposition::ExternalUnavailable,
            view,
        }
    }

    pub fn internal_error(view: CommandView) -> Self {
        Self {
            disposition: CommandDisposition::InternalError,
            view,
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        self.disposition.exit_code()
    }

    pub fn view(&self) -> &CommandView {
        &self.view
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDisposition {
    Success,
    Unconfigured,
    ExternalUnavailable,
    InternalError,
}

impl CommandDisposition {
    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::SUCCESS,
            Self::Unconfigured => ExitCode::from(3),
            Self::ExternalUnavailable => ExitCode::from(4),
            Self::InternalError => ExitCode::from(1),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CommandView {
    AccountNew(AccountNewView),
    AccountWhoami(AccountWhoamiView),
    ConfigShow(ConfigShowView),
    MycStatus(MycStatusView),
    SignerStatus(SignerStatusView),
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigShowView {
    pub output_format: String,
    pub paths: PathsRuntimeView,
    pub logging: LoggingRuntimeView,
    pub account: AccountRuntimeView,
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
pub struct PathsRuntimeView {
    pub user_config_path: String,
    pub workspace_config_path: String,
    pub user_state_root: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountRuntimeView {
    pub identity_path: String,
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
pub struct AccountWhoamiView {
    pub path: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_identity: Option<IdentityPublicView>,
}

impl AccountWhoamiView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountNewView {
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
    pub myc: Option<MycStatusView>,
}

impl SignerStatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "degraded" => CommandDisposition::ExternalUnavailable,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalSignerStatusView {
    pub account_id: String,
    pub public_identity: IdentityPublicView,
    pub availability: String,
    pub secret_backed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MycStatusView {
    pub executable: String,
    pub state: String,
    pub service_status: Option<String>,
    pub ready: bool,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_signer: Option<LocalSignerStatusView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custody: Option<MycCustodyView>,
}

impl MycStatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "degraded" => CommandDisposition::ExternalUnavailable,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MycCustodyView {
    pub signer: MycCustodyIdentityView,
    pub user: MycCustodyIdentityView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_app: Option<MycCustodyIdentityView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MycCustodyIdentityView {
    pub resolved: bool,
    pub selected_account_id: Option<String>,
    pub selected_account_state: Option<String>,
    pub identity_id: Option<String>,
    pub public_key_hex: Option<String>,
    pub error: Option<String>,
}
