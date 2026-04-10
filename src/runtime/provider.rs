use crate::runtime::config::{
    CapabilityBindingInspection, CapabilityBindingInspectionState, RuntimeConfig,
    INFERENCE_HYF_STDIO_CAPABILITY, WORKFLOW_TRADE_CAPABILITY,
    WRITE_PLANE_TRADE_JSONRPC_CAPABILITY,
};
use crate::runtime::hyf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProvenance {
    ExplicitBinding,
    ManagedDefault,
    DirectConfig,
    Disabled,
    Unavailable,
}

impl ProviderProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitBinding => "explicit_binding",
            Self::ManagedDefault => "managed_default",
            Self::DirectConfig => "direct_config",
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProviderView {
    pub capability_id: String,
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    pub target_kind: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePlaneProviderView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    pub target_kind: Option<String>,
    pub target: Option<String>,
    pub detail: String,
    pub bridge_auth_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProviderView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    pub target_kind: Option<String>,
    pub target: Option<String>,
    pub hyf_helper_state: String,
    pub hyf_helper_detail: String,
}

impl WorkflowProviderView {
    pub fn detail(&self) -> String {
        match (self.target_kind.as_deref(), self.target.as_deref()) {
            (Some(target_kind), Some(target)) if self.state == "configured" => {
                format!(
                    "{} workflow provider configured via {} {}",
                    self.provider_runtime_id, target_kind, target
                )
            }
            _ if self.state == "configured" => {
                format!("{} workflow provider configured", self.provider_runtime_id)
            }
            _ => self.source.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyfProviderView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub provenance: String,
    pub source: String,
    pub target_kind: Option<String>,
    pub target: Option<String>,
    pub executable: String,
    pub reason: Option<String>,
    pub protocol_version: Option<u64>,
    pub deterministic_available: Option<bool>,
}

pub fn resolve_write_plane_provider(config: &RuntimeConfig) -> WritePlaneProviderView {
    let _binding = inspect_binding(config, WRITE_PLANE_TRADE_JSONRPC_CAPABILITY);
    WritePlaneProviderView {
        provider_runtime_id: "radrootsd".to_owned(),
        binding_model: "daemon_backed_jsonrpc".to_owned(),
        state: "configured".to_owned(),
        provenance: ProviderProvenance::DirectConfig.as_str().to_owned(),
        source: "raw rpc config resolves the current write plane".to_owned(),
        target_kind: None,
        target: Some(config.rpc.url.clone()),
        detail: "actor-authored durable writes still resolve through rpc.url until authoritative write-plane binding resolution lands".to_owned(),
        bridge_auth_configured: config.rpc.bridge_bearer_token.is_some(),
    }
}

pub fn resolve_workflow_provider(config: &RuntimeConfig) -> WorkflowProviderView {
    let binding = inspect_binding(config, WORKFLOW_TRADE_CAPABILITY);
    let provenance = binding_provenance(&binding).as_str().to_owned();

    WorkflowProviderView {
        provider_runtime_id: binding.provider_runtime_id,
        binding_model: binding.binding_model,
        state: binding.state.as_str().to_owned(),
        provenance,
        source: binding.source,
        target_kind: binding.target_kind,
        target: binding.target,
        hyf_helper_state: "not_implied".to_owned(),
        hyf_helper_detail:
            "cli bindings do not imply an rhi -> hyf helper path; any worker helper remains explicit and optional"
                .to_owned(),
    }
}

pub fn resolve_hyf_provider(config: &RuntimeConfig) -> HyfProviderView {
    let binding = inspect_binding(config, INFERENCE_HYF_STDIO_CAPABILITY);
    let status = hyf::resolve_runtime_status(config);
    let binding_configured = binding.state == CapabilityBindingInspectionState::Configured;
    let provenance = if binding_configured {
        binding_provenance(&binding)
    } else if status.state == "disabled" {
        ProviderProvenance::Disabled
    } else {
        ProviderProvenance::DirectConfig
    }
    .as_str()
    .to_owned();
    let target_kind = hyf_target_kind(config, &binding);
    let target = hyf_target(config, &binding);
    let executable = hyf_executable(config, &binding, &status);
    let source = if binding_configured {
        binding.source.clone()
    } else {
        status.source.clone()
    };

    HyfProviderView {
        provider_runtime_id: binding.provider_runtime_id,
        binding_model: binding.binding_model,
        state: status.state,
        provenance,
        source,
        target_kind,
        target,
        executable,
        reason: status.reason,
        protocol_version: status.protocol_version,
        deterministic_available: status.deterministic_available,
    }
}

pub fn resolve_capability_providers(config: &RuntimeConfig) -> Vec<ResolvedProviderView> {
    let write = resolve_write_plane_provider(config);
    let workflow = resolve_workflow_provider(config);
    let hyf = resolve_hyf_provider(config);

    vec![
        ResolvedProviderView {
            capability_id: WRITE_PLANE_TRADE_JSONRPC_CAPABILITY.to_owned(),
            provider_runtime_id: write.provider_runtime_id,
            binding_model: write.binding_model,
            state: write.state,
            provenance: write.provenance,
            source: write.source,
            target_kind: write.target_kind,
            target: write.target,
        },
        ResolvedProviderView {
            capability_id: WORKFLOW_TRADE_CAPABILITY.to_owned(),
            provider_runtime_id: workflow.provider_runtime_id,
            binding_model: workflow.binding_model,
            state: workflow.state,
            provenance: workflow.provenance,
            source: workflow.source,
            target_kind: workflow.target_kind,
            target: workflow.target,
        },
        ResolvedProviderView {
            capability_id: INFERENCE_HYF_STDIO_CAPABILITY.to_owned(),
            provider_runtime_id: hyf.provider_runtime_id,
            binding_model: hyf.binding_model,
            state: hyf.state,
            provenance: hyf.provenance,
            source: hyf.source,
            target_kind: hyf.target_kind,
            target: hyf.target,
        },
    ]
}

fn inspect_binding(config: &RuntimeConfig, capability_id: &str) -> CapabilityBindingInspection {
    config
        .inspect_capability_bindings()
        .into_iter()
        .find(|binding| binding.capability_id == capability_id)
        .expect("provider capability binding inspection must exist")
}

fn binding_provenance(binding: &CapabilityBindingInspection) -> ProviderProvenance {
    match binding.state {
        CapabilityBindingInspectionState::Configured => match binding.target_kind.as_deref() {
            Some("managed_instance") => ProviderProvenance::ManagedDefault,
            _ => ProviderProvenance::ExplicitBinding,
        },
        CapabilityBindingInspectionState::Disabled => ProviderProvenance::Disabled,
        CapabilityBindingInspectionState::NotConfigured => ProviderProvenance::Unavailable,
    }
}

fn hyf_target_kind(
    config: &RuntimeConfig,
    binding: &CapabilityBindingInspection,
) -> Option<String> {
    if binding.state == CapabilityBindingInspectionState::Configured {
        return binding.target_kind.clone();
    }
    if config.hyf.enabled {
        return Some("direct_config".to_owned());
    }
    None
}

fn hyf_target(config: &RuntimeConfig, binding: &CapabilityBindingInspection) -> Option<String> {
    if binding.state == CapabilityBindingInspectionState::Configured {
        return binding.target.clone();
    }
    if config.hyf.enabled {
        return Some(config.hyf.executable.display().to_string());
    }
    None
}

fn hyf_executable(
    config: &RuntimeConfig,
    binding: &CapabilityBindingInspection,
    status: &hyf::HyfStatusView,
) -> String {
    if binding.state == CapabilityBindingInspectionState::Configured
        && binding.target_kind.as_deref() == Some("explicit_endpoint")
    {
        return binding.target.clone().unwrap_or_else(|| status.executable.clone());
    }
    if !config.hyf.enabled {
        return status.executable.clone();
    }
    status.executable.clone()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;

    use super::{
        ProviderProvenance, resolve_capability_providers, resolve_hyf_provider,
        resolve_workflow_provider, resolve_write_plane_provider,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, CapabilityBindingConfig,
        CapabilityBindingSource, CapabilityBindingTargetKind, HyfConfig, IdentityConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    fn sample_config(bindings: Vec<CapabilityBindingConfig>, hyf_enabled: bool) -> RuntimeConfig {
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Normal,
                color: true,
                dry_run: false,
            },
            paths: PathsConfig {
                profile: "interactive_user".into(),
                profile_source: "default".into(),
                allowed_profiles: vec!["interactive_user".into()],
                root_source: "host_defaults".into(),
                repo_local_root: None,
                repo_local_root_source: None,
                subordinate_path_override_source: "runtime_config".into(),
                app_namespace: "apps/cli".into(),
                shared_accounts_namespace: "shared/accounts".into(),
                shared_identities_namespace: "shared/identities".into(),
                app_config_path: PathBuf::from("/tmp/config.toml"),
                workspace_config_path: PathBuf::from("/tmp/workspace-config.toml"),
                app_data_root: PathBuf::from("/tmp/data"),
                app_logs_root: PathBuf::from("/tmp/logs"),
                shared_accounts_data_root: PathBuf::from("/tmp/shared/accounts"),
                shared_accounts_secrets_root: PathBuf::from("/tmp/shared/accounts-secrets"),
                default_identity_path: PathBuf::from("/tmp/default-identity.json"),
            },
            migration: MigrationConfig {
                report: RadrootsMigrationReport::empty(),
            },
            logging: LoggingConfig {
                filter: "info".into(),
                directory: None,
                stdout: true,
            },
            account: AccountConfig {
                selector: None,
                store_path: PathBuf::from("/tmp/store.json"),
                secrets_dir: PathBuf::from("/tmp/secrets"),
                secret_backend: RadrootsSecretBackend::EncryptedFile,
                secret_fallback: None,
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
                default_fallback: Some("encrypted_file".into()),
                allowed_backends: vec!["host_vault".into(), "encrypted_file".into()],
                host_vault_policy: Some("desktop".into()),
                uses_protected_store: true,
            },
            identity: IdentityConfig {
                path: PathBuf::from("/tmp/default-identity.json"),
            },
            signer: SignerConfig {
                backend: SignerBackend::Local,
            },
            relay: RelayConfig {
                urls: Vec::new(),
                publish_policy: RelayPublishPolicy::Any,
                source: RelayConfigSource::Defaults,
            },
            local: LocalConfig {
                root: PathBuf::from("/tmp/local"),
                replica_db_path: PathBuf::from("/tmp/local/replica.sqlite"),
                backups_dir: PathBuf::from("/tmp/local/backups"),
                exports_dir: PathBuf::from("/tmp/local/exports"),
            },
            myc: MycConfig {
                executable: PathBuf::from("myc"),
            },
            hyf: HyfConfig {
                enabled: hyf_enabled,
                executable: PathBuf::from("hyfd"),
            },
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
                bridge_bearer_token: None,
            },
            capability_bindings: bindings,
        }
    }

