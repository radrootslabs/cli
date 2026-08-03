//! CLI-owned composition of the final SDK capability graph.

use std::{
    fs,
    future::Future,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use radroots_sdk::{Client, ClientBuilder, Error as SdkError};
use radroots_signing::{
    SignReceipt, SignRequest, Signer, SignerStatus, signer::BoxFuture as SigningFuture,
};
use radroots_storage::{event::SourceGeneration, memory::MemoryStorage};
use radroots_storage_sqlite::{OpenMode, OpenOptions, Paths, SqliteStorage};
use radroots_sync::policy::{Clock, DeadlinePolicy, IdSource, OperationKind, SyncId, SyncStorage};
use radroots_transport::{
    BoxFuture, DeliveryReceipt, DeliveryRequest, Error as TransportError, EventSink, EventSource,
    FetchPage, FetchRequest, SinkStatus, SourceStatus, TransportId,
    capability::{Availability, Maturity, SinkCapabilities, SourceCapabilities},
};
use radroots_transport_nostr::{Config as NostrConfig, NostrTransport, RelayUrlPolicy};
use tokio::runtime::{Builder as TokioRuntimeBuilder, Runtime};

use crate::runtime::{
    RuntimeError,
    config::{RuntimeConfig, TransportProfileKind},
    signing,
};

const SDK_STORAGE_DIR_NAME: &str = "sdk";
const SYNC_TIMEOUT_MS: u64 = 30_000;
const LOCAL_TRANSPORT_MESSAGE: &str =
    "local-only transport is configured without network fetch or delivery";

pub(crate) use signing::{MYC_NIP46_SESSION_SECRET_SERVICE, myc_managed_account_ref_matches};

#[derive(Debug, thiserror::Error)]
pub enum CliSdkAdapterError {
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
    #[error("{0}")]
    Sdk(#[from] SdkError),
    #[error("{0}")]
    Sync(#[from] radroots_sync::Error),
    #[error("{0}")]
    Storage(#[from] radroots_storage_sqlite::Error),
    #[error("{0}")]
    Transport(#[from] radroots_transport_nostr::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliSdkConfig {
    pub storage_root: PathBuf,
}

impl CliSdkConfig {
    pub fn from_runtime_config(config: &RuntimeConfig) -> Self {
        Self {
            storage_root: sdk_storage_root(config),
        }
    }

    fn sqlite_options(&self) -> Result<OpenOptions, CliSdkAdapterError> {
        fs::create_dir_all(&self.storage_root)?;
        let paths = Paths::from_directory(&self.storage_root)?;
        let mut options = OpenOptions::new(paths, OpenMode::Create);
        if !self.storage_root.join("runtime.sqlite").exists() {
            options = options.with_source_generation(new_source_generation()?, now_unix_ms()?)?;
        }
        Ok(options)
    }
}

pub struct CliSdkSession {
    runtime: Runtime,
    sdk: Client,
    config: CliSdkConfig,
}

impl CliSdkSession {
    pub fn connect(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        Self::connect_inner(config, None, false)
    }

    pub fn connect_storage_status(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        Self::connect(config)
    }

    pub fn connect_memory(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        Self::connect_inner(config, None, true)
    }

    pub fn connect_for_actor(
        config: &RuntimeConfig,
        actor_account_id: Option<&str>,
        actor_pubkey: &str,
        actor_label: &str,
    ) -> Result<Self, CliSdkAdapterError> {
        let runtime = sdk_runtime()?;
        let provider = runtime.block_on(signing::provider_for_actor(
            config,
            actor_account_id,
            actor_pubkey,
            actor_label,
        ))?;
        Self::compose(config, runtime, Some(provider), false)
    }

    pub fn connect_memory_for_actor(
        config: &RuntimeConfig,
        actor_account_id: Option<&str>,
        actor_pubkey: &str,
        actor_label: &str,
    ) -> Result<Self, CliSdkAdapterError> {
        let runtime = sdk_runtime()?;
        let provider = runtime.block_on(signing::provider_for_actor(
            config,
            actor_account_id,
            actor_pubkey,
            actor_label,
        ))?;
        Self::compose(config, runtime, Some(provider), true)
    }

    fn connect_inner(
        config: &RuntimeConfig,
        signer: Option<radroots_sdk::signing::Provider>,
        memory: bool,
    ) -> Result<Self, CliSdkAdapterError> {
        Self::compose(config, sdk_runtime()?, signer, memory)
    }

    fn compose(
        config: &RuntimeConfig,
        runtime: Runtime,
        signer: Option<radroots_sdk::signing::Provider>,
        memory: bool,
    ) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config);
        if memory {
            let storage = Arc::new(MemoryStorage::new(new_source_generation()?));
            Self::compose_with_storage(config, runtime, sdk_config, signer, storage)
        } else {
            let storage =
                Arc::new(runtime.block_on(SqliteStorage::open(sdk_config.sqlite_options()?))?);
            Self::compose_with_storage(config, runtime, sdk_config, signer, storage)
        }
    }

    fn compose_with_storage<T>(
        config: &RuntimeConfig,
        runtime: Runtime,
        sdk_config: CliSdkConfig,
        signer: Option<radroots_sdk::signing::Provider>,
        storage: Arc<T>,
    ) -> Result<Self, CliSdkAdapterError>
    where
        T: radroots_storage::Storage + SyncStorage + 'static,
    {
        let (source, sink) = transport_capabilities(config)?;
        let signer_capability = signer
            .as_ref()
            .map(|provider| Arc::new(SharedProvider(provider.clone())) as Arc<dyn Signer>);
        let mut engine = radroots_sync::Engine::builder(
            storage.clone(),
            Arc::new(SystemClock),
            Arc::new(RandomIds),
            DeadlinePolicy::new(SYNC_TIMEOUT_MS, SYNC_TIMEOUT_MS, SYNC_TIMEOUT_MS)?,
        )
        .source(Arc::clone(&source))
        .sink(Arc::clone(&sink));
        if let Some(capability) = signer_capability.as_ref() {
            engine = engine.signer(Arc::clone(capability));
        }
        let engine = engine.build()?;

        let mut builder = ClientBuilder::new()
            .storage(storage)
            .source(source)
            .sink(sink)
            .sync_engine(engine);
        if let Some(provider) = signer {
            builder = builder.signing(provider);
        }
        let sdk = builder.build()?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn sdk(&self) -> &Client {
        &self.sdk
    }

    pub fn config(&self) -> &CliSdkConfig {
        &self.config
    }

    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }
}

pub fn validate_configured_signer_for_actor(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
    actor_label: &str,
) -> Result<(), RuntimeError> {
    signing::validate_for_actor(config, actor_account_id, actor_pubkey, actor_label)
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

pub fn sdk_nostr_relay_url_policy(config: &RuntimeConfig) -> RelayUrlPolicy {
    if config
        .transport
        .nostr_relay_urls
        .iter()
        .any(|relay_url| relay_url.starts_with("ws://"))
    {
        RelayUrlPolicy::Local
    } else {
        RelayUrlPolicy::Public
    }
}

pub(crate) fn sync_targets(
    config: &RuntimeConfig,
) -> Result<radroots_transport::TargetSet, TransportError> {
    let targets = config
        .transport
        .nostr_relay_urls
        .iter()
        .map(|relay| radroots_transport::Target::nostr_relay(relay))
        .collect::<Result<Vec<_>, _>>()?;
    radroots_transport::TargetSet::new(targets)
}

fn transport_capabilities(
    config: &RuntimeConfig,
) -> Result<(Arc<dyn EventSource>, Arc<dyn EventSink>), CliSdkAdapterError> {
    if matches!(
        config.transport.profile,
        TransportProfileKind::Nostr | TransportProfileKind::MultiTarget
    ) && !config.transport.nostr_relay_urls.is_empty()
    {
        let transport = Arc::new(NostrTransport::new(NostrConfig::new(
            sdk_nostr_relay_url_policy(config),
            &config.transport.nostr_relay_urls,
        )?));
        let source: Arc<dyn EventSource> = transport.clone();
        let sink: Arc<dyn EventSink> = transport;
        Ok((source, sink))
    } else {
        let transport = Arc::new(UnavailableTransport);
        let source: Arc<dyn EventSource> = transport.clone();
        let sink: Arc<dyn EventSink> = transport;
        Ok((source, sink))
    }
}

fn now_unix_ms() -> Result<u64, RuntimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RuntimeError::Config(format!("system clock error: {error}")))?
        .as_millis()
        .try_into()
        .map_err(|_| RuntimeError::Config("system clock is outside SDK range".to_owned()))
}

fn new_source_generation() -> Result<SourceGeneration, RuntimeError> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        RuntimeError::Config(format!("failed to generate SDK source identity: {error}"))
    })?;
    SourceGeneration::new(bytes)
        .map_err(|error| RuntimeError::Config(format!("invalid SDK source identity: {error}")))
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> Result<u64, radroots_sync::Error> {
        now_unix_ms().map_err(|_| radroots_sync::Error::ClockUnavailable)
    }
}

