use std::path::PathBuf;

use radroots_runtime_manager::{ManagedRuntimeInstallState, load_registry, read_secret_file};
use url::Url;

use crate::runtime::config::{
    CapabilityBindingInspection, CapabilityBindingInspectionState, CapabilityBindingTargetKind,
    INFERENCE_HYF_STDIO_CAPABILITY, RuntimeConfig, WORKFLOW_TRADE_CAPABILITY,
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
pub struct ResolvedWritePlaneTarget {
    pub url: String,
    pub bridge_bearer_token: String,
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
        match (self.state.as_str(), self.target_kind.as_deref(), self.target.as_deref()) {
            ("not_configured", _, _) => {
                "optional workflow provider is not configured; rhi remains status-only in this wave"
                    .to_owned()
            }
            ("unsupported", Some(target_kind), Some(target)) => {
                format!(
                    "configured workflow binding via {} {} is not executable in this wave; rhi remains status-only",
                    target_kind, target
                )
            }
            ("unsupported", _, _) => {
                "configured workflow binding is not executable in this wave; rhi remains status-only"
                    .to_owned()
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum WritePlaneResolution {
    Ready {
        target: ResolvedWritePlaneTarget,
        view: WritePlaneProviderView,
    },
    Unconfigured(WritePlaneProviderView),
}

pub fn resolve_write_plane_provider(config: &RuntimeConfig) -> WritePlaneProviderView {
    match resolve_write_plane_resolution(config) {
        WritePlaneResolution::Ready { view, .. } | WritePlaneResolution::Unconfigured(view) => view,
    }
}

pub fn resolve_actor_write_plane_target(
    config: &RuntimeConfig,
) -> Result<ResolvedWritePlaneTarget, String> {
    match resolve_write_plane_resolution(config) {
        WritePlaneResolution::Ready { target, .. } => Ok(target),
        WritePlaneResolution::Unconfigured(view) => Err(view.detail),
    }
}

pub fn resolve_workflow_provider(config: &RuntimeConfig) -> WorkflowProviderView {
    let binding = inspect_binding(config, WORKFLOW_TRADE_CAPABILITY);
    let (state, provenance) = match binding.state {
        CapabilityBindingInspectionState::Configured => (
            "unsupported".to_owned(),
            ProviderProvenance::ExplicitBinding.as_str().to_owned(),
        ),
        CapabilityBindingInspectionState::Disabled => (
            "disabled".to_owned(),
            ProviderProvenance::Disabled.as_str().to_owned(),
        ),
        CapabilityBindingInspectionState::NotConfigured => (
            "not_configured".to_owned(),
            ProviderProvenance::Unavailable.as_str().to_owned(),
        ),
    };

    WorkflowProviderView {
        provider_runtime_id: binding.provider_runtime_id,
        binding_model: binding.binding_model,
        state,
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

fn resolve_write_plane_resolution(config: &RuntimeConfig) -> WritePlaneResolution {
    if let Some(binding) = config.capability_binding(WRITE_PLANE_TRADE_JSONRPC_CAPABILITY) {
        return resolve_bound_write_plane(config, binding);
    }

    match resolve_managed_write_plane_instance(config, "local") {
        Ok(target) => WritePlaneResolution::Ready {
            view: WritePlaneProviderView {
                provider_runtime_id: "radrootsd".to_owned(),
                binding_model: "daemon_backed_jsonrpc".to_owned(),
                state: "configured".to_owned(),
                provenance: ProviderProvenance::ManagedDefault.as_str().to_owned(),
                source: "managed preferred radrootsd instance".to_owned(),
                target_kind: Some("managed_instance".to_owned()),
                target: Some("local".to_owned()),
                detail: format!(
                    "actor-authored durable writes resolve through managed radrootsd instance `local` at {}",
                    target.url
                ),
                bridge_auth_configured: true,
            },
            target,
        },
        Err(reason) => WritePlaneResolution::Unconfigured(WritePlaneProviderView {
            provider_runtime_id: "radrootsd".to_owned(),
            binding_model: "daemon_backed_jsonrpc".to_owned(),
            state: "unconfigured".to_owned(),
            provenance: ProviderProvenance::Unavailable.as_str().to_owned(),
            source: "no explicit capability binding or managed preferred default".to_owned(),
            target_kind: None,
            target: None,
            detail: reason,
            bridge_auth_configured: false,
        }),
    }
}

fn resolve_bound_write_plane(
    config: &RuntimeConfig,
    binding: &crate::runtime::config::CapabilityBindingConfig,
) -> WritePlaneResolution {
    match binding.target_kind {
        CapabilityBindingTargetKind::ExplicitEndpoint => {
            let target_url = match validate_write_plane_url(binding.target.as_str()) {
                Ok(url) => url,
                Err(reason) => {
                    return WritePlaneResolution::Unconfigured(WritePlaneProviderView {
                        provider_runtime_id: "radrootsd".to_owned(),
                        binding_model: "daemon_backed_jsonrpc".to_owned(),
                        state: "unconfigured".to_owned(),
                        provenance: ProviderProvenance::ExplicitBinding.as_str().to_owned(),
                        source: binding.source.as_str().to_owned(),
                        target_kind: Some(binding.target_kind.as_str().to_owned()),
                        target: Some(binding.target.clone()),
                        detail: reason,
                        bridge_auth_configured: false,
                    });
                }
            };
            let Some(bridge_bearer_token) = config
                .rpc
                .bridge_bearer_token
                .as_deref()
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(ToOwned::to_owned)
            else {
                return WritePlaneResolution::Unconfigured(WritePlaneProviderView {
                    provider_runtime_id: "radrootsd".to_owned(),
                    binding_model: "daemon_backed_jsonrpc".to_owned(),
                    state: "unconfigured".to_owned(),
                    provenance: ProviderProvenance::ExplicitBinding.as_str().to_owned(),
                    source: binding.source.as_str().to_owned(),
                    target_kind: Some(binding.target_kind.as_str().to_owned()),
                    target: Some(binding.target.clone()),
                    detail:
                        "explicit write-plane capability bindings require RADROOTS_RPC_BEARER_TOKEN for actor-authored durable writes"
                            .to_owned(),
                    bridge_auth_configured: false,
                });
            };
            WritePlaneResolution::Ready {
                view: WritePlaneProviderView {
                    provider_runtime_id: "radrootsd".to_owned(),
                    binding_model: "daemon_backed_jsonrpc".to_owned(),
                    state: "configured".to_owned(),
                    provenance: ProviderProvenance::ExplicitBinding.as_str().to_owned(),
                    source: binding.source.as_str().to_owned(),
                    target_kind: Some(binding.target_kind.as_str().to_owned()),
                    target: Some(target_url.clone()),
                    detail: format!(
                        "actor-authored durable writes resolve through explicit write-plane endpoint {}",
                        target_url
                    ),
                    bridge_auth_configured: true,
                },
                target: ResolvedWritePlaneTarget {
                    url: target_url,
                    bridge_bearer_token,
                },
            }
        }
        CapabilityBindingTargetKind::ManagedInstance => {
            match resolve_managed_write_plane_instance(config, binding.target.as_str()) {
                Ok(target) => WritePlaneResolution::Ready {
                    view: WritePlaneProviderView {
                        provider_runtime_id: "radrootsd".to_owned(),
                        binding_model: "daemon_backed_jsonrpc".to_owned(),
                        state: "configured".to_owned(),
                        provenance: ProviderProvenance::ManagedDefault.as_str().to_owned(),
                        source: binding.source.as_str().to_owned(),
                        target_kind: Some(binding.target_kind.as_str().to_owned()),
                        target: Some(binding.target.clone()),
                        detail: format!(
                            "actor-authored durable writes resolve through managed radrootsd instance `{}` at {}",
                            binding.target, target.url
                        ),
                        bridge_auth_configured: true,
                    },
                    target,
                },
                Err(reason) => WritePlaneResolution::Unconfigured(WritePlaneProviderView {
                    provider_runtime_id: "radrootsd".to_owned(),
                    binding_model: "daemon_backed_jsonrpc".to_owned(),
                    state: "unconfigured".to_owned(),
                    provenance: ProviderProvenance::ManagedDefault.as_str().to_owned(),
                    source: binding.source.as_str().to_owned(),
                    target_kind: Some(binding.target_kind.as_str().to_owned()),
                    target: Some(binding.target.clone()),
                    detail: reason,
                    bridge_auth_configured: false,
                }),
            }
        }
    }
}

fn resolve_managed_write_plane_instance(
    config: &RuntimeConfig,
    instance_id: &str,
) -> Result<ResolvedWritePlaneTarget, String> {
    let registry_path = runtime_manager_registry_path(config)?;
    let registry = load_registry(&registry_path).map_err(|err| {
        format!(
            "load runtime-manager registry {}: {err}",
            registry_path.display()
        )
    })?;
    let Some(record) = registry
        .instances
        .iter()
        .find(|record| record.runtime_id == "radrootsd" && record.instance_id == instance_id)
    else {
        return Err(format!(
            "actor-authored durable writes require an explicit write-plane capability binding or managed radrootsd instance `{instance_id}` in {}",
            registry_path.display()
        ));
    };
    if record.install_state != ManagedRuntimeInstallState::Configured {
        return Err(format!(
            "managed radrootsd instance `{instance_id}` is not configured in {}",
            registry_path.display()
        ));
    }
    let Some(health_endpoint) = record
        .health_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(format!(
            "managed radrootsd instance `{instance_id}` is missing health_endpoint in {}",
            registry_path.display()
        ));
    };
    let url = validate_write_plane_url(health_endpoint)?;
    let Some(secret_material_ref) = record
        .secret_material_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(format!(
            "managed radrootsd instance `{instance_id}` is missing secret_material_ref in {}",
            registry_path.display()
        ));
    };
    let bridge_bearer_token = read_secret_file(secret_material_ref).map_err(|err| {
        format!(
            "read managed radrootsd secret material for instance `{instance_id}` at {secret_material_ref}: {err}"
        )
    })?;
    let bridge_bearer_token = bridge_bearer_token.trim().to_owned();
    if bridge_bearer_token.is_empty() {
        return Err(format!(
            "managed radrootsd instance `{instance_id}` has empty secret material at {secret_material_ref}"
        ));
    }
    Ok(ResolvedWritePlaneTarget {
        url,
        bridge_bearer_token,
    })
}

fn runtime_manager_registry_path(config: &RuntimeConfig) -> Result<PathBuf, String> {
    let Some(app_dir) = config.paths.app_config_path.parent() else {
        return Err("resolve cli app config directory for runtime-manager lookup".to_owned());
    };
    let Some(apps_dir) = app_dir.parent() else {
        return Err("resolve cli apps config root for runtime-manager lookup".to_owned());
    };
    let Some(config_root) = apps_dir.parent() else {
        return Err("resolve cli config root for runtime-manager lookup".to_owned());
    };
    Ok(config_root.join("shared/runtime-manager/instances.toml"))
}

fn validate_write_plane_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("write-plane endpoint must not be empty".to_owned());
    }
    let parsed = Url::parse(trimmed)
        .map_err(|err| format!("write-plane endpoint `{trimmed}` is invalid: {err}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(format!(
            "write-plane endpoint must use http or https, got `{trimmed}`"
        ));
    }
    Ok(trimmed.to_owned())
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
    use std::fs;
    use std::path::PathBuf;

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use tempfile::tempdir;

    use super::{
        ProviderProvenance, resolve_actor_write_plane_target, resolve_capability_providers,
        resolve_hyf_provider, resolve_workflow_provider, resolve_write_plane_provider,
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
                app_config_path: PathBuf::from("/tmp/config/apps/cli/config.toml"),
                workspace_config_path: PathBuf::from("/tmp/workspace/.radroots/config.toml"),
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
    fn write_plane_requires_authoritative_binding_or_managed_default() {
        let view = resolve_write_plane_provider(&sample_config(Vec::new(), false));
        assert_eq!(view.state, "unconfigured");
        assert_eq!(view.provenance, ProviderProvenance::Unavailable.as_str());
        assert!(view.target.is_none());
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
        assert_eq!(view.state, "unsupported");
        assert_eq!(
            view.provenance,
            ProviderProvenance::ExplicitBinding.as_str()
        );
        assert_eq!(view.target_kind.as_deref(), Some("explicit_endpoint"));
    }

    #[test]
    fn explicit_write_plane_binding_requires_bridge_bearer_auth() {
        let binding = CapabilityBindingConfig {
            capability_id: "write_plane.trade_jsonrpc".into(),
            provider_runtime_id: "radrootsd".into(),
            binding_model: "daemon_backed_jsonrpc".into(),
            source: CapabilityBindingSource::WorkspaceConfig,
            target_kind: CapabilityBindingTargetKind::ExplicitEndpoint,
            target: "https://rpc.workspace.test".into(),
            managed_account_ref: None,
            signer_session_ref: None,
        };
        let view = resolve_write_plane_provider(&sample_config(vec![binding], false));
        assert_eq!(view.state, "unconfigured");
        assert_eq!(
            view.provenance,
            ProviderProvenance::ExplicitBinding.as_str()
        );
        assert_eq!(view.target.as_deref(), Some("https://rpc.workspace.test"));
    }

    #[test]
    fn managed_default_write_plane_uses_runtime_manager_registry() {
        let dir = tempdir().expect("tempdir");
        let config_dir = dir.path().join("config");
        let app_config_path = config_dir.join("apps/cli/config.toml");
        fs::create_dir_all(app_config_path.parent().expect("app config parent"))
            .expect("create app config dir");
        fs::write(&app_config_path, "").expect("write app config");

        let registry_path = config_dir.join("shared/runtime-manager/instances.toml");
        fs::create_dir_all(registry_path.parent().expect("registry parent"))
            .expect("create registry parent");
        let managed_config_path = dir.path().join("radrootsd-config.toml");
        let bridge_token_path = dir.path().join("bridge-token.txt");
        fs::write(
            &managed_config_path,
            "[metadata]\nname = \"managed-radrootsd\"\n",
        )
        .expect("write managed config");
        fs::write(&bridge_token_path, "managed-bridge-token").expect("write token");
        fs::write(
            &registry_path,
            format!(
                r#"schema = "radroots_runtime-instance-registry"
schema_version = 1

[[instances]]
runtime_id = "radrootsd"
instance_id = "local"
management_mode = "interactive_user_managed"
install_state = "configured"
binary_path = "/tmp/radrootsd"
config_path = "{}"
logs_path = "/tmp/logs"
run_path = "/tmp/run"
installed_version = "0.1.0"
health_endpoint = "http://127.0.0.1:7444"
secret_material_ref = "{}"
"#,
                managed_config_path.display(),
                bridge_token_path.display()
            ),
        )
        .expect("write registry");

        let mut config = sample_config(Vec::new(), false);
        config.paths.app_config_path = app_config_path;

        let view = resolve_write_plane_provider(&config);
        assert_eq!(view.state, "configured");
        assert_eq!(view.provenance, ProviderProvenance::ManagedDefault.as_str());
        assert_eq!(view.target_kind.as_deref(), Some("managed_instance"));
        assert_eq!(view.target.as_deref(), Some("local"));

        let target =
            resolve_actor_write_plane_target(&config).expect("resolve actor write plane target");
        assert_eq!(target.url, "http://127.0.0.1:7444");
        assert_eq!(target.bridge_bearer_token, "managed-bridge-token");
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
    fn capability_provider_list_covers_write_workflow_and_hyf() {
        let providers = resolve_capability_providers(&sample_config(Vec::new(), false));
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[0].capability_id, "write_plane.trade_jsonrpc");
        assert_eq!(providers[1].capability_id, "workflow.trade");
        assert_eq!(providers[2].capability_id, "inference.hyf_stdio");
    }
}