    #[test]
    fn write_plane_uses_direct_config_provenance() {
        let view = resolve_write_plane_provider(&sample_config(Vec::new(), false));
        assert_eq!(view.provenance, ProviderProvenance::DirectConfig.as_str());
        assert_eq!(view.target.as_deref(), Some("http://127.0.0.1:7070"));
    }

    #[test]
    fn workflow_uses_explicit_binding_provenance_when_configured() {
        let binding = CapabilityBindingConfig {
            capability_id: "workflow.trade".into(),
            provider_runtime_id: "rhi".into(),
            binding_model: "out_of_process_worker".into(),
            source: CapabilityBindingSource::WorkspaceConfig,
            target_kind: CapabilityBindingTargetKind::ExplicitEndpoint,
            target: "/tmp/rhi".into(),
            managed_account_ref: None,
            signer_session_ref: None,
        };
        let view = resolve_workflow_provider(&sample_config(vec![binding], false));
        assert_eq!(view.provenance, ProviderProvenance::ExplicitBinding.as_str());
        assert_eq!(view.target_kind.as_deref(), Some("explicit_endpoint"));
    }

    #[test]
    fn hyf_uses_direct_config_when_enabled_without_binding() {
        let view = resolve_hyf_provider(&sample_config(Vec::new(), true));
        assert_eq!(view.provenance, ProviderProvenance::DirectConfig.as_str());
        assert_eq!(view.target_kind.as_deref(), Some("direct_config"));
        assert_eq!(view.target.as_deref(), Some("hyfd"));
    }

