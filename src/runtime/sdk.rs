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

pub(crate) fn sdk_runtime() -> Result<Runtime, RuntimeError> {
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
    use std::collections::BTreeSet;
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

    struct DirectRrRsDependency {
        section: &'static str,
        name: &'static str,
        owner: &'static str,
        reason: &'static str,
        lifecycle: &'static str,
    }

    struct LegacyDirectRelayConsumer {
        path: &'static str,
        required_tokens: &'static [&'static str],
        owner: &'static str,
        reason: &'static str,
        lifecycle: &'static str,
    }

    struct MigratedCliPathGuard {
        label: &'static str,
        path: &'static str,
        start: &'static str,
        end: &'static str,
        required_tokens: &'static [&'static str],
    }

    const DIRECT_RR_RS_DEPENDENCIES: &[DirectRrRsDependency] = &[
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_authority",
            owner: "cli-sdk-adapter",
            reason: "local account signer materialization for SDK and remaining CLI-authored signing",
            lifecycle: "retain until all signed mutation construction moves behind SDK signer requests",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_core",
            owner: "cli-drafts-and-rendering",
            reason: "CLI draft parsing, numeric validation, and display DTOs",
            lifecycle: "retain while CLI owns TOML draft UX and command rendering",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_events",
            owner: "cli-drafts-and-non-migrated-workflows",
            reason: "event DTOs for local drafts, views, relay reads, and validation receipt surfaces",
            lifecycle: "retain until the remaining event-authoring and inspection surfaces migrate",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_events_codec",
            owner: "cli-drafts-and-non-migrated-workflows",
            reason: "event encoding and decoding for farm, listing draft, order, sync pull, and validation inspection",
            lifecycle: "retain until those command families are SDK-backed",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_identity",
            owner: "cli-account-and-signer-ux",
            reason: "account identity views, local signer materialization, and direct-relay workflows outside the migrated paths",
            lifecycle: "retain while CLI owns account selection and local identity custody UX",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_local_events",
            owner: "cli-app-interop",
            reason: "shared local work and signed-event interop with the desktop app",
            lifecycle: "retain until a shared local-events SDK boundary replaces direct CLI access",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_log",
            owner: "cli-runtime-shell",
            reason: "CLI logging initialization and file layout",
            lifecycle: "permanent CLI runtime ownership",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_nostr",
            owner: "non-migrated-direct-relay-workflows",
            reason: "direct relay fetch/publish and event conversion for active non-migrated commands",
            lifecycle: "retain until direct relay command families migrate or are retired",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_nostr_accounts",
            owner: "cli-account-store",
            reason: "CLI account selection, import, local signer status, and account persistence",
            lifecycle: "retain while CLI owns local account UX and storage",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_nostr_signer",
            owner: "cli-signer-readiness",
            reason: "signer readiness reporting for active mutation command surfaces",
            lifecycle: "retain until signer readiness is fully SDK-owned",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_replica_db",
            owner: "legacy-replica-and-market-projection",
            reason: "legacy derived replica status, export, market reads, sync pull, basket lookup, and order draft preflight",
            lifecycle: "transitional until those derived projection surfaces migrate",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_replica_db_schema",
            owner: "legacy-replica-and-market-projection",
            reason: "typed query filters for legacy market, basket, and order lookup projections",
            lifecycle: "transitional until those derived projection surfaces migrate",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_replica_sync",
            owner: "legacy-sync-pull-and-derived-replica",
            reason: "legacy relay ingest, sync pull, market refresh, and derived replica state reporting",
            lifecycle: "transitional until relay ingest and projection repair move behind SDK APIs",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_runtime",
            owner: "cli-config",
            reason: "strict environment and config value parsing",
            lifecycle: "permanent CLI configuration ownership unless a shared runtime config crate replaces it",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_runtime_paths",
            owner: "cli-runtime-paths",
            reason: "profile-aware CLI config, data, logs, and secrets path resolution",
            lifecycle: "permanent CLI runtime ownership",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_secret_vault",
            owner: "cli-account-store",
            reason: "local account secret backend selection and readiness",
            lifecycle: "retain while CLI owns local account custody UX",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_sp1_host_trade",
            owner: "validation-receipts",
            reason: "validation receipt SP1 proof inspection and verification",
            lifecycle: "retain until validation receipt verification moves behind SDK APIs",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_sql_core",
            owner: "legacy-replica-and-local-events",
            reason: "SQLite executor for legacy derived replica and shared local-events storage",
            lifecycle: "transitional until those storage surfaces move behind SDK or shared runtime APIs",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_trade",
            owner: "cli-drafts-and-validation",
            reason: "listing draft validation, order economics, order reducer helpers, and validation receipt parsing",
            lifecycle: "retain until remaining trade validation and draft behavior migrates",
        },
        DirectRrRsDependency {
            section: "dev-dependencies",
            name: "radroots_protected_store",
            owner: "account-tests",
            reason: "unit coverage for protected file secret vault behavior",
            lifecycle: "test-only",
        },
    ];

    const LEGACY_DIRECT_RELAY_CONSUMERS: &[LegacyDirectRelayConsumer] = &[
        LegacyDirectRelayConsumer {
            path: "src/runtime/farm.rs",
            required_tokens: &[
                "publish_via_direct_relay(",
                "publish_signed_event_with_identity",
            ],
            owner: "farm.publish",
            reason: "non-migrated farm publish direct relay write mode",
            lifecycle: "retain until farm publish migrates to SDK-backed write APIs",
        },
        LegacyDirectRelayConsumer {
            path: "src/runtime/listing.rs",
            required_tokens: &[
                "mutate_via_direct_relay(",
                "publish_signed_event_with_identity",
            ],
            owner: "listing.nostr_relay.write",
            reason: "non-migrated listing direct relay write mode outside SDK local publish",
            lifecycle: "retain until listing relay publish migrates to SDK-backed write APIs",
        },
        LegacyDirectRelayConsumer {
            path: "src/runtime/local_events.rs",
            required_tokens: &["DirectRelayFailure", "DirectRelayPublishError"],
            owner: "local-event.delivery-evidence",
            reason: "delivery evidence mapping for non-migrated direct relay publish outcomes",
            lifecycle: "retain until delivery evidence moves behind SDK or local-events APIs",
        },
        LegacyDirectRelayConsumer {
            path: "src/runtime/order.rs",
            required_tokens: &["fetch_events_from_relays", "publish_parts_with_identity"],
            owner: "order.lifecycle.preflight-and-mutations",
            reason: "non-migrated order lifecycle preflight reads and mutation writes",
            lifecycle: "retain until full order lifecycle behavior migrates to SDK APIs",
        },
        LegacyDirectRelayConsumer {
            path: "src/runtime/sync.rs",
            required_tokens: &["fetch_events_from_relays", "pull_with_fetcher"],
            owner: "sync.pull-and-market-refresh",
            reason: "non-migrated relay ingest into the legacy derived replica",
            lifecycle: "retain until relay ingest and derived projection repair migrate to SDK APIs",
        },
        LegacyDirectRelayConsumer {
            path: "src/runtime/validation_receipt.rs",
            required_tokens: &["fetch_events_from_relays", "DirectRelayFetchReceipt"],
            owner: "validation.receipt.relay-reads",
            reason: "non-migrated validation receipt relay inspection",
            lifecycle: "retain until validation receipt inspection migrates to SDK APIs",
        },
    ];

    const MIGRATED_CLI_PATH_GUARDS: &[MigratedCliPathGuard] = &[
        MigratedCliPathGuard {
            label: "listing publish",
            path: "src/runtime/listing.rs",
            start: "pub fn publish_via_sdk(",
            end: "fn sdk_listing_publish_input(",
            required_tokens: &[
                "session.sdk().listings().prepare_publish",
                "session.sdk().listings().enqueue_publish",
                "session.sdk().sync().push_outbox",
            ],
        },
        MigratedCliPathGuard {
            label: "sync status",
            path: "src/runtime/sync.rs",
            start: "pub fn status(config: &RuntimeConfig) -> Result<SyncStatusView, CliSdkAdapterError>",
            end: "pub fn pull(",
            required_tokens: &["session.sdk().sync().status"],
        },
        MigratedCliPathGuard {
            label: "sync push",
            path: "src/runtime/sync.rs",
            start: "pub fn push(config: &RuntimeConfig) -> Result<SyncActionView, CliSdkAdapterError>",
            end: "pub fn watch(",
            required_tokens: &["session.sdk().sync().push_outbox", "PushOutboxRequest::new"],
        },
        MigratedCliPathGuard {
            label: "order status",
            path: "src/runtime/order.rs",
            start: "pub fn status(\n    config: &RuntimeConfig",
            end: "fn relay_status(",
            required_tokens: &["OrderStatusRequest::parse", "session.sdk().orders().status"],
        },
        MigratedCliPathGuard {
            label: "store status",
            path: "src/runtime/store.rs",
            start: "pub fn status(config: &RuntimeConfig) -> Result<LocalStatusView, CliSdkAdapterError>",
            end: "fn legacy_replica_status(",
            required_tokens: &[
                "session.sdk()",
                "storage_status(StorageStatusRequest::new())",
                "integrity(IntegrityRequest::new())",
            ],
        },
        MigratedCliPathGuard {
            label: "store backup",
            path: "src/runtime/store.rs",
            start: "pub fn backup(\n    config: &RuntimeConfig",
            end: "pub fn backup_preflight(",
            required_tokens: &["session.sdk().backup", "BackupRequest"],
        },
        MigratedCliPathGuard {
            label: "store backup preflight",
            path: "src/runtime/store.rs",
            start: "pub fn backup_preflight(",
            end: "pub fn restore(",
            required_tokens: &[
                "storage_status(StorageStatusRequest::new())",
                "integrity(IntegrityRequest::new())",
            ],
        },
        MigratedCliPathGuard {
            label: "store restore",
            path: "src/runtime/store.rs",
            start: "pub fn restore(",
            end: "pub fn export(",
            required_tokens: &[
                "RestoreRequest::new",
                "sdk_runtime()",
                "RadrootsSdk::restore",
            ],
        },
    ];

    const MIGRATED_PATH_DISALLOWED_TOKENS: &[&str] = &[
        "fetch_events_from_relays",
        "publish_parts_with_identity",
        "publish_via_direct_relay",
        "mutate_via_direct_relay",
        "radroots_replica_pending_publish",
        "radroots_replica_pending_publish_batch",
        "radroots_replica_sync_status",
        "ReplicaSql::new",
        "SqliteExecutor::open(&config.local.replica_db_path)",
        "outbox_idempotency_digest",
        "canonical_target_relays",
    ];

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
            .block_on(session.sdk().storage_status(StorageStatusRequest::new()))
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
        let forbidden = [
            ("radroots_cli", "CLI crate identity"),
            ("domains/radroots/cli", "CLI mount path"),
            ("approval_token", "CLI approval-token UX"),
            ("OutputEnvelope", "CLI output envelope"),
            ("next_actions", "CLI next-action rendering"),
            ("exit_code", "CLI exit-code contract"),
            ("docs/", "repository docs path"),
            ("radroots store", "CLI command string"),
            ("radroots sync", "CLI command string"),
            ("radroots listing", "CLI command string"),
            ("radroots order", "CLI command string"),
        ];

        for file in files {
            let source = fs::read_to_string(&file).expect("read sdk source");
            for (needle, description) in forbidden {
                assert!(
                    !source.contains(needle),
                    "SDK source contains {description} `{needle}` in {}",
                    file.display()
                );
            }
        }
    }

    #[test]
    fn cli_direct_rr_rs_dependencies_are_classified() {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let manifest = fs::read_to_string(&manifest_path).expect("read manifest");
        let manifest = manifest.parse::<toml::Value>().expect("parse manifest");
        let actual = direct_rr_rs_dependency_keys(&manifest);
        let expected = DIRECT_RR_RS_DEPENDENCIES
            .iter()
            .map(direct_rr_rs_dependency_key)
            .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
        for dependency in DIRECT_RR_RS_DEPENDENCIES {
            assert!(!dependency.owner.trim().is_empty());
            assert!(!dependency.reason.trim().is_empty());
            assert!(!dependency.lifecycle.trim().is_empty());
        }
    }

    #[test]
    fn legacy_direct_relay_consumers_are_explicitly_allowlisted() {
        let actual = legacy_direct_relay_consumer_paths();
        let expected = LEGACY_DIRECT_RELAY_CONSUMERS
            .iter()
            .map(|consumer| consumer.path.to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
        for consumer in LEGACY_DIRECT_RELAY_CONSUMERS {
            let source = crate_source(consumer.path);
            for token in consumer.required_tokens {
                assert!(
                    source.contains(token),
                    "{} does not contain legacy direct-relay token `{token}`",
                    consumer.path
                );
            }
            assert!(!consumer.owner.trim().is_empty());
            assert!(!consumer.reason.trim().is_empty());
            assert!(!consumer.lifecycle.trim().is_empty());
        }
    }

    #[test]
    fn migrated_cli_paths_are_guarded_against_direct_relay_and_legacy_canonical_use() {
        for guard in MIGRATED_CLI_PATH_GUARDS {
            let source = crate_source(guard.path);
            assert_migrated_path(
                guard.label,
                source_segment(&source, guard.start, guard.end),
                guard.required_tokens,
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

    fn direct_rr_rs_dependency_keys(manifest: &toml::Value) -> BTreeSet<String> {
        ["dependencies", "dev-dependencies"]
            .into_iter()
            .flat_map(|section| {
                manifest
                    .get(section)
                    .and_then(toml::Value::as_table)
                    .into_iter()
                    .flat_map(move |dependencies| {
                        dependencies.iter().filter_map(move |(name, value)| {
                            dependency_path(value)
                                .filter(|path| path.contains("domains/radroots/lib/crates"))
                                .map(|_| format!("{section}:{name}"))
                        })
                    })
            })
            .collect()
    }

    fn direct_rr_rs_dependency_key(dependency: &DirectRrRsDependency) -> String {
        format!("{}:{}", dependency.section, dependency.name)
    }

    fn legacy_direct_relay_consumer_paths() -> BTreeSet<String> {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_rs_files(manifest_dir.join("src/runtime").as_path(), &mut files);
        files
            .into_iter()
            .filter(|file| {
                !matches!(
                    file.file_name().and_then(|name| name.to_str()),
                    Some("direct_relay.rs" | "sdk.rs")
                )
            })
            .filter_map(|file| {
                let source = fs::read_to_string(&file).expect("read runtime source");
                source
                    .contains("use crate::runtime::direct_relay")
                    .then(|| relative_source_path(manifest_dir, file.as_path()))
            })
            .collect()
    }

    fn relative_source_path(root: &Path, path: &Path) -> String {
        path.strip_prefix(root)
            .expect("source path under manifest root")
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn dependency_path(value: &toml::Value) -> Option<&str> {
        value
            .as_table()
            .and_then(|table| table.get("path"))
            .and_then(toml::Value::as_str)
    }

    fn crate_source(path: &str) -> String {
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).expect("read source")
    }

    fn source_segment<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start_index = source.find(start).expect("source segment start");
        let end_index = source[start_index..]
            .find(end)
            .map(|index| start_index + index)
            .expect("source segment end");
        &source[start_index..end_index]
    }

    fn assert_migrated_path(label: &str, source: &str, required_tokens: &[&str]) {
        for token in required_tokens {
            assert!(
                source.contains(token),
                "{label} does not contain required SDK token `{token}`"
            );
        }

        for token in MIGRATED_PATH_DISALLOWED_TOKENS {
            assert!(
                !source.contains(token),
                "{label} contains disallowed migrated-path token `{token}`"
            );
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
