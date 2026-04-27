#[cfg(not(test))]
use crate::runtime::config::RuntimeConfig;
#[cfg(test)]
use crate::runtime::config::{
    CapabilityBindingInspection, CapabilityBindingInspectionState, INFERENCE_HYF_STDIO_CAPABILITY,
    RuntimeConfig,
};
#[cfg(test)]
use crate::runtime::hyf;

const WRITE_PLANE_UNAVAILABLE_DETAIL: &str = "legacy write-plane provider is unavailable; use seller publish commands with configured direct relays";

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProvenance {
    ExplicitBinding,
    ManagedDefault,
    #[cfg(test)]
    DirectConfig,
    #[cfg(test)]
    Disabled,
    Unavailable,
}

#[cfg(test)]
impl ProviderProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitBinding => "explicit_binding",
            Self::ManagedDefault => "managed_default",
            #[cfg(test)]
            Self::DirectConfig => "direct_config",
            #[cfg(test)]
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
        }
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
pub struct ResolvedWritePlaneTarget {
    pub url: String,
    pub bridge_bearer_token: String,
}

#[cfg(test)]
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

#[cfg(test)]
pub fn resolve_write_plane_provider(config: &RuntimeConfig) -> WritePlaneProviderView {
    let _ = config;
    unavailable_write_plane_view()
}

pub fn resolve_actor_write_plane_target(
    config: &RuntimeConfig,
) -> Result<ResolvedWritePlaneTarget, String> {
    let _ = config;
    Err(WRITE_PLANE_UNAVAILABLE_DETAIL.to_owned())
}

#[cfg(test)]
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

#[cfg(test)]
pub fn resolve_capability_providers(config: &RuntimeConfig) -> Vec<ResolvedProviderView> {
    let hyf = resolve_hyf_provider(config);

    vec![ResolvedProviderView {
        capability_id: INFERENCE_HYF_STDIO_CAPABILITY.to_owned(),
        provider_runtime_id: hyf.provider_runtime_id,
        binding_model: hyf.binding_model,
        state: hyf.state,
        provenance: hyf.provenance,
        source: hyf.source,
        target_kind: hyf.target_kind,
        target: hyf.target,
    }]
}

#[cfg(test)]
fn unavailable_write_plane_view() -> WritePlaneProviderView {
    WritePlaneProviderView {
        provider_runtime_id: "nostr_relay".to_owned(),
        binding_model: "direct_relay_publish".to_owned(),
        state: "unavailable".to_owned(),
        provenance: ProviderProvenance::Unavailable.as_str().to_owned(),
        source: "legacy write-plane provider is not active".to_owned(),
        target_kind: None,
        target: None,
        detail: WRITE_PLANE_UNAVAILABLE_DETAIL.to_owned(),
        bridge_auth_configured: false,
    }
}

#[cfg(test)]
fn inspect_binding(config: &RuntimeConfig, capability_id: &str) -> CapabilityBindingInspection {
    config
        .inspect_capability_bindings()
        .into_iter()
        .find(|binding| binding.capability_id == capability_id)
        .expect("provider capability binding inspection must exist")
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn hyf_target(config: &RuntimeConfig, binding: &CapabilityBindingInspection) -> Option<String> {
    if binding.state == CapabilityBindingInspectionState::Configured {
        return binding.target.clone();
    }
    if config.hyf.enabled {
        return Some(config.hyf.executable.display().to_string());
    }
    None
}

#[cfg(test)]
fn hyf_executable(
    config: &RuntimeConfig,
    binding: &CapabilityBindingInspection,
    status: &hyf::HyfStatusView,
) -> String {
    if binding.state == CapabilityBindingInspectionState::Configured
        && binding.target_kind.as_deref() == Some("explicit_endpoint")
    {
        return binding
            .target
            .clone()
            .unwrap_or_else(|| status.executable.clone());
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
        ProviderProvenance, resolve_actor_write_plane_target, resolve_capability_providers,
        resolve_hyf_provider, resolve_write_plane_provider,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, CapabilityBindingConfig,
        CapabilityBindingSource, CapabilityBindingTargetKind, HyfConfig, IdentityConfig,
        InteractionConfig, LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig,
        OutputFormat, PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig,
        RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };

    fn sample_config(bindings: Vec<CapabilityBindingConfig>, hyf_enabled: bool) -> RuntimeConfig {
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Normal,
                color: true,
                dry_run: false,
            },
            interaction: InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: true,
                stdout_tty: true,
                prompts_allowed: true,
                confirmations_allowed: true,
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
                app_config_path: PathBuf::from("/tmp/config/apps/cli/config.toml"),
                workspace_config_path: None,
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
                status_timeout_ms: 2_000,
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
    fn write_plane_provider_is_not_active_for_direct_relay_publish() {
        let view = resolve_write_plane_provider(&sample_config(Vec::new(), false));
        assert_eq!(view.provider_runtime_id, "nostr_relay");
        assert_eq!(view.binding_model, "direct_relay_publish");
        assert_eq!(view.state, "unavailable");
        assert_eq!(view.provenance, ProviderProvenance::Unavailable.as_str());
        assert!(view.target.is_none());
        assert!(view.detail.contains("seller publish commands"));
    }

    #[test]
    fn actor_write_plane_target_fails_closed() {
        let error = resolve_actor_write_plane_target(&sample_config(Vec::new(), false))
            .expect_err("write plane target");
        assert_eq!(
            error,
            "legacy write-plane provider is unavailable; use seller publish commands with configured direct relays"
        );
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
        assert_eq!(
            view.provenance,
            ProviderProvenance::ExplicitBinding.as_str()
        );
        assert_eq!(view.source, "user config [[capability_binding]]");
        assert_eq!(view.target_kind.as_deref(), Some("explicit_endpoint"));
        assert_eq!(view.target.as_deref(), Some("bin/hyfd-user"));
        assert_eq!(view.executable, "bin/hyfd-user");
    }

    #[test]
    fn capability_provider_list_only_covers_active_hyf_provider() {
        let providers = resolve_capability_providers(&sample_config(Vec::new(), false));
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].capability_id, "inference.hyf_stdio");
    }
}
