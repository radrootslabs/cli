use std::process::ExitCode;

use radroots_nostr_accounts::prelude::RadrootsNostrAccountRecord;
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
    AccountList(AccountListView),
    AccountNew(AccountNewView),
    AccountUse(AccountUseView),
    AccountWhoami(AccountWhoamiView),
    ConfigShow(ConfigShowView),
    Doctor(DoctorView),
    LocalBackup(LocalBackupView),
    LocalExport(LocalExportView),
    LocalInit(LocalInitView),
    LocalStatus(LocalStatusView),
    MycStatus(MycStatusView),
    NetStatus(NetStatusView),
    RelayList(RelayListView),
    SignerStatus(SignerStatusView),
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigShowView {
    pub source: String,
    pub output: OutputRuntimeView,
    pub config_files: ConfigFilesRuntimeView,
    pub paths: PathsRuntimeView,
    pub logging: LoggingRuntimeView,
    pub account: AccountRuntimeView,
    pub signer: SignerRuntimeView,
    pub relay: RelayRuntimeView,
    pub local: LocalRuntimeView,
    pub myc: MycRuntimeView,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputRuntimeView {
    pub format: String,
    pub verbosity: String,
    pub color: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFilesRuntimeView {
    pub user_present: bool,
    pub workspace_present: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    pub store_path: String,
    pub secrets_dir: String,
    pub legacy_identity_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerRuntimeView {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayRuntimeView {
    pub count: usize,
    pub urls: Vec<String>,
    pub publish_policy: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalRuntimeView {
    pub root: String,
    pub replica_db_path: String,
    pub backups_dir: String,
    pub exports_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MycRuntimeView {
    pub executable: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorView {
    pub ok: bool,
    pub state: String,
    pub checks: Vec<DoctorCheckView>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheckView {
    pub name: String,
    pub status: String,
    pub detail: String,
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
pub struct AccountSummaryView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub signer: String,
    pub is_default: bool,
}

impl AccountSummaryView {
    pub fn from_account_record(
        record: &RadrootsNostrAccountRecord,
        signer: &str,
        is_default: bool,
    ) -> Self {
        Self {
            id: record.account_id.to_string(),
            display_name: record.label.clone(),
            signer: signer.to_owned(),
            is_default,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountWhoamiView {
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountSummaryView>,
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
    pub state: String,
    pub source: String,
    pub account: AccountSummaryView,
    pub public_identity: IdentityPublicView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountUseView {
    pub state: String,
    pub source: String,
    pub active_account_id: String,
    pub account: AccountSummaryView,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountListView {
    pub source: String,
    pub count: usize,
    pub accounts: Vec<AccountSummaryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalInitView {
    pub state: String,
    pub source: String,
    pub local_root: String,
    pub replica_db: String,
    pub path: String,
    pub replica_db_version: String,
    pub backup_format_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalStatusView {
    pub state: String,
    pub source: String,
    pub local_root: String,
    pub replica_db: String,
    pub path: String,
    pub replica_db_version: String,
    pub backup_format_version: String,
    pub schema_hash: String,
    pub counts: LocalReplicaCountsView,
    pub sync: LocalReplicaSyncView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl LocalStatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalReplicaCountsView {
    pub farms: u64,
    pub listings: u64,
    pub profiles: u64,
    pub relays: u64,
    pub event_states: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalReplicaSyncView {
    pub expected_count: usize,
    pub pending_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalBackupView {
    pub state: String,
    pub source: String,
    pub file: String,
    pub size_bytes: u64,
    pub backup_format_version: String,
    pub replica_db_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl LocalBackupView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalExportView {
    pub state: String,
    pub source: String,
    pub format: String,
    pub file: String,
    pub records: usize,
    pub export_version: String,
    pub schema_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl LocalExportView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayListView {
    pub state: String,
    pub source: String,
    pub publish_policy: String,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub relays: Vec<RelayEntryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl RelayListView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayEntryView {
    pub url: String,
    pub read: bool,
    pub write: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetStatusView {
    pub state: String,
    pub source: String,
    pub session: String,
    pub relay_count: usize,
    pub publish_policy: String,
    pub signer_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl NetStatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerStatusView {
    pub mode: String,
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalSignerStatusView>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    pub source: String,
    pub service_status: Option<String>,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
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
