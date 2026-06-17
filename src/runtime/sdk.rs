#![allow(dead_code)]

use std::future::Future;
use std::path::PathBuf;

use radroots_authority::RadrootsLocalEventSigner;
use radroots_nostr::prelude::RadrootsNostrKeys;
use radroots_sdk::{
    RadrootsSdk, RadrootsSdkBuilder, RadrootsSdkError, RadrootsSdkStorageConfig, SdkRelayUrlPolicy,
};
use tokio::runtime::{Builder as TokioRuntimeBuilder, Runtime};

use crate::runtime::RuntimeError;
use crate::runtime::account;
use crate::runtime::config::RuntimeConfig;

const SDK_STORAGE_DIR_NAME: &str = "sdk";

#[derive(Debug, thiserror::Error)]
pub enum CliSdkAdapterError {
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
    #[error("{0}")]
    Sdk(#[from] RadrootsSdkError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliSdkConfig {
    pub storage_root: PathBuf,
    pub relay_url_policy: SdkRelayUrlPolicy,
    pub relay_urls: Vec<String>,
}

impl CliSdkConfig {
    pub fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self {
            storage_root: sdk_storage_root(config),
            relay_url_policy: sdk_relay_url_policy(config),
            relay_urls: config.relay.urls.clone(),
        }
    }

    pub fn builder(&self) -> RadrootsSdkBuilder {
        self.relay_urls.iter().fold(
            RadrootsSdk::builder()
                .storage(RadrootsSdkStorageConfig::Directory(
                    self.storage_root.clone(),
                ))
                .relay_url_policy(self.relay_url_policy),
            |builder, relay_url| builder.relay_url(relay_url.clone()),
        )
    }
}

pub struct CliSdkSession {
    runtime: Runtime,
    sdk: RadrootsSdk,
    config: CliSdkConfig,
}

impl CliSdkSession {
    pub fn connect(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config);
        let runtime = sdk_runtime()?;
        let sdk = runtime.block_on(sdk_config.builder().build())?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn connect_memory(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config);
        let runtime = sdk_runtime()?;
        let sdk = runtime.block_on(memory_builder(&sdk_config).build())?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn sdk(&self) -> &RadrootsSdk {
        &self.sdk
    }

    pub fn config(&self) -> &CliSdkConfig {
        &self.config
    }

    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        self.runtime.block_on(future)
    }
}

pub struct CliSdkLocalSigner {
    account_id: String,
    public_key_hex: String,
    signer: RadrootsLocalEventSigner,
}

impl CliSdkLocalSigner {
    pub fn from_runtime_config(config: &RuntimeConfig) -> Result<Self, RuntimeError> {
        let signing = account::resolve_local_signing_identity(config)?;
        let account_id = signing.account.record.account_id.to_string();
        let public_key_hex = signing
            .account
            .record
            .public_identity
            .public_key_hex
            .clone();
        let keys: RadrootsNostrKeys = signing.identity.into_keys();
        let signer = RadrootsLocalEventSigner::new(keys)
            .map_err(|error| RuntimeError::Config(error.to_string()))?;
        Ok(Self {
            account_id,
            public_key_hex,
            signer,
        })
    }

    pub fn account_id(&self) -> &str {
        self.account_id.as_str()
    }

    pub fn public_key_hex(&self) -> &str {
        self.public_key_hex.as_str()
    }

    pub fn signer(&self) -> &RadrootsLocalEventSigner {
        &self.signer
    }
}

pub fn sdk_storage_root(config: &RuntimeConfig) -> PathBuf {
    config.local.root.join(SDK_STORAGE_DIR_NAME)
}

fn sdk_runtime() -> Result<Runtime, RuntimeError> {
    TokioRuntimeBuilder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            RuntimeError::Config(format!("failed to initialize SDK async runtime: {error}"))
        })
}

fn memory_builder(config: &CliSdkConfig) -> RadrootsSdkBuilder {
    config.relay_urls.iter().fold(
        RadrootsSdk::builder().relay_url_policy(config.relay_url_policy),
        |builder, relay_url| builder.relay_url(relay_url.clone()),
    )
}

