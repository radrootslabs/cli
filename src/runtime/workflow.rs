use crate::runtime::config::{RuntimeConfig, WORKFLOW_TRADE_CAPABILITY};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProviderStatusView {
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: String,
    pub source: String,
    pub target_kind: Option<String>,
    pub target: Option<String>,
    pub hyf_helper_state: String,
    pub hyf_helper_detail: String,
}

impl WorkflowProviderStatusView {
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

pub fn resolve_workflow_provider(config: &RuntimeConfig) -> WorkflowProviderStatusView {
    let binding = config
        .inspect_capability_bindings()
        .into_iter()
        .find(|binding| binding.capability_id == WORKFLOW_TRADE_CAPABILITY)
        .expect("workflow.trade binding inspection must exist");

    WorkflowProviderStatusView {
        provider_runtime_id: binding.provider_runtime_id,
        binding_model: binding.binding_model,
        state: binding.state.as_str().to_owned(),
        source: binding.source,
        target_kind: binding.target_kind,
        target: binding.target,
        hyf_helper_state: "not_implied".to_owned(),
        hyf_helper_detail:
            "cli bindings do not imply an rhi -> hyf helper path; any worker helper remains explicit and optional"
                .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;

    use super::resolve_workflow_provider;
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, CapabilityBindingConfig,
        CapabilityBindingSource, CapabilityBindingTargetKind, HyfConfig, IdentityConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    fn sample_config(workflow_binding: Option<CapabilityBindingConfig>) -> RuntimeConfig {
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
                enabled: false,
                executable: PathBuf::from("hyfd"),
            },
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
                bridge_bearer_token: None,
            },
            capability_bindings: workflow_binding.into_iter().collect(),
        }
    }

    #[test]
    fn workflow_provider_reports_not_configured_without_binding() {
        let view = resolve_workflow_provider(&sample_config(None));
        assert_eq!(view.provider_runtime_id, "rhi");
        assert_eq!(view.binding_model, "out_of_process_worker");
        assert_eq!(view.state, "not_configured");
        assert_eq!(view.source, "no explicit capability binding");
        assert_eq!(view.hyf_helper_state, "not_implied");
    }

    #[test]
    fn workflow_provider_reports_explicit_binding_details() {
        let binding = CapabilityBindingConfig {
            capability_id: "workflow.trade".into(),
            provider_runtime_id: "rhi".into(),
            binding_model: "out_of_process_worker".into(),
            source: CapabilityBindingSource::WorkspaceConfig,
            target_kind: CapabilityBindingTargetKind::ExplicitEndpoint,
            target: "/tmp/rhi-binary".into(),
            managed_account_ref: None,
            signer_session_ref: None,
        };
        let view = resolve_workflow_provider(&sample_config(Some(binding)));
        assert_eq!(view.state, "configured");
        assert_eq!(view.target_kind.as_deref(), Some("explicit_endpoint"));
        assert_eq!(view.target.as_deref(), Some("/tmp/rhi-binary"));
        assert!(view.detail().contains("explicit_endpoint /tmp/rhi-binary"));
    }
}
