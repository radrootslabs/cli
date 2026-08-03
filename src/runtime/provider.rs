use crate::runtime::config::RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE;
#[cfg(test)]
use crate::runtime::config::{
    CapabilityBindingInspection, CapabilityBindingInspectionState, INFERENCE_HYF_STDIO_CAPABILITY,
};
use crate::runtime::config::{RuntimeConfig, TransportProfileKind};
#[cfg(test)]
use crate::runtime::hyf;
use crate::view::runtime::PublishRuntimeView;

#[cfg(test)]
const WRITE_PLANE_TARGET_DETAIL: &str =
    "write-plane targets are resolved by mode-specific publish commands";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProvenance {
    #[cfg(test)]
    ExplicitBinding,
    #[cfg(test)]
    ManagedDefault,
    #[cfg(test)]
    DirectConfig,
    #[cfg(test)]
    Disabled,
    TransportProfile,
    #[cfg(test)]
    Unavailable,
}

impl ProviderProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            #[cfg(test)]
            Self::ExplicitBinding => "explicit_binding",
            #[cfg(test)]
            Self::ManagedDefault => "managed_default",
            #[cfg(test)]
            Self::DirectConfig => "direct_config",
            #[cfg(test)]
            Self::Disabled => "disabled",
            Self::TransportProfile => "transport_profile",
            #[cfg(test)]
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
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWritePlaneTarget {
    pub url: String,
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

pub fn resolve_write_plane_provider(
    config: &RuntimeConfig,
    publish: &PublishRuntimeView,
) -> WritePlaneProviderView {
    let (provider_runtime_id, binding_model, detail) = match config.transport.profile {
        TransportProfileKind::Nostr | TransportProfileKind::MultiTarget => (
            config.transport.profile.as_str(),
            "nostr_transport",
            "configured transport profile publishes through Nostr; readiness is reported under publish",
        ),
        TransportProfileKind::LocalOnly => (
            "local_only",
            "local_transport",
            "local_only transport profile does not provide network publish",
        ),
        TransportProfileKind::Reticulum => (
            "reticulum",
            "reticulum_transport",
            RADROOTS_RETICULUM_UNAVAILABLE_MESSAGE,
        ),
    };
    WritePlaneProviderView {
        provider_runtime_id: provider_runtime_id.to_owned(),
        binding_model: binding_model.to_owned(),
        state: publish.state.clone(),
        provenance: ProviderProvenance::TransportProfile.as_str().to_owned(),
        source: publish.source.clone(),
        target_kind: None,
        target: None,
        detail: publish.reason.clone().unwrap_or_else(|| detail.to_owned()),
    }
}

#[cfg(test)]
pub fn resolve_actor_write_plane_target(
    config: &RuntimeConfig,
) -> Result<ResolvedWritePlaneTarget, String> {
    let _ = config;
    Err(WRITE_PLANE_TARGET_DETAIL.to_owned())
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
    use radroots_secret_vault::RadrootsSecretBackend;
    use std::path::PathBuf;

    use super::{
        ProviderProvenance, resolve_actor_write_plane_target, resolve_capability_providers,
        resolve_hyf_provider, resolve_write_plane_provider,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, CapabilityBindingConfig,
        CapabilityBindingSource, CapabilityBindingTargetKind, HyfConfig, IdentityConfig,
        InteractionConfig, LocalConfig, LoggingConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RpcConfig, RuntimeConfig, SignerBackend, SignerConfig, Verbosity,
    };
    use crate::view::runtime::{
        PublishProviderRuntimeView, PublishRelayRuntimeView, PublishRuntimeView,
    };

    fn sample_config(bindings: Vec<CapabilityBindingConfig>, hyf_enabled: bool) -> RuntimeConfig {
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Terminal,
                verbosity: Verbosity::Normal,
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
                shared_cache_root: PathBuf::from("/tmp/cache"),
                app_logs_root: PathBuf::from("/tmp/logs"),
                shared_accounts_data_root: PathBuf::from("/tmp/shared/accounts"),
                shared_accounts_secrets_root: PathBuf::from("/tmp/shared/accounts-secrets"),
                default_identity_path: PathBuf::from("/tmp/default-identity.json"),
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
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
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
            transport: crate::runtime::config::TransportConfig::local_only(),
            local: LocalConfig {
                root: PathBuf::from("/tmp/local"),
                replica_store_path: PathBuf::from("/tmp/local/replica.sqlite"),
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
            mesh: crate::runtime::config::MeshConfig::disabled(),
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
            },
            rhi: crate::runtime::config::RhiConfig {
                validator_set: None,
                require_cryptographic_proof: false,
            },
            capability_bindings: bindings,
        }
    }

    fn publish_view(
        config: &RuntimeConfig,
        state: &str,
        reason: Option<&str>,
    ) -> PublishRuntimeView {
        PublishRuntimeView {
            transport: config.transport.profile.as_str().to_owned(),
            source: config.transport.source.as_str().to_owned(),
            transport_family: config.transport.profile.transport_family().to_owned(),
            state: state.to_owned(),
            executable: state == "ready",
            reason: reason.map(str::to_owned),
            signed_write_required: true,
            relay: PublishRelayRuntimeView {
                ready: !config.transport.nostr_relay_urls.is_empty(),
                count: config.transport.nostr_relay_urls.len(),
                source: config.transport.source.as_str().to_owned(),
            },
            provider: PublishProviderRuntimeView {
                provider_runtime_id: config.transport.profile.as_str().to_owned(),
                state: state.to_owned(),
                source: config.transport.source.as_str().to_owned(),
                reason: reason.map(str::to_owned),
            },
        }
    }

    #[test]
    fn write_plane_provider_tracks_nostr_transport_publish() {
        let mut config = sample_config(Vec::new(), false);
        config.transport = crate::runtime::config::TransportConfig::from_nostr_relay_urls(vec![
            "wss://relay.example.com".to_owned(),
        ]);
        let publish = publish_view(
            &config,
            "ready",
            Some("Nostr relay transport is configured for delivery"),
        );
        let view = resolve_write_plane_provider(&config, &publish);
        assert_eq!(view.provider_runtime_id, "nostr");
        assert_eq!(view.binding_model, "nostr_transport");
        assert_eq!(view.state, "ready");
        assert_eq!(
            view.provenance,
            ProviderProvenance::TransportProfile.as_str()
        );
        assert!(view.target.is_none());
        assert!(view.detail.contains("configured for delivery"));
    }

    #[test]
    fn actor_write_plane_target_fails_closed() {
        let error = resolve_actor_write_plane_target(&sample_config(Vec::new(), false))
            .expect_err("write plane target");
        assert_eq!(
            error,
            "write-plane targets are resolved by mode-specific publish commands"
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