pub fn sdk_relay_url_policy(config: &RuntimeConfig) -> SdkRelayUrlPolicy {
    if config
        .relay
        .urls
        .iter()
        .any(|relay_url| relay_url.starts_with("ws://"))
    {
        SdkRelayUrlPolicy::Localhost
    } else {
        SdkRelayUrlPolicy::Public
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use radroots_authority::RadrootsEventSigner;
    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_sdk::{SdkStorageKind, StorageStatusRequest};
    use radroots_secret_vault::RadrootsSecretBackend;
    use tempfile::tempdir;

    use super::*;
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, PublishConfig, PublishMode, PublishModeSource, RelayConfig, RelayConfigSource,
        RelayPublishPolicy, RhiConfig, RpcConfig, SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn maps_runtime_config_to_sdk_builder_inputs() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(
            root.path(),
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()],
        );

        let sdk_config = CliSdkConfig::from_runtime_config(&config);

        assert_eq!(sdk_config.storage_root, config.local.root.join("sdk"));
        assert_eq!(sdk_config.relay_url_policy, SdkRelayUrlPolicy::Public);
        assert_eq!(
            sdk_config.relay_urls,
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
    }

    #[test]
    fn maps_localhost_ws_relays_to_localhost_sdk_policy() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(root.path(), vec!["ws://127.0.0.1:8080".to_owned()]);

        assert_eq!(sdk_relay_url_policy(&config), SdkRelayUrlPolicy::Localhost);
    }

    #[test]
    fn materializes_local_account_signer_for_sdk_workflows() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(root.path(), Vec::new());
        let account = account::create_or_migrate_default_account(&config).expect("create account");

        let signer = CliSdkLocalSigner::from_runtime_config(&config).expect("sdk signer");

        assert_eq!(
            signer.account_id(),
            account.account.record.account_id.as_str()
        );
        assert_eq!(
            signer.public_key_hex(),
            account.account.record.public_identity.public_key_hex
        );
        assert_eq!(
            signer.signer().pubkey().as_str(),
            account.account.record.public_identity.public_key_hex
        );
    }

    #[test]
    fn sdk_session_builds_once_and_runs_async_storage_smoke() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(root.path(), Vec::new());
        let session = CliSdkSession::connect(&config).expect("sdk session");

        let status = session
            .block_on(
                session
                    .sdk()
                    .storage_status(StorageStatusRequest::default()),
            )
            .expect("storage status");

        assert_eq!(session.config().storage_root, config.local.root.join("sdk"));
        assert_eq!(status.storage, SdkStorageKind::Directory);
        assert_eq!(status.event_store.total_events, 0);
        assert_eq!(status.outbox.total_events, 0);
    }

    #[test]
    fn sdk_sources_do_not_import_cli_types() {
        let sdk_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../domains/radroots/sdk/crates/sdk/src");
        let mut files = Vec::new();
        collect_rs_files(sdk_src.as_path(), &mut files);

        for file in files {
            let source = fs::read_to_string(&file).expect("read sdk source");
            assert!(
                !source.contains("radroots_cli"),
                "SDK source imports CLI crate identity in {}",
                file.display()
            );
            assert!(
                !source.contains("domains/radroots/cli"),
                "SDK source references CLI path in {}",
                file.display()
            );
        }
    }

    fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                collect_rs_files(path.as_path(), files);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }

    fn sample_config(root: &Path, relays: Vec<String>) -> RuntimeConfig {
        let data = root.join("data");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Json,
                verbosity: Verbosity::Normal,
                color: false,
                dry_run: false,
            },
            interaction: InteractionConfig {
                input_enabled: false,
                assume_yes: false,
                stdin_tty: false,
                stdout_tty: false,
                prompts_allowed: false,
                confirmations_allowed: false,
            },
            paths: PathsConfig {
                profile: "interactive_user".to_owned(),
                profile_source: "test".to_owned(),
                allowed_profiles: vec!["interactive_user".to_owned(), "repo_local".to_owned()],
                root_source: "test".to_owned(),
                repo_local_root: None,
                repo_local_root_source: None,
                subordinate_path_override_source: "runtime_config".to_owned(),
                app_namespace: "apps/cli".to_owned(),
                shared_accounts_namespace: "shared/accounts".to_owned(),
                shared_identities_namespace: "shared/identities".to_owned(),
                app_config_path: root.join("config/apps/cli/config.toml"),
                workspace_config_path: None,
                app_data_root: data.join("apps/cli"),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
            },
            migration: MigrationConfig {
                report: RadrootsMigrationReport::empty(),
            },
            logging: LoggingConfig {
                filter: "info".to_owned(),
                directory: None,
                stdout: false,
            },
            account: AccountConfig {
                selector: None,
                store_path: data.join("shared/accounts/store.json"),
                secrets_dir: secrets.join("shared/accounts"),
                secret_backend: RadrootsSecretBackend::EncryptedFile,
                secret_fallback: None,
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".to_owned(),
                default_fallback: Some("encrypted_file".to_owned()),
                allowed_backends: vec!["host_vault".to_owned(), "encrypted_file".to_owned()],
                host_vault_policy: Some("desktop".to_owned()),
                uses_protected_store: true,
            },
            identity: IdentityConfig {
                path: secrets.join("shared/identities/default.json"),
            },
            signer: SignerConfig {
                backend: SignerBackend::Local,
            },
            publish: PublishConfig {
                mode: PublishMode::NostrRelay,
                source: PublishModeSource::Defaults,
            },
            relay: RelayConfig {
                urls: relays,
                publish_policy: RelayPublishPolicy::Any,
                source: RelayConfigSource::Flags,
            },
            local: LocalConfig {
                root: data.join("apps/cli/replica"),
                replica_db_path: data.join("apps/cli/replica/replica.sqlite"),
                backups_dir: data.join("apps/cli/replica/backups"),
                exports_dir: data.join("apps/cli/replica/exports"),
            },
            myc: MycConfig {
                executable: PathBuf::from("myc"),
                status_timeout_ms: 2_000,
            },
            hyf: HyfConfig {
                enabled: false,
                executable: PathBuf::from("hyfd"),
            },
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".to_owned(),
                bridge_bearer_token: None,
            },
            rhi: RhiConfig {
                trusted_worker_pubkeys: Vec::new(),
            },
            capability_bindings: Vec::new(),
        }
    }
}