struct RandomIds;

impl IdSource for RandomIds {
    fn next_id(&self, _operation: OperationKind) -> Result<SyncId, radroots_sync::Error> {
        let mut bytes = [0_u8; 16];
        getrandom::getrandom(&mut bytes).map_err(|_| radroots_sync::Error::InvalidSyncId)?;
        SyncId::new(bytes)
    }
}

#[derive(Clone)]
struct SharedProvider(radroots_sdk::signing::Provider);

impl Signer for SharedProvider {
    fn status(&self) -> SigningFuture<'_, Result<SignerStatus, radroots_signing::Error>> {
        Box::pin(self.0.status())
    }

    fn sign(
        &self,
        request: SignRequest,
    ) -> SigningFuture<'_, Result<SignReceipt, radroots_signing::Error>> {
        Box::pin(self.0.sign(request))
    }
}

struct UnavailableTransport;

impl EventSource for UnavailableTransport {
    fn status(&self) -> BoxFuture<'_, Result<SourceStatus, TransportError>> {
        Box::pin(async {
            Ok(SourceStatus::new(
                TransportId::LOCAL,
                false,
                Maturity::Stable,
                Availability::Unavailable,
                SourceCapabilities::NONE,
                LOCAL_TRANSPORT_MESSAGE,
            ))
        })
    }

    fn fetch(&self, _request: FetchRequest) -> BoxFuture<'_, Result<FetchPage, TransportError>> {
        Box::pin(async { Err(TransportError::UnsupportedOperation) })
    }
}

impl EventSink for UnavailableTransport {
    fn status(&self) -> BoxFuture<'_, Result<SinkStatus, TransportError>> {
        Box::pin(async {
            Ok(SinkStatus::new(
                TransportId::LOCAL,
                false,
                Maturity::Stable,
                Availability::Unavailable,
                SinkCapabilities::NONE,
                LOCAL_TRANSPORT_MESSAGE,
            ))
        })
    }

    fn deliver(
        &self,
        _request: DeliveryRequest,
    ) -> BoxFuture<'_, Result<DeliveryReceipt, TransportError>> {
        Box::pin(async { Err(TransportError::UnsupportedOperation) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_host_policies_produce_valid_values() {
        assert!(SystemClock.now_unix_ms().expect("clock") > 0);
        assert_ne!(
            RandomIds
                .next_id(OperationKind::Pull)
                .expect("sync id")
                .as_bytes(),
            &[0; 16]
        );
    }
}