    #[test]
    fn hyf_binding_remains_visible_when_runtime_is_disabled() {
        let binding = CapabilityBindingConfig {
            capability_id: "inference.hyf_stdio".into(),
            provider_runtime_id: "hyf".into(),
            binding_model: "stdio_service".into(),
            source: CapabilityBindingSource::UserConfig,
            target_kind: CapabilityBindingTargetKind::ExplicitEndpoint,
            target: "bin/hyfd-user".into(),
            managed_account_ref: None,
            signer_session_ref: None,
        };
        let view = resolve_hyf_provider(&sample_config(vec![binding], false));
        assert_eq!(view.state, "disabled");
        assert_eq!(view.provenance, ProviderProvenance::ExplicitBinding.as_str());
        assert_eq!(view.source, "user config [[capability_binding]]");
        assert_eq!(view.target_kind.as_deref(), Some("explicit_endpoint"));
        assert_eq!(view.target.as_deref(), Some("bin/hyfd-user"));
        assert_eq!(view.executable, "bin/hyfd-user");
    }

    #[test]
    fn capability_provider_list_covers_write_workflow_and_hyf() {
        let providers = resolve_capability_providers(&sample_config(Vec::new(), false));
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[0].capability_id, "write_plane.trade_jsonrpc");
        assert_eq!(providers[1].capability_id, "workflow.trade");
        assert_eq!(providers[2].capability_id, "inference.hyf_stdio");
    }
}
