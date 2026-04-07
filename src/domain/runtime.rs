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
    Find(FindView),
    JobGet(JobGetView),
    JobList(JobListView),
    JobWatch(JobWatchView),
    ListingGet(ListingGetView),
    ListingMutation(ListingMutationView),
    ListingNew(ListingNewView),
    ListingValidate(ListingValidateView),
    LocalBackup(LocalBackupView),
    LocalExport(LocalExportView),
    LocalInit(LocalInitView),
    LocalStatus(LocalStatusView),
    MycStatus(MycStatusView),
    NetStatus(NetStatusView),
    RpcSessions(RpcSessionsView),
    RpcStatus(RpcStatusView),
    RelayList(RelayListView),
    SignerStatus(SignerStatusView),
    SyncPull(SyncActionView),
    SyncPush(SyncActionView),
    SyncStatus(SyncStatusView),
    SyncWatch(SyncWatchView),
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
    pub rpc: RpcRuntimeView,
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
pub struct RpcRuntimeView {
    pub url: String,
    pub bridge_auth_configured: bool,
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
pub struct FindView {
    pub state: String,
    pub source: String,
    pub query: String,
    pub count: usize,
    pub relay_count: usize,
    pub replica_db: String,
    pub freshness: SyncFreshnessView,
    pub results: Vec<FindResultView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl FindView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobListView {
    pub state: String,
    pub source: String,
    pub rpc_url: String,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub jobs: Vec<JobSummaryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobGetView {
    pub state: String,
    pub source: String,
    pub rpc_url: String,
    pub lookup: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobDetailView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobWatchView {
    pub state: String,
    pub source: String,
    pub rpc_url: String,
    pub job_id: String,
    pub interval_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub frames: Vec<JobWatchFrameView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobSummaryView {
    pub id: String,
    pub command: String,
    pub state: String,
    pub terminal: bool,
    pub signer: String,
    pub requested_at_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_unix: Option<u64>,
    pub recovered_after_restart: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobDetailView {
    pub id: String,
    pub command: String,
    pub state: String,
    pub terminal: bool,
    pub signer: String,
    pub requested_at_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at_unix: Option<u64>,
    pub recovered_after_restart: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_addr: Option<String>,
    pub delivery_policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_quorum: Option<usize>,
    pub relay_count: usize,
    pub acknowledged_relay_count: usize,
    pub required_acknowledged_relay_count: usize,
    pub attempt_count: usize,
    pub relay_outcome_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempt_summaries: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobWatchFrameView {
    pub sequence: usize,
    pub observed_at_unix: u64,
    pub state: String,
    pub terminal: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingNewView {
    pub state: String,
    pub source: String,
    pub file: String,
    pub listing_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_d_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingValidateView {
    pub state: String,
    pub source: String,
    pub file: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_d_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ListingValidationIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingValidationIssueView {
    pub field: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingGetView {
    pub state: String,
    pub source: String,
    pub lookup: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<FindQuantityView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<FindPriceView>,
    pub provenance: FindResultProvenanceView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl ListingGetView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingMutationView {
    pub state: String,
    pub operation: String,
    pub source: String,
    pub file: String,
    pub listing_id: String,
    pub listing_addr: String,
    pub seller_pubkey: String,
    pub event_kind: u32,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub deduplicated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<ListingMutationJobView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<ListingMutationEventView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl ListingMutationView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingMutationJobView {
    pub rpc_method: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingMutationEventView {
    pub kind: u32,
    pub author: String,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub event_addr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindResultView {
    pub id: String,
    pub product_key: String,
    pub title: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    pub available: FindQuantityView,
    pub price: FindPriceView,
    pub provenance: FindResultProvenanceView,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindQuantityView {
    pub total_amount: i64,
    pub total_unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_amount: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindPriceView {
    pub amount: f64,
    pub currency: String,
    pub per_amount: u32,
    pub per_unit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindResultProvenanceView {
    pub origin: String,
    pub freshness: String,
    pub relay_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFreshnessView {
    pub state: String,
    pub display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncQueueView {
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
pub struct RpcStatusView {
    pub state: String,
    pub source: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_signer_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_signer_modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_nip46_signer_sessions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_status_retention: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retained_jobs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_jobs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_jobs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_jobs: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_failed_jobs: Option<usize>,
    pub session_surface_enabled: bool,
    pub methods_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcSessionsView {
    pub state: String,
    pub source: String,
    pub url: String,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub sessions: Vec<RpcSessionView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcSessionView {
    pub session_id: String,
    pub role: String,
    pub client_pubkey: String,
    pub signer_pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_pubkey: Option<String>,
    pub relay_count: usize,
    pub permissions_count: usize,
    pub auth_required: bool,
    pub authorized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatusView {
    pub state: String,
    pub source: String,
    pub local_root: String,
    pub replica_db: String,
    pub relay_count: usize,
    pub publish_policy: String,
    pub freshness: SyncFreshnessView,
    pub queue: SyncQueueView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SyncStatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncActionView {
    pub direction: String,
    pub state: String,
    pub source: String,
    pub local_root: String,
    pub replica_db: String,
    pub relay_count: usize,
    pub publish_policy: String,
    pub freshness: SyncFreshnessView,
    pub queue: SyncQueueView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SyncActionView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncWatchView {
    pub state: String,
    pub source: String,
    pub interval_ms: u64,
    pub frames: Vec<SyncWatchFrameView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SyncWatchView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncWatchFrameView {
    pub sequence: usize,
    pub observed_at: u64,
    pub state: String,
    pub relay_count: usize,
    pub freshness: SyncFreshnessView,
    pub queue: SyncQueueView,
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
