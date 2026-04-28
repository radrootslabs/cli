#![allow(dead_code)]

use std::process::ExitCode;

use radroots_events::farm::RadrootsFarm;
use radroots_events::listing::RadrootsListingLocation;
use radroots_events::profile::RadrootsProfile;
use radroots_nostr_accounts::prelude::RadrootsNostrAccountRecord;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDisposition {
    Success,
    NotFound,
    Unconfigured,
    ExternalUnavailable,
    Unsupported,
    InternalError,
}

impl CommandDisposition {
    pub fn exit_code(self) -> ExitCode {
        match self {
            Self::Success => ExitCode::SUCCESS,
            Self::NotFound => ExitCode::from(4),
            Self::Unconfigured => ExitCode::from(3),
            Self::ExternalUnavailable => ExitCode::from(4),
            Self::Unsupported => ExitCode::from(5),
            Self::InternalError => ExitCode::from(1),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigShowView {
    pub source: String,
    pub output: OutputRuntimeView,
    pub interaction: InteractionRuntimeView,
    pub config_files: ConfigFilesRuntimeView,
    pub paths: PathsRuntimeView,
    pub migration: MigrationRuntimeView,
    pub logging: LoggingRuntimeView,
    pub account: AccountRuntimeView,
    pub signer: SignerRuntimeView,
    pub relay: RelayRuntimeView,
    pub local: LocalRuntimeView,
    pub myc: MycRuntimeView,
    pub write_plane: WritePlaneRuntimeView,
    pub workflow: WorkflowRuntimeView,
    pub hyf_provider: HyfProviderRuntimeView,
    pub hyf: HyfRuntimeView,
    pub rpc: RpcRuntimeView,
    pub capability_bindings: Vec<CapabilityBindingRuntimeView>,
    pub resolved_providers: Vec<ResolvedProviderRuntimeView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeActionView {
    pub action: String,
    pub runtime_id: String,
    pub instance_id: String,
    pub instance_source: String,
    pub runtime_group: String,
    pub state: String,
    pub source: String,
    pub detail: String,
    pub mutates_bindings: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_step: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeManagedConfigView {
    pub runtime_id: String,
    pub instance_id: String,
    pub instance_source: String,
    pub runtime_group: String,
    pub state: String,
    pub source: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub config_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_bootstrap_secret: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_config_bootstrap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_signer_provider: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeLogsView {
    pub runtime_id: String,
    pub instance_id: String,
    pub instance_source: String,
    pub runtime_group: String,
    pub state: String,
    pub source: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_log_path: Option<String>,
    pub stdout_log_present: bool,
    pub stderr_log_present: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatusView {
    pub runtime_id: String,
    pub instance_id: String,
    pub instance_source: String,
    pub runtime_group: String,
    pub management_posture: String,
    pub state: String,
    pub source: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub management_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_manager_integration: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses_absolute_binary_paths: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_cli_binding: Option<bool>,
    pub install_state: String,
    pub health_state: String,
    pub health_source: String,
    pub registry_path: String,
    pub lifecycle_actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_paths: Option<RuntimeInstancePathsView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_record: Option<RuntimeInstanceRecordView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInstancePathsView {
    pub install_dir: String,
    pub state_dir: String,
    pub logs_dir: String,
    pub run_dir: String,
    pub secrets_dir: String,
    pub pid_file_path: String,
    pub stdout_log_path: String,
    pub stderr_log_path: String,
    pub metadata_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInstanceRecordView {
    pub management_mode: String,
    pub install_state: String,
    pub binary_path: String,
    pub config_path: String,
    pub logs_path: String,
    pub run_path: String,
    pub installed_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_material_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stopped_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationRuntimeView {
    pub posture: String,
    pub state: String,
    pub silent_startup_relocation: bool,
    pub compatibility_window: String,
    pub detected_legacy_paths: Vec<LegacyPathRuntimeView>,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyPathRuntimeView {
    pub id: String,
    pub description: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    pub import_hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputRuntimeView {
    pub format: String,
    pub verbosity: String,
    pub color: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InteractionRuntimeView {
    pub input_enabled: bool,
    pub assume_yes: bool,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub prompts_allowed: bool,
    pub confirmations_allowed: bool,
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
    pub profile: String,
    pub profile_source: String,
    pub allowed_profiles: Vec<String>,
    pub root_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_local_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_local_root_source: Option<String>,
    pub subordinate_path_override_source: String,
    pub app_namespace: String,
    pub shared_accounts_namespace: String,
    pub shared_identities_namespace: String,
    pub app_config_path: String,
    pub workspace_config_enabled: bool,
    pub workspace_config_path: Option<String>,
    pub app_data_root: String,
    pub app_logs_root: String,
    pub shared_accounts_data_root: String,
    pub shared_accounts_secrets_root: String,
    pub default_identity_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountRuntimeView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    pub store_path: String,
    pub secrets_dir: String,
    pub identity_path: String,
    pub secret_backend: AccountSecretRuntimeView,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountSecretRuntimeView {
    pub contract_default_backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_default_fallback: Option<String>,
    pub allowed_backends: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_vault_policy: Option<String>,
    pub uses_protected_store: bool,
    pub configured_primary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_fallback: Option<String>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_backend: Option<String>,
    pub used_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    pub status_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowRuntimeView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub hyf_helper_state: String,
    pub hyf_helper_detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HyfRuntimeView {
    pub enabled: bool,
    pub executable: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HyfProviderRuntimeView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub executable: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deterministic_available: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WritePlaneRuntimeView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub detail: String,
    pub bridge_auth_configured: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcRuntimeView {
    pub url: String,
    pub bridge_auth_configured: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedProviderRuntimeView {
    pub capability_id: String,
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityBindingRuntimeView {
    pub capability_id: String,
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_account_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorView {
    pub ok: bool,
    pub state: String,
    pub account_resolution: AccountResolutionView,
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
pub struct AccountResolutionView {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_account: Option<AccountSummaryView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_account: Option<AccountSummaryView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountWhoamiView {
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub account_resolution: AccountResolutionView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_identity: Option<IdentityPublicView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
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
pub struct AccountImportView {
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
    pub default_account_id: String,
    pub account: AccountSummaryView,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountClearDefaultView {
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared_account: Option<AccountSummaryView>,
    pub remaining_account_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountRemoveView {
    pub state: String,
    pub source: String,
    pub removed_account: AccountSummaryView,
    pub default_cleared: bool,
    pub remaining_account_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
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
pub struct SetupView {
    pub state: String,
    pub source: String,
    pub role: String,
    pub account_resolution: AccountResolutionView,
    pub local_state: String,
    pub local_root: String,
    pub relay_state: String,
    pub relay_count: usize,
    pub farm_state: String,
    #[serde(default)]
    pub ready: Vec<String>,
    #[serde(default)]
    pub needs_attention: Vec<String>,
    #[serde(default)]
    pub next: Vec<String>,
}

impl SetupView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusView {
    pub state: String,
    pub source: String,
    pub account_resolution: AccountResolutionView,
    pub local_state: String,
    pub local_root: String,
    pub relay_state: String,
    pub relay_count: usize,
    pub farm_state: String,
    #[serde(default)]
    pub ready: Vec<String>,
    #[serde(default)]
    pub needs_attention: Vec<String>,
    #[serde(default)]
    pub next: Vec<String>,
}

impl StatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmSetupView {
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<FarmConfigSummaryView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl FarmSetupView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmSetView {
    pub state: String,
    pub source: String,
    pub field: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<FarmConfigSummaryView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl FarmSetView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmStatusView {
    pub state: String,
    pub source: String,
    pub scope: String,
    pub path: String,
    pub config_present: bool,
    pub config_valid: bool,
    pub account_state: String,
    pub listing_defaults_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<FarmConfigSummaryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl FarmStatusView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmGetView {
    pub state: String,
    pub source: String,
    pub scope: String,
    pub path: String,
    pub config_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<FarmConfigDocumentView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl FarmGetView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" | "missing" => CommandDisposition::Unconfigured,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmPublishView {
    pub state: String,
    pub source: String,
    pub scope: String,
    pub path: String,
    pub config_present: bool,
    pub dry_run: bool,
    pub selected_account_id: String,
    pub selected_account_pubkey: String,
    pub farm_d_tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_signer_session_id: Option<String>,
    pub profile: FarmPublishComponentView,
    pub farm: FarmPublishComponentView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl FarmPublishView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "partial" | "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmPublishComponentView {
    pub state: String,
    pub rpc_method: String,
    pub event_kind: u32,
    pub deduplicated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<FarmPublishJobView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<FarmPublishEventView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmPublishJobView {
    pub rpc_method: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmPublishEventView {
    pub kind: u32,
    pub author: String,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_addr: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayFailureView {
    pub relay: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmConfigSummaryView {
    pub scope: String,
    pub path: String,
    pub selected_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_account_pubkey: Option<String>,
    pub farm_d_tag: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    pub delivery_method: String,
    pub publication: FarmPublicationView,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmConfigDocumentView {
    pub selection: FarmSelectionView,
    pub profile: RadrootsProfile,
    pub farm: RadrootsFarm,
    pub listing_defaults: FarmListingDefaultsView,
    pub publication: FarmPublicationView,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmSelectionView {
    pub scope: String,
    pub account: String,
    pub farm_d_tag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmListingDefaultsView {
    pub delivery_method: String,
    pub location: RadrootsListingLocation,
}

#[derive(Debug, Clone, Serialize)]
pub struct FarmPublicationView {
    pub profile_state: String,
    pub farm_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_published_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_published_at: Option<u64>,
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
    pub hyf: Option<FindHyfView>,
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
pub struct FindHyfView {
    pub state: String,
    pub source: String,
    pub rewritten_query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_terms: Vec<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
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
    pub signer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderNewView {
    pub state: String,
    pub source: String,
    pub order_id: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_lookup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    pub ready_for_submit: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<OrderDraftItemView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<OrderIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderNewView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderGetView {
    pub state: String,
    pub source: String,
    pub lookup: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_lookup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    pub ready_for_submit: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<OrderDraftItemView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<OrderJobView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OrderWorkflowView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<OrderIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderGetView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderListView {
    pub state: String,
    pub source: String,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orders: Vec<OrderSummaryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderListView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderSubmitView {
    pub state: String,
    pub source: String,
    pub order_id: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_lookup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<u32>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub deduplicated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<OrderJobView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<OrderIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderSubmitView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "missing" => CommandDisposition::NotFound,
            "unconfigured" => CommandDisposition::Unconfigured,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderDecisionView {
    pub state: String,
    pub source: String,
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<u32>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<OrderIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderDecisionView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "missing" => CommandDisposition::NotFound,
            "unconfigured" => CommandDisposition::Unconfigured,
            "unavailable" => CommandDisposition::ExternalUnavailable,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderStatusView {
    pub state: String,
    pub source: String,
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reducer_issues: Vec<OrderIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
    #[serde(default)]
    pub fetched_count: usize,
    #[serde(default)]
    pub decoded_count: usize,
    #[serde(default)]
    pub skipped_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderStatusView {
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
pub struct OrderSubmitWatchView {
    pub submit: OrderSubmitView,
    pub watch: OrderWatchView,
}

impl OrderSubmitWatchView {
    pub fn disposition(&self) -> CommandDisposition {
        let submit = self.submit.disposition();
        if submit != CommandDisposition::Success {
            return submit;
        }
        self.watch.disposition()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderWatchView {
    pub state: String,
    pub source: String,
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub interval_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OrderWorkflowView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<OrderWatchFrameView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderWatchView {
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
pub struct OrderWatchFrameView {
    pub sequence: usize,
    pub observed_at_unix: u64,
    pub state: String,
    pub terminal: bool,
    pub signer_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderWorkflowView {
    pub state: String,
    pub source: String,
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated_listing_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderHistoryView {
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
    #[serde(default)]
    pub fetched_count: usize,
    #[serde(default)]
    pub decoded_count: usize,
    #[serde(default)]
    pub skipped_count: usize,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orders: Vec<OrderHistoryEntryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderHistoryView {
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
pub struct OrderHistoryEntryView {
    pub id: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_lookup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted_at_unix: Option<u64>,
    pub updated_at_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<OrderJobView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OrderWorkflowView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<OrderIssueView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderCancelView {
    pub state: String,
    pub source: String,
    pub lookup: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<OrderJobView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl OrderCancelView {
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
pub struct OrderSummaryView {
    pub id: String,
    pub state: String,
    pub ready_for_submit: bool,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_lookup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buyer_account_id: Option<String>,
    pub item_count: usize,
    pub updated_at_unix: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<OrderJobView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<OrderIssueView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderDraftItemView {
    pub bin_id: String,
    pub bin_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderIssueView {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderJobView {
    pub job_id: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_addr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingNewView {
    pub state: String,
    pub source: String,
    pub file: String,
    pub listing_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_d_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl ListingNewView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
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
pub struct ListingListView {
    pub state: String,
    pub source: String,
    pub count: usize,
    pub draft_dir: String,
    pub listings: Vec<ListingSummaryView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl ListingListView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingSummaryView {
    pub id: String,
    pub state: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_d_tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    pub updated_at_unix: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ListingValidationIssueView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SellAddView {
    pub state: String,
    pub source: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SellAddView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "unconfigured" => CommandDisposition::Unconfigured,
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SellShowView {
    pub state: String,
    pub source: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SellShowView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SellCheckView {
    pub state: String,
    pub source: String,
    pub file: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub farm_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ListingValidationIssueView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SellCheckView {
    pub fn disposition(&self) -> CommandDisposition {
        CommandDisposition::Success
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SellMutationView {
    pub state: String,
    pub operation: String,
    pub source: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    pub listing_addr: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub deduplicated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SellMutationView {
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
pub struct SellDraftMutationView {
    pub state: String,
    pub operation: String,
    pub source: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_key: Option<String>,
    pub changed_label: String,
    pub changed_value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

impl SellDraftMutationView {
    pub fn disposition(&self) -> CommandDisposition {
        match self.state.as_str() {
            "error" => CommandDisposition::InternalError,
            _ => CommandDisposition::Success,
        }
    }
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
    pub listing_addr: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acknowledged_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
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
    pub signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_signer_session_id: Option<String>,
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
    pub requested_signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingMutationEventView {
    pub kind: u32,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u32>,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub event_addr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindResultView {
    pub id: String,
    pub product_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_addr: Option<String>,
    pub title: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_primary: Option<String>,
    pub available: FindQuantityView,
    pub price: FindPriceView,
    pub provenance: FindResultProvenanceView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyf: Option<FindResultHyfView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindResultHyfView {
    pub state: String,
    pub rewritten_query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_terms: Vec<String>,
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
    pub account_resolution: AccountResolutionView,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connected_relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_relays: Vec<RelayFailureView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingested_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_count: Option<usize>,
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
    pub signer_account_id: Option<String>,
    pub account_resolution: AccountResolutionView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub binding: SignerBindingStatusView,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_kinds: Vec<SignerWriteKindReadinessView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalSignerStatusView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub myc: Option<MycStatusView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerWriteKindReadinessView {
    pub command: String,
    pub event_kind: u32,
    pub permission: String,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerSessionActionView {
    pub action: String,
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_signer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relays: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replayed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    pub backend: String,
    pub used_fallback: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerBindingStatusView {
    pub capability_id: String,
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_account_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_session_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_signer_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_session_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    pub remote_session_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_signer: Option<LocalSignerStatusView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_sessions: Vec<MycRemoteSessionView>,
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
pub struct MycRemoteSessionView {
    pub connection_id: String,
    pub signer_identity: IdentityPublicView,
    pub user_identity: IdentityPublicView,
    pub relay_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
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
