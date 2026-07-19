use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use radroots_authority::RadrootsLocalEventSigner;
use radroots_identity::RadrootsIdentity;
use radroots_nostr::prelude::{
    RadrootsNostrClient, RadrootsNostrEvent, RadrootsNostrFilter, RadrootsNostrKeys,
    RadrootsNostrKind, RadrootsNostrRelayPoolNotification, RadrootsNostrTimestamp,
    radroots_nostr_filter_tag,
};
use radroots_nostr_connect::prelude::{
    RADROOTS_NOSTR_CONNECT_RPC_KIND, RadrootsNostrConnectBunkerUri,
    RadrootsNostrConnectClientTarget, RadrootsNostrConnectError, RadrootsNostrConnectUri,
};
use radroots_sdk::{
    MeshScopeId, MultiTargetProfile, NostrProfile, NostrRelayUrlPolicy,
    PushOutboxTargetOutcomeKind, PushOutboxTransportOutcomeKind, RadrootsClient,
    RadrootsClientBuilder, RadrootsSdkError, RadrootsSdkLocalKeySigner,
    RadrootsSdkMycNip46RequestPolicy, RadrootsSdkMycNip46Signer, RadrootsSdkNip46Transport,
    RadrootsSdkNip46TransportFuture, RadrootsSdkSignerProvider, RadrootsSdkStorageConfig,
    RadrootsdExecutionProfile, ReticulumAgentEndpoint, ReticulumBehavior as SdkReticulumBehavior,
    ReticulumProfile, TargetPolicy, TransportProfile,
};
use radroots_transport_nostr::{
    RadrootsNostrClientFetchAdapter, RadrootsRelayFetchRequest, RadrootsRelayFetchedEventsReceipt,
    RadrootsRelayTransportError, fetch_relay_events_blocking,
};
use tokio::runtime::{Builder as TokioRuntimeBuilder, Runtime};
use tokio::sync::{Mutex, broadcast};
use tokio::time::{Instant, timeout};
use url::Url;

use crate::runtime::RuntimeError;
use crate::runtime::account;
use crate::runtime::config::{
    CapabilityBindingTargetKind, ReticulumBehavior, RuntimeConfig, SIGNER_REMOTE_NIP46_CAPABILITY,
    SignerBackend, TransportProfileKind,
};

const SDK_STORAGE_DIR_NAME: &str = "sdk";
const CLI_RELAY_FETCH_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MYC_NIP46_SESSION_SECRET_SERVICE: &str = "org.radroots.cli.myc-nip46-session";

#[derive(Debug, thiserror::Error)]
pub enum CliSdkAdapterError {
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
    #[error("{0}")]
    Sdk(#[from] RadrootsSdkError),
}

pub fn sdk_transport_outcome_kind_label(kind: PushOutboxTransportOutcomeKind) -> String {
    kind.as_str().to_owned()
}

pub fn sdk_target_outcome_kind_label(kind: PushOutboxTargetOutcomeKind) -> String {
    kind.as_str().to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliSdkConfig {
    pub storage_root: PathBuf,
    pub geonames_cache_root: PathBuf,
    pub nostr_relay_url_policy: NostrRelayUrlPolicy,
    pub transport_profile: TransportProfile,
    pub radrootsd_execution_profile: Option<RadrootsdExecutionProfile>,
}

impl CliSdkConfig {
    pub fn from_runtime_config(config: &RuntimeConfig) -> Result<Self, RuntimeError> {
        Ok(Self {
            storage_root: sdk_storage_root(config),
            geonames_cache_root: config.paths.shared_cache_root.clone(),
            nostr_relay_url_policy: sdk_nostr_relay_url_policy(config),
            transport_profile: sdk_transport_profile(config)?,
            radrootsd_execution_profile: sdk_radrootsd_execution_profile(config)?,
        })
    }

    pub fn from_runtime_config_for_storage_status(config: &RuntimeConfig) -> Self {
        Self {
            storage_root: sdk_storage_root(config),
            geonames_cache_root: config.paths.shared_cache_root.clone(),
            nostr_relay_url_policy: sdk_nostr_relay_url_policy(config),
            transport_profile: TransportProfile::local_only(),
            radrootsd_execution_profile: None,
        }
    }

    pub fn builder(&self) -> RadrootsClientBuilder {
        let builder = RadrootsClient::builder()
            .storage(RadrootsSdkStorageConfig::Directory(
                self.storage_root.clone(),
            ))
            .geonames_cache_root(self.geonames_cache_root.clone())
            .transport_profile(self.transport_profile.clone());
        if let Some(profile) = self.radrootsd_execution_profile.clone() {
            builder.radrootsd_execution_profile(profile)
        } else {
            builder
        }
    }
}

pub struct CliSdkSession {
    runtime: Runtime,
    sdk: RadrootsClient,
    config: CliSdkConfig,
}

impl CliSdkSession {
    pub fn connect(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config)?;
        let runtime = sdk_runtime()?;
        let sdk = runtime.block_on(sdk_config.builder().build())?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn connect_storage_status(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config_for_storage_status(config);
        let runtime = sdk_runtime()?;
        let sdk = runtime.block_on(sdk_config.builder().build())?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn connect_memory(config: &RuntimeConfig) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config)?;
        let runtime = sdk_runtime()?;
        let sdk = runtime.block_on(memory_builder(&sdk_config).build())?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn connect_for_actor(
        config: &RuntimeConfig,
        actor_account_id: Option<&str>,
        actor_pubkey: &str,
        actor_label: &str,
    ) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config)?;
        let signer_input =
            configured_signer_input(config, actor_account_id, actor_pubkey, actor_label)?;
        let runtime = sdk_runtime()?;
        let signer_provider = runtime.block_on(signer_provider(config, signer_input))?;
        let sdk = runtime.block_on(
            sdk_config
                .builder()
                .signer_provider(signer_provider)
                .build(),
        )?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn connect_memory_for_actor(
        config: &RuntimeConfig,
        actor_account_id: Option<&str>,
        actor_pubkey: &str,
        actor_label: &str,
    ) -> Result<Self, CliSdkAdapterError> {
        let sdk_config = CliSdkConfig::from_runtime_config(config)?;
        let signer_input =
            configured_signer_input(config, actor_account_id, actor_pubkey, actor_label)?;
        let runtime = sdk_runtime()?;
        let signer_provider = runtime.block_on(signer_provider(config, signer_input))?;
        let sdk = runtime.block_on(
            memory_builder(&sdk_config)
                .signer_provider(signer_provider)
                .build(),
        )?;
        Ok(Self {
            runtime,
            sdk,
            config: sdk_config,
        })
    }

    pub fn sdk(&self) -> &RadrootsClient {
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

pub fn validate_configured_signer_for_actor(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
    actor_label: &str,
) -> Result<(), RuntimeError> {
    configured_signer_input(config, actor_account_id, actor_pubkey, actor_label).map(|_| ())
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
        let keys = signing.identity.into_keys();
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

enum CliSdkSignerInput {
    LocalSigner(RadrootsLocalEventSigner),
    MycNip46 {
        client_keys: RadrootsNostrKeys,
        target: RadrootsNostrConnectClientTarget,
        actor_pubkey: String,
    },
}

fn configured_signer_input(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
    actor_label: &str,
) -> Result<CliSdkSignerInput, RuntimeError> {
    match config.signer.backend {
        SignerBackend::Local => {
            let signer =
                local_key_signer_input(config, actor_account_id, actor_pubkey, actor_label)?;
            Ok(CliSdkSignerInput::LocalSigner(signer))
        }
        SignerBackend::Myc => myc_nip46_signer_input(config, actor_account_id, actor_pubkey),
    }
}

fn local_key_signer_input(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
    actor_label: &str,
) -> Result<RadrootsLocalEventSigner, RuntimeError> {
    let signing = match actor_account_id {
        Some(account_id) => {
            account::resolve_local_signing_identity_for_account(config, account_id)?
        }
        None => account::resolve_local_signing_identity(config)?,
    };
    let signer_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !signer_pubkey.eq_ignore_ascii_case(actor_pubkey) {
        return Err(account::AccountRuntimeFailure::mismatch(format!(
            "{actor_label} public key `{actor_pubkey}` does not match local signer account `{}` public key `{signer_pubkey}`",
            signing.account.record.account_id
        ))
        .into());
    }
    RadrootsLocalEventSigner::new(signing.identity.into_keys())
        .map_err(|error| RuntimeError::Config(error.to_string()))
}

fn myc_nip46_signer_input(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
) -> Result<CliSdkSignerInput, RuntimeError> {
    let binding = config
        .capability_binding(SIGNER_REMOTE_NIP46_CAPABILITY)
        .ok_or_else(|| RuntimeError::Config("signer.remote_nip46 binding is missing".to_owned()))?;
    if binding.target_kind != CapabilityBindingTargetKind::ExplicitEndpoint {
        return Err(RuntimeError::Config(format!(
            "signer.remote_nip46 binding target_kind `{}` is not supported for CLI Myc signing; use `explicit_endpoint`",
            binding.target_kind.as_str()
        )));
    }
    if let Some(managed_account_ref) = binding.managed_account_ref.as_deref()
        && !myc_managed_account_ref_matches(managed_account_ref, actor_account_id, actor_pubkey)
    {
        return Err(RuntimeError::Config(format!(
            "signer.remote_nip46 managed_account_ref `{managed_account_ref}` does not match actor account or pubkey"
        )));
    }
    let signer_session_ref = binding.signer_session_ref.as_deref().ok_or_else(|| {
        RuntimeError::Config("signer.remote_nip46 signer_session_ref is missing".to_owned())
    })?;
    let secret =
        account::load_secret_backend_secret(config, signer_session_ref, MYC_NIP46_SESSION_SECRET_SERVICE)?
            .ok_or_else(|| {
                RuntimeError::Config(format!(
                    "signer.remote_nip46 signer_session_ref `{signer_session_ref}` was not found in the account secret backend"
                ))
            })?;
    let client_keys = RadrootsIdentity::from_secret_key_str(secret.trim())
        .map_err(|error| {
            RuntimeError::Config(format!(
                "signer.remote_nip46 signer_session_ref `{signer_session_ref}` contains invalid client secret key material: {error}"
            ))
        })?
        .into_keys();
    let bunker = parse_myc_nip46_target(binding.target.as_str())?;
    let target =
        RadrootsNostrConnectClientTarget::new(bunker.remote_signer_public_key, bunker.relays);
    Ok(CliSdkSignerInput::MycNip46 {
        client_keys,
        target,
        actor_pubkey: actor_pubkey.to_owned(),
    })
}

pub(crate) fn myc_managed_account_ref_matches(
    managed_account_ref: &str,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
) -> bool {
    actor_account_id.is_some_and(|account_id| managed_account_ref == account_id)
        || managed_account_ref == actor_pubkey
}

async fn signer_provider(
    config: &RuntimeConfig,
    signer_input: CliSdkSignerInput,
) -> Result<RadrootsSdkSignerProvider, RuntimeError> {
    match signer_input {
        CliSdkSignerInput::LocalSigner(signer) => {
            let signer = RadrootsSdkLocalKeySigner::from_event_signer(signer)
                .map_err(|error| RuntimeError::Config(error.to_string()))?;
            Ok(RadrootsSdkSignerProvider::LocalKey(signer))
        }
        CliSdkSignerInput::MycNip46 {
            client_keys,
            target,
            actor_pubkey,
        } => {
            let request_policy = myc_nip46_request_policy(config)?;
            let request_timeout = request_policy.request_timeout();
            let transport = Arc::new(
                CliSdkNip46RelayTransport::connect(&client_keys, &target, request_timeout).await?,
            );
            let signer = RadrootsSdkMycNip46Signer::new_with_request_policy(
                client_keys,
                target,
                actor_pubkey,
                transport,
                request_policy,
            )
            .map_err(|error| RuntimeError::Config(error.to_string()))?;
            Ok(RadrootsSdkSignerProvider::MycNip46(Box::new(signer)))
        }
    }
}

fn myc_nip46_request_policy(
    config: &RuntimeConfig,
) -> Result<RadrootsSdkMycNip46RequestPolicy, RuntimeError> {
    RadrootsSdkMycNip46RequestPolicy::new(Duration::from_millis(config.myc.status_timeout_ms))
        .map_err(|error| RuntimeError::Config(error.to_string()))
}

fn parse_myc_nip46_target(value: &str) -> Result<RadrootsNostrConnectBunkerUri, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.starts_with("nostrconnect://") {
        return Err(RuntimeError::Config(
            "signer.remote_nip46 target must be a bunker URI or discovery URL; raw nostrconnect client URIs are signer-side only"
                .to_owned(),
        ));
    }
    let bunker_uri = if trimmed.starts_with("bunker://") {
        trimmed.to_owned()
    } else {
        let url = Url::parse(trimmed).map_err(|error| {
            RuntimeError::Config(format!("signer.remote_nip46 target is invalid: {error}"))
        })?;
        url.query_pairs()
            .find(|(key, _)| key == "uri")
            .map(|(_, uri)| uri.into_owned())
            .ok_or_else(|| {
                RuntimeError::Config(
                    "signer.remote_nip46 discovery target is missing `uri` query parameter"
                        .to_owned(),
                )
            })?
    };
    match RadrootsNostrConnectUri::parse(bunker_uri.as_str()).map_err(|error| {
        RuntimeError::Config(format!("signer.remote_nip46 target is invalid: {error}"))
    })? {
        RadrootsNostrConnectUri::Bunker(bunker) => Ok(bunker),
        RadrootsNostrConnectUri::Client(_) => Err(RuntimeError::Config(
            "signer.remote_nip46 target must resolve to a bunker URI; raw nostrconnect client URIs are signer-side only"
                .to_owned(),
        )),
    }
}

struct CliSdkNip46RelayTransport {
    client: RadrootsNostrClient,
    notifications: Mutex<broadcast::Receiver<RadrootsNostrRelayPoolNotification>>,
    request_timeout: Duration,
    deadline: Mutex<Option<Instant>>,
}

impl CliSdkNip46RelayTransport {
    async fn connect(
        client_keys: &RadrootsNostrKeys,
        target: &RadrootsNostrConnectClientTarget,
        request_timeout: Duration,
    ) -> Result<Self, RuntimeError> {
        if request_timeout.is_zero() {
            return Err(RuntimeError::Config(
                "RADROOTS_CLI_MYC_STATUS_TIMEOUT_MS must be greater than zero".to_owned(),
            ));
        }
        let client = RadrootsNostrClient::new_signerless();
        for relay in &target.relays {
            client.add_relay(relay.as_str()).await.map_err(|error| {
                RuntimeError::Network(format!(
                    "failed to add signer.remote_nip46 relay `{relay}`: {error}"
                ))
            })?;
        }
        let connect_output = client.try_connect(request_timeout).await;
        if connect_output.success.is_empty() {
            let failures = connect_output
                .failed
                .iter()
                .map(|(relay, error)| format!("{relay}: {error}"))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(RuntimeError::Network(if failures.is_empty() {
                "failed to connect to signer.remote_nip46 relays".to_owned()
            } else {
                format!("failed to connect to signer.remote_nip46 relays: {failures}")
            }));
        }
        let filter = radroots_nostr_filter_tag(
            RadrootsNostrFilter::new()
                .kind(RadrootsNostrKind::Custom(RADROOTS_NOSTR_CONNECT_RPC_KIND))
                .since(RadrootsNostrTimestamp::now()),
            "p",
            vec![client_keys.public_key().to_hex()],
        )
        .map_err(|error| {
            RuntimeError::Config(format!(
                "failed to build signer.remote_nip46 filter: {error}"
            ))
        })?;
        let notifications = client.clone().into_inner().notifications();
        let subscribe_output = client.subscribe(filter, None).await.map_err(|error| {
            RuntimeError::Network(format!(
                "failed to subscribe to signer.remote_nip46 response relays: {error}"
            ))
        })?;
        validate_myc_response_subscription_acceptance(
            subscribe_output.success.len(),
            subscribe_output
                .failed
                .iter()
                .map(|(relay, error)| (relay.to_string(), error.to_owned())),
        )?;
        Ok(Self {
            client,
            notifications: Mutex::new(notifications),
            request_timeout,
            deadline: Mutex::new(None),
        })
    }
}

fn validate_myc_response_subscription_acceptance<I>(
    success_count: usize,
    failed: I,
) -> Result<(), RuntimeError>
where
    I: IntoIterator<Item = (String, String)>,
{
    if success_count > 0 {
        return Ok(());
    }
    let failures = failed
        .into_iter()
        .map(|(relay, error)| format!("{relay}: {error}"))
        .collect::<Vec<_>>()
        .join("; ");
    Err(RuntimeError::Network(if failures.is_empty() {
        "signer.remote_nip46 response subscription was not accepted by any relay".to_owned()
    } else {
        format!(
            "signer.remote_nip46 response subscription was not accepted by any relay: {failures}"
        )
    }))
}

impl RadrootsSdkNip46Transport for CliSdkNip46RelayTransport {
    fn publish_request_event<'a>(
        &'a self,
        event: RadrootsNostrEvent,
    ) -> RadrootsSdkNip46TransportFuture<'a, ()> {
        Box::pin(async move {
            *self.deadline.lock().await = Some(Instant::now() + self.request_timeout);
            let output = self.client.send_event(&event).await.map_err(|error| {
                RadrootsNostrConnectError::Transport {
                    reason: error.to_string(),
                }
            })?;
            if output.success.is_empty() {
                let failures = output
                    .failed
                    .iter()
                    .map(|(relay, error)| format!("{relay}: {error}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(RadrootsNostrConnectError::Transport {
                    reason: if failures.is_empty() {
                        "signer.remote_nip46 request event was not accepted by any relay".to_owned()
                    } else {
                        format!(
                            "signer.remote_nip46 request event was not accepted by any relay: {failures}"
                        )
                    },
                });
            }
            Ok(())
        })
    }

    fn next_response_event<'a>(
        &'a self,
    ) -> RadrootsSdkNip46TransportFuture<'a, RadrootsNostrEvent> {
        Box::pin(async move {
            loop {
                let Some(deadline) = *self.deadline.lock().await else {
                    return Err(RadrootsNostrConnectError::Transport {
                        reason: "signer.remote_nip46 request deadline is not initialized"
                            .to_owned(),
                    });
                };
                let now = Instant::now();
                if now >= deadline {
                    return Err(RadrootsNostrConnectError::RequestTimedOut);
                }
                let remaining = deadline - now;
                let mut notifications = self.notifications.lock().await;
                let received = timeout(remaining, notifications.recv()).await;
                drop(notifications);
                let notification = match received {
                    Ok(Ok(notification)) => notification,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        return Err(RadrootsNostrConnectError::Transport {
                            reason: "signer.remote_nip46 relay notification stream closed"
                                .to_owned(),
                        });
                    }
                    Err(_) => return Err(RadrootsNostrConnectError::RequestTimedOut),
                };
                let RadrootsNostrRelayPoolNotification::Event { event, .. } = notification else {
                    continue;
                };
                return Ok((*event).clone());
            }
        })
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

pub(crate) fn fetch_relay_events_via_shared_transport(
    relay_urls: &[String],
    observed_at_ms: i64,
    max_events: usize,
    filter: RadrootsNostrFilter,
) -> Result<RadrootsRelayFetchedEventsReceipt, RadrootsRelayTransportError> {
    let request = RadrootsRelayFetchRequest::fetch(observed_at_ms, max_events, [filter])?
        .with_relay_urls(relay_urls.iter().cloned())
        .with_timeout_ms(CLI_RELAY_FETCH_TIMEOUT_MS)?;
    fetch_relay_events_blocking(&RadrootsNostrClientFetchAdapter, request)
}

fn memory_builder(config: &CliSdkConfig) -> RadrootsClientBuilder {
    RadrootsClient::builder()
        .geonames_cache_root(config.geonames_cache_root.clone())
        .transport_profile(config.transport_profile.clone())
}

pub fn sdk_nostr_relay_url_policy(config: &RuntimeConfig) -> NostrRelayUrlPolicy {
    if config
        .transport
        .nostr_relay_urls
        .iter()
        .any(|relay_url| relay_url.starts_with("ws://"))
    {
        NostrRelayUrlPolicy::Localhost
    } else {
        NostrRelayUrlPolicy::Public
    }
}

pub fn sdk_target_policy(_config: &RuntimeConfig) -> TargetPolicy {
    TargetPolicy::default_profile()
}

fn sdk_transport_profile(config: &RuntimeConfig) -> Result<TransportProfile, RuntimeError> {
    match config.transport.profile {
        TransportProfileKind::LocalOnly => Ok(TransportProfile::local_only()),
        TransportProfileKind::Nostr => {
            let profile = NostrProfile::new(
                config.transport.nostr_relay_urls.iter().map(String::as_str),
                sdk_nostr_relay_url_policy(config),
            )
            .map_err(|error| RuntimeError::Config(error.to_string()))?;
            Ok(TransportProfile::nostr(profile))
        }
        TransportProfileKind::Reticulum => {
            Ok(TransportProfile::reticulum(sdk_reticulum_profile(config)?))
        }
        TransportProfileKind::MultiTarget => {
            let nostr = NostrProfile::new(
                config.transport.nostr_relay_urls.iter().map(String::as_str),
                sdk_nostr_relay_url_policy(config),
            )
            .map_err(|error| RuntimeError::Config(error.to_string()))?;
            Ok(TransportProfile::multi_target(MultiTargetProfile::new(
                nostr,
                sdk_reticulum_profile(config)?,
            )))
        }
    }
}

fn sdk_reticulum_profile(config: &RuntimeConfig) -> Result<ReticulumProfile, RuntimeError> {
    let behavior = match config.transport.reticulum_behavior {
        ReticulumBehavior::RejectDeliveryAttempts => SdkReticulumBehavior::RejectDeliveryAttempts,
        ReticulumBehavior::DeferDeliveryPlans => SdkReticulumBehavior::DeferDeliveryPlans,
    };
    let scope = MeshScopeId::parse(config.transport.reticulum_scope.as_str())
        .map_err(|error| RuntimeError::Config(error.to_string()))?;
    let mut profile = ReticulumProfile::deferred_until_implemented()
        .with_behavior(behavior)
        .with_scope(scope);
    if let Some(agent_endpoint) = config.transport.reticulum_agent_endpoint.as_ref() {
        profile = profile.with_agent_endpoint(
            ReticulumAgentEndpoint::parse(agent_endpoint.as_str())
                .map_err(|error| RuntimeError::Config(error.to_string()))?,
        );
    }
    Ok(profile)
}

fn sdk_radrootsd_execution_profile(
    config: &RuntimeConfig,
) -> Result<Option<RadrootsdExecutionProfile>, RuntimeError> {
    if config.transport.radrootsd_execution.token_file.is_none()
        && config
            .transport
            .radrootsd_execution
            .token_secret_id
            .is_none()
    {
        return Ok(None);
    }
    Ok(Some(
        RadrootsdExecutionProfile::new(config.transport.radrootsd_execution.url.clone())
            .with_bearer_token(radrootsd_execution_bearer_token(config)?),
    ))
}

fn radrootsd_execution_bearer_token(config: &RuntimeConfig) -> Result<String, RuntimeError> {
    if let Some(path) = config.transport.radrootsd_execution.token_file.as_ref() {
        let token = fs::read_to_string(path).map_err(|error| {
            RuntimeError::Config(format!(
                "failed to read radrootsd execution token file {}: {error}",
                path.display()
            ))
        })?;
        return normalize_radrootsd_execution_bearer_token(
            token.as_str(),
            format!("radrootsd execution token file {}", path.display()).as_str(),
        );
    }

    if let Some(secret_id) = config
        .transport
        .radrootsd_execution
        .token_secret_id
        .as_ref()
    {
        let vault = account::account_secret_vault(config)?;
        let token = vault.load_secret(secret_id).map_err(|error| {
            RuntimeError::Config(format!(
                "failed to load radrootsd execution token secret `{secret_id}`: {error}"
            ))
        })?;
        let token = token.ok_or_else(|| {
            RuntimeError::Config(format!(
                "radrootsd execution token secret `{secret_id}` was not found"
            ))
        })?;
        return normalize_radrootsd_execution_bearer_token(
            token.as_str(),
            format!("radrootsd execution token secret `{secret_id}`").as_str(),
        );
    }

    Err(RuntimeError::Config(
        "radrootsd execution requires a configured token file or token secret id".to_owned(),
    ))
}

fn normalize_radrootsd_execution_bearer_token(
    raw: &str,
    source: &str,
) -> Result<String, RuntimeError> {
    let token = raw.trim();
    if token.is_empty() {
        return Err(RuntimeError::Config(format!("{source} is empty")));
    }
    if token.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(RuntimeError::Config(format!(
            "{source} contains unsupported control characters"
        )));
    }
    Ok(token.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use radroots_authority::RadrootsEventSigner;
    use radroots_sdk::{
        PushOutboxTargetOutcomeKind, PushOutboxTransportOutcomeKind, RadrootsdExecutionAuth,
        SdkStorageKind, StorageStatusRequest,
    };
    use radroots_secret_vault::RadrootsSecretBackend;
    use tempfile::tempdir;

    use super::*;
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MycConfig, OutputConfig, OutputFormat, PathsConfig,
        ReticulumBehavior, RhiConfig, RpcConfig, SignerBackend, SignerConfig, Verbosity,
    };

    struct DirectRrRsDependency {
        section: &'static str,
        name: &'static str,
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

    struct SdkOutcomeLabelHelperGuard {
        label: &'static str,
        start: &'static str,
        end: &'static str,
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
            name: "radroots_event",
            owner: "cli-drafts-and-non-migrated-workflows",
            reason: "event DTOs for local drafts, views, relay reads, and release-product mutation inspection",
            lifecycle: "retain until the remaining event-authoring and inspection surfaces migrate",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_event_codec",
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
            name: "radroots_runtime_store",
            owner: "cli-app-interop",
            reason: "shared local work and signed-event interop with the desktop app",
            lifecycle: "retain until a shared runtime-store SDK boundary replaces direct CLI access",
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
            name: "radroots_mesh",
            owner: "cli-mesh-reticulum-admission-policy",
            reason: "canonical Reticulum admission policy and structured delivery denial reporting",
            lifecycle: "retain while CLI exposes Reticulum availability status and policy inspection",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_nostr",
            owner: "cli-signer-and-event-runtime",
            reason: "remote signer relay transport, account event conversion, and direct publish command transport",
            lifecycle: "retain while CLI owns signer transport and direct publish selection",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_transport",
            owner: "cli-transport-config",
            reason: "canonical transport scope identifiers and fail-closed transport profile configuration",
            lifecycle: "retain while CLI owns runtime transport config parsing",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_transport_nostr",
            owner: "cli-nostr-transport-read-boundary",
            reason: "shared fail-closed Nostr relay fetch receipts for trade event list, sync pull, and market refresh",
            lifecycle: "retain until those read surfaces are fully SDK-owned",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_nostr_connect",
            owner: "sdk-myc-nip46-transport",
            reason: "CLI Myc signer target parsing and NIP-46 relay transport wiring for SDK signing",
            lifecycle: "retain while CLI owns signer backend wiring",
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
            name: "radroots_replica_store",
            owner: "derived-projection-and-market-reads",
            reason: "derived projection status, export, market reads, sync pull, basket lookup, and trade draft preflight",
            lifecycle: "retain until those derived projection surfaces move behind SDK APIs",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_replica_schema",
            owner: "derived-projection-and-market-reads",
            reason: "typed query filters for market, basket, and order lookup projections",
            lifecycle: "retain until those derived projection surfaces move behind SDK APIs",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_replica_sync",
            owner: "sync-pull-and-derived-projection",
            reason: "relay ingest, sync pull, market refresh, and derived projection state reporting",
            lifecycle: "retain until relay ingest and projection repair move behind SDK APIs",
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
            name: "radroots_protected_store",
            owner: "cli-account-store",
            reason: "protected file secret vault selection for local account and Myc session material",
            lifecycle: "retain while CLI owns account and signer session custody UX",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_sql_core",
            owner: "derived-projection-and-runtime-store",
            reason: "SQLite executor for derived projection and shared runtime-store storage",
            lifecycle: "transitional until those storage surfaces move behind SDK or shared runtime APIs",
        },
        DirectRrRsDependency {
            section: "dependencies",
            name: "radroots_trade",
            owner: "cli-drafts-and-validation",
            reason: "listing draft validation, trade economics, reducer helpers, and release-product mutation parsing",
            lifecycle: "retain until remaining trade validation and draft behavior migrates",
        },
        DirectRrRsDependency {
            section: "dev-dependencies",
            name: "radroots_outbox",
            owner: "cli-test-fixtures",
            reason: "test-only outbox fixture assertions for CLI transport and SDK workflow coverage",
            lifecycle: "retain while CLI integration tests assert local outbox side effects directly",
        },
    ];

    const NOSTR_RELAY_FETCH_DISALLOWED_TOKENS: &[&str] = &[
        "pub mod direct_relay",
        "use crate::runtime::direct_relay",
        "fetch_events_from_relays",
        "fetch_events_from_relays_with_timeout",
        ".fetch_events(",
    ];

    const SDK_OUTCOME_LABEL_SOURCE_DISALLOWED_TOKENS: &[(&str, &str)] = &[
        (
            concat!("sdk_enum", "_label"),
            "serde-backed SDK enum label extraction",
        ),
        (
            concat!("serde_json::to_value", "(kind)"),
            "serde-backed SDK enum label extraction",
        ),
        (
            concat!("panic!", "(\"SDK enum"),
            "production SDK enum label panic",
        ),
        (
            concat!("fn sdk_target", "_outcome_kind("),
            "local SDK target outcome label helper",
        ),
    ];

    const SDK_OUTCOME_LABEL_HELPER_GUARDS: &[SdkOutcomeLabelHelperGuard] = &[
        SdkOutcomeLabelHelperGuard {
            label: "push outbox transport outcome label helper",
            start: "pub fn sdk_transport_outcome_kind_label(",
            end: "pub fn sdk_target_outcome_kind_label(",
        },
        SdkOutcomeLabelHelperGuard {
            label: "push outbox target outcome label helper",
            start: "pub fn sdk_target_outcome_kind_label(",
            end: "#[derive(Debug, Clone, PartialEq, Eq)]",
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
            label: "farm publish",
            path: "src/runtime/farm.rs",
            start: "fn publish_via_sdk(",
            end: "#[derive(Debug, Clone)]\nstruct SdkFarmPublishInput",
            required_tokens: &[
                "prepare_publish(FarmPreparePublishRequest::new",
                "enqueue_publish(request)",
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
            label: "trade release product commands",
            path: "src/runtime/trade.rs",
            start: "pub fn submit_proposal(",
            end: "pub fn get_trade(",
            required_tokens: &[
                "SubmitProposalRequest::new",
                "ProposeRevisionRequest::new",
                "DecideCandidateRequest::new",
                "CancelTradeRequest::new",
                "ResumeOperationRequest::new",
                "session.sdk().trades().commands().submit_proposal",
                "session.sdk().trades().commands().propose_revision",
                "session.sdk().trades().commands().decide_candidate",
                "session.sdk().trades().commands().cancel_trade",
                "session.sdk().trades().commands().resume_operation",
            ],
        },
        MigratedCliPathGuard {
            label: "trade release product queries",
            path: "src/runtime/trade.rs",
            start: "pub fn get_trade(",
            end: "pub fn seal_private_artifact(",
            required_tokens: &[
                "GetTradeRequest::new",
                "ListTradesRequest::new",
                "RefreshTradeEvidenceRequest::new",
                "InspectEvidenceRequest::new",
                "session.sdk().trades().queries().get_trade",
                "session.sdk().trades().queries().list_trades",
                "session.sdk().trades().queries().refresh_evidence",
                "session.sdk().trades().queries().inspect_evidence",
            ],
        },
        MigratedCliPathGuard {
            label: "trade private artifact SDK",
            path: "src/runtime/trade.rs",
            start: "pub fn seal_private_artifact(",
            end: "pub fn scaffold_proposal_draft(",
            required_tokens: &[
                "TradePrivateArtifactSealRequest::binding_terms",
                "TradePrivateArtifactOpenRequest::new",
                "TradePrivateArtifactDeleteRequest::new",
                "session.sdk().trades().seal_private_artifact",
                "session.sdk().trades().open_private_artifact",
                "session.sdk().trades().delete_private_artifact",
            ],
        },
        MigratedCliPathGuard {
            label: "store status",
            path: "src/runtime/store.rs",
            start: "pub fn status(config: &RuntimeConfig) -> Result<LocalStatusView, CliSdkAdapterError>",
            end: "fn derived_projection_status(",
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
                "RadrootsClient::restore",
            ],
        },
    ];

    const MIGRATED_PATH_DISALLOWED_TOKENS: &[&str] = &[
        "fetch_events_from_relays",
        "fetch_relay_events_via_shared_transport",
        "publish_parts_with_identity",
        "publish_via_direct_relay",
        "mutate_via_direct_relay",
        "radroots_replica_pending_publish",
        "radroots_replica_pending_publish_batch",
        "radroots_replica_sync_status",
        "ReplicaSql::new",
        "SqliteExecutor::open(&config.local.replica_store_path)",
        "outbox_idempotency_digest",
        "canonical_target_transport_endpoints",
        "radroots_sdk::protocol::order",
        "build_order_request_draft",
        "build_order_decision_draft",
        "build_order_cancellation_draft",
        "parse_order_root_tag",
        "parse_order_prev_tag",
        "build_transition_proof_request_tags",
        "build_transition_proof_result_tags",
        "build_job_feedback_tags",
        "KIND_TRADE_TRANSITION_PROOF",
        "KIND_JOB_FEEDBACK",
        "status_client(",
        "TradeStatusClient",
        "TradeValidationClient",
    ];

    const REMOVED_SDK_ROOT_TRADE_ALIAS_NAMES: &[&str] = &[
        "trade_buyer",
        "trade_seller",
        "trade_status",
        "trade_resync",
        "trade_validation",
    ];

    const REMOVED_SDK_STATUS_SURFACE_TOKENS: &[&str] = &[
        "status_client(",
        "TradeStatusClient",
        "TradeValidationClient",
    ];

    mod removed_surface_fixtures {
        pub const ACCOUNT_CREATE_MODE: &str = "AccountCreateMode";
        pub const CONFIG_IDENTITY_PATH_EXISTS: &str = "config.identity.path.exists()";
        pub const CREATE_OR_MIGRATE_DEFAULT_ACCOUNT: &str = "create_or_migrate_default_account";
        pub const MIGRATED_OUTPUT_JSON: &str = "\"migrated\"";
        pub const MIGRATE_LEGACY_IDENTITY_FILE: &str = "migrate_legacy_identity_file";
    }

    #[test]
    fn maps_runtime_config_to_sdk_builder_inputs() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(
            root.path(),
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()],
        );

        let sdk_config = CliSdkConfig::from_runtime_config(&config).expect("sdk config");

        assert_eq!(sdk_config.storage_root, config.local.root.join("sdk"));
        assert_eq!(
            sdk_config.nostr_relay_url_policy,
            NostrRelayUrlPolicy::Public
        );
        let TransportProfile::Nostr { profile } = sdk_config.transport_profile else {
            panic!("expected Nostr transport profile");
        };
        assert_eq!(
            profile.relay_urls(),
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
    }

    #[test]
    fn maps_multi_target_runtime_config_to_sdk_multi_target_profile() {
        let root = tempdir().expect("tempdir");
        let mut config = sample_config(
            root.path(),
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()],
        );
        config.transport.profile = TransportProfileKind::MultiTarget;
        config.transport.reticulum_behavior = ReticulumBehavior::DeferDeliveryPlans;
        config.transport.reticulum_scope = "farmers_market".to_owned();
        config.transport.reticulum_agent_endpoint = Some("reticulum-agent:local".to_owned());

        let sdk_config = CliSdkConfig::from_runtime_config(&config).expect("sdk config");

        let TransportProfile::MultiTarget { profile } = sdk_config.transport_profile else {
            panic!("expected multi-target transport profile");
        };
        assert_eq!(
            profile.nostr().relay_urls(),
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
        assert_eq!(
            profile.reticulum().behavior().as_str(),
            "defer_delivery_plans"
        );
        assert_eq!(profile.reticulum().scope().as_str(), "farmers_market");
        assert_eq!(
            profile
                .reticulum()
                .agent_endpoint()
                .expect("agent endpoint")
                .as_str(),
            "reticulum-agent:local"
        );
    }

    #[test]
    fn maps_radrootsd_execution_token_file_to_sdk_profile_auth() {
        let root = tempdir().expect("tempdir");
        let mut config = sample_config(root.path(), Vec::new());
        let token_file = root.path().join("radrootsd_execution.token");
        fs::write(&token_file, "radrootsd-execution-file-token\n").expect("write token file");
        config.transport.radrootsd_execution.url = "http://127.0.0.1:7070".to_owned();
        config.transport.radrootsd_execution.token_file = Some(token_file);

        let sdk_config = CliSdkConfig::from_runtime_config(&config).expect("sdk config");

        assert!(matches!(
            sdk_config.transport_profile,
            TransportProfile::LocalOnly
        ));
        let profile = sdk_config
            .radrootsd_execution_profile
            .expect("radrootsd execution profile");
        assert_eq!(profile.endpoint_url(), "http://127.0.0.1:7070");
        assert_eq!(
            profile.auth(),
            &RadrootsdExecutionAuth::BearerToken("radrootsd-execution-file-token".to_owned())
        );
    }

    #[test]
    fn maps_radrootsd_execution_token_secret_id_to_sdk_profile_auth() {
        let root = tempdir().expect("tempdir");
        let mut config = sample_config(root.path(), Vec::new());
        config.transport.radrootsd_execution.url = "http://127.0.0.1:7070".to_owned();
        config.transport.radrootsd_execution.token_secret_id =
            Some("radrootsd_execution_token".to_owned());
        let vault = account::account_secret_vault(&config).expect("account vault");
        vault
            .store_secret(
                "radrootsd_execution_token",
                "radrootsd-execution-secret-token",
            )
            .expect("store radrootsd execution token");

        let sdk_config = CliSdkConfig::from_runtime_config(&config).expect("sdk config");

        let profile = sdk_config
            .radrootsd_execution_profile
            .expect("radrootsd execution profile");
        assert_eq!(
            profile.auth(),
            &RadrootsdExecutionAuth::BearerToken("radrootsd-execution-secret-token".to_owned())
        );
    }

    #[test]
    fn radrootsd_execution_profile_requires_materialized_bearer_token() {
        let root = tempdir().expect("tempdir");
        let mut config = sample_config(root.path(), Vec::new());
        config.transport.radrootsd_execution.url = "http://127.0.0.1:7070".to_owned();

        let sdk_config = CliSdkConfig::from_runtime_config(&config).expect("sdk config");
        assert!(sdk_config.radrootsd_execution_profile.is_none());
    }

    #[test]
    fn radrootsd_execution_token_resolution_rejects_empty_or_header_unsafe_tokens() {
        assert!(matches!(
            normalize_radrootsd_execution_bearer_token(" \n", "radrootsd execution token"),
            Err(RuntimeError::Config(message)) if message.contains("empty")
        ));
        assert!(matches!(
            normalize_radrootsd_execution_bearer_token(
                "radrootsd\nexecution",
                "radrootsd execution token"
            ),
            Err(RuntimeError::Config(message)) if message.contains("control characters")
        ));
    }

    #[test]
    fn maps_localhost_ws_relays_to_localhost_sdk_policy() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(root.path(), vec!["ws://127.0.0.1:8080".to_owned()]);

        assert_eq!(
            sdk_nostr_relay_url_policy(&config),
            NostrRelayUrlPolicy::Localhost
        );
    }

    #[test]
    fn materializes_local_account_signer_for_sdk_workflows() {
        let root = tempdir().expect("tempdir");
        let config = sample_config(root.path(), Vec::new());
        let account = account::create_default_account(&config).expect("create account");

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
        assert_eq!(status.event_store.valid_stream_events, 0);
        assert_eq!(status.outbox.total_events, 0);
    }

    #[test]
    fn myc_request_policy_uses_cli_timeout_config() {
        let root = tempdir().expect("tempdir");
        let mut config = sample_config(root.path(), Vec::new());
        config.myc.status_timeout_ms = 12_345;

        let policy = myc_nip46_request_policy(&config).expect("request policy");

        assert_eq!(policy.request_timeout(), Duration::from_millis(12_345));
    }

    #[test]
    fn myc_request_policy_rejects_zero_cli_timeout() {
        let root = tempdir().expect("tempdir");
        let mut config = sample_config(root.path(), Vec::new());
        config.myc.status_timeout_ms = 0;

        let error = myc_nip46_request_policy(&config).expect_err("zero timeout");

        assert!(error.to_string().contains("must be greater than zero"));
    }

    #[test]
    fn myc_response_subscription_requires_relay_acceptance() {
        let error = validate_myc_response_subscription_acceptance(
            0,
            [(
                "ws://127.0.0.1:8080".to_owned(),
                "subscription rejected".to_owned(),
            )],
        )
        .expect_err("response subscription acceptance");

        assert!(
            error
                .to_string()
                .contains("response subscription was not accepted by any relay")
        );
        assert!(error.to_string().contains("subscription rejected"));

        validate_myc_response_subscription_acceptance(1, std::iter::empty())
            .expect("accepted response subscription");
    }

    #[test]
    fn sdk_sources_do_not_import_cli_types() {
        let sdk_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sdk/crates/sdk/src");
        let mut files = Vec::new();
        collect_rs_files(sdk_src.as_path(), &mut files);
        let forbidden = vec![
            ("radroots_cli".to_owned(), "CLI crate identity"),
            ("domains/radroots/cli".to_owned(), "CLI mount path"),
            (["approval", "token"].join("_"), "retired approval string"),
            ("OutputEnvelope".to_owned(), "CLI output envelope"),
            ("next_actions".to_owned(), "CLI next-action rendering"),
            ("exit_code".to_owned(), "CLI exit-code contract"),
            ("docs/".to_owned(), "repository docs path"),
            ("radroots store".to_owned(), "CLI command string"),
            ("radroots sync".to_owned(), "CLI command string"),
            ("radroots listing".to_owned(), "CLI command string"),
            ("radroots trade".to_owned(), "CLI command string"),
        ];

        for file in files {
            let source = fs::read_to_string(&file).expect("read sdk source");
            for (needle, description) in forbidden.iter() {
                assert!(
                    !source.contains(needle.as_str()),
                    "SDK source contains {description} `{needle}` in {}",
                    file.display()
                );
            }
        }
    }

    #[test]
    fn sync_order_transport_label_helpers_cover_public_outcomes() {
        for (kind, label) in [
            (PushOutboxTargetOutcomeKind::Accepted, "accepted"),
            (
                PushOutboxTargetOutcomeKind::DuplicateAccepted,
                "duplicate_accepted",
            ),
            (PushOutboxTargetOutcomeKind::Blocked, "blocked"),
            (PushOutboxTargetOutcomeKind::RateLimited, "rate_limited"),
            (PushOutboxTargetOutcomeKind::Invalid, "invalid"),
            (PushOutboxTargetOutcomeKind::PowRequired, "pow_required"),
            (PushOutboxTargetOutcomeKind::Restricted, "restricted"),
            (PushOutboxTargetOutcomeKind::AuthRequired, "auth_required"),
            (PushOutboxTargetOutcomeKind::Muted, "muted"),
            (PushOutboxTargetOutcomeKind::Unsupported, "unsupported"),
            (
                PushOutboxTargetOutcomeKind::PaymentRequired,
                "payment_required",
            ),
            (PushOutboxTargetOutcomeKind::Error, "error"),
            (PushOutboxTargetOutcomeKind::Timeout, "timeout"),
            (
                PushOutboxTargetOutcomeKind::ConnectionFailed,
                "connection_failed",
            ),
            (
                PushOutboxTargetOutcomeKind::TargetUriRejected,
                "target_uri_rejected",
            ),
            (
                PushOutboxTargetOutcomeKind::SkippedAlreadyAccepted,
                "skipped_already_accepted",
            ),
            (
                PushOutboxTargetOutcomeKind::DeferredUntilImplemented,
                "deferred_until_implemented",
            ),
            (PushOutboxTargetOutcomeKind::Unknown, "unknown"),
        ] {
            assert_eq!(sdk_target_outcome_kind_label(kind), label);
        }

        for (kind, label) in [
            (PushOutboxTransportOutcomeKind::Accepted, "accepted"),
            (
                PushOutboxTransportOutcomeKind::DuplicateAccepted,
                "duplicate_accepted",
            ),
            (PushOutboxTransportOutcomeKind::Delivered, "delivered"),
            (PushOutboxTransportOutcomeKind::Forwarded, "forwarded"),
            (
                PushOutboxTransportOutcomeKind::StoredByGateway,
                "stored_by_gateway",
            ),
            (PushOutboxTransportOutcomeKind::Seen, "seen"),
            (
                PushOutboxTransportOutcomeKind::DeferredUntilImplemented,
                "deferred_until_implemented",
            ),
            (PushOutboxTransportOutcomeKind::Rejected, "rejected"),
            (
                PushOutboxTransportOutcomeKind::RouteUnavailable,
                "route_unavailable",
            ),
            (
                PushOutboxTransportOutcomeKind::PayloadTooLarge,
                "payload_too_large",
            ),
            (
                PushOutboxTransportOutcomeKind::PolicyDenied,
                "policy_denied",
            ),
            (PushOutboxTransportOutcomeKind::Timeout, "timeout"),
            (
                PushOutboxTransportOutcomeKind::ConnectionFailed,
                "connection_failed",
            ),
            (
                PushOutboxTransportOutcomeKind::TransportUnavailable,
                "transport_unavailable",
            ),
        ] {
            assert_eq!(sdk_transport_outcome_kind_label(kind), label);
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
    fn cli_production_sources_reject_unowned_relay_fetch_helpers() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_rs_files(manifest_dir.join("src").as_path(), &mut files);
        files.sort();

        let findings = files
            .iter()
            .flat_map(|file| {
                let source = fs::read_to_string(file).expect("read cli source");
                let relative_path = relative_source_path(manifest_dir, file.as_path());
                match production_source_without_tests(&relative_path, &source) {
                    Ok(production_source) => {
                        nostr_relay_fetch_findings(&relative_path, production_source.as_str())
                    }
                    Err(error) => vec![error],
                }
            })
            .collect::<Vec<_>>();

        assert!(
            findings.is_empty(),
            "CLI production sources contain unowned Nostr relay fetch helpers:\n{}",
            findings.join("\n")
        );
    }

    #[test]
    fn cli_production_sources_reject_dead_code_suppressions() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_rs_files(manifest_dir.join("src").as_path(), &mut files);
        files.sort();

        let findings = files
            .iter()
            .flat_map(|file| {
                let source = fs::read_to_string(file).expect("read cli source");
                let relative_path = relative_source_path(manifest_dir, file.as_path());
                match production_source_without_tests(&relative_path, &source) {
                    Ok(production_source) => {
                        if production_source.contains("allow(dead_code)") {
                            vec![format!("{relative_path}: production dead-code suppression")]
                        } else {
                            Vec::new()
                        }
                    }
                    Err(error) => vec![error],
                }
            })
            .collect::<Vec<_>>();

        assert!(
            findings.is_empty(),
            "CLI production sources contain dead-code suppressions:\n{}",
            findings.join("\n")
        );
    }

    #[test]
    fn sync_order_transport_cli_production_sources_use_sdk_outcome_label_contract() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_rs_files(manifest_dir.join("src").as_path(), &mut files);
        files.sort();

        let findings = files
            .iter()
            .flat_map(|file| {
                let source = fs::read_to_string(file).expect("read cli source");
                let relative_path = relative_source_path(manifest_dir, file.as_path());
                match production_source_without_tests(&relative_path, &source) {
                    Ok(production_source) => sdk_outcome_label_contract_findings(
                        &relative_path,
                        source.as_str(),
                        production_source.as_str(),
                    ),
                    Err(error) => vec![error],
                }
            })
            .collect::<Vec<_>>();

        assert!(
            findings.is_empty(),
            "CLI production sources violate the SDK outcome label contract:\n{}",
            findings.join("\n")
        );
    }

    #[test]
    fn sdk_outcome_label_guard_rejects_wildcard_unknown_inside_helper_span() {
        let source = sdk_outcome_label_helper_source(
            concat!(
                "    match kind {\n",
                "        PushOutboxTransportOutcomeKind::Accepted => \"accepted\".to_owned(),\n",
                "        _ => \"unknown\".to_owned(),\n",
                "    }\n"
            ),
            "",
        );
        let production_source =
            production_source_without_tests("fixture.rs", source.as_str()).expect("source");
        let findings =
            sdk_outcome_label_contract_findings("fixture.rs", source.as_str(), &production_source);

        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("does not delegate to SDK enum as_str labels")),
            "SDK outcome label guard must reject local helper rendering:\n{}",
            findings.join("\n")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("uses a wildcard outcome label arm")),
            "SDK outcome label guard must reject wildcard unknown helper rendering:\n{}",
            findings.join("\n")
        );
    }

    #[test]
    fn sdk_outcome_label_guard_allows_unrelated_unknown_fallbacks_outside_helper_spans() {
        let source = sdk_outcome_label_helper_source(
            "    kind.as_str().to_owned()\n",
            concat!(
                "fn unrelated_status_label(value: Option<&str>) -> &str {\n",
                "    match value {\n",
                "        Some(value) => value,\n",
                "        _ => \"unknown\",\n",
                "    }\n",
                "}\n"
            ),
        );
        let production_source =
            production_source_without_tests("fixture.rs", source.as_str()).expect("source");
        let findings =
            sdk_outcome_label_contract_findings("fixture.rs", source.as_str(), &production_source);

        assert!(
            findings.is_empty(),
            "SDK outcome label guard must ignore unrelated unknown fallbacks:\n{}",
            findings.join("\n")
        );
    }

    #[test]
    fn cli_account_create_sources_reject_implicit_identity_ingestion() {
        let account_source = rust_code_without_non_code(
            "src/runtime/account.rs",
            crate_source("src/runtime/account.rs").as_str(),
        )
        .expect("account source classification");
        let core_source = rust_code_without_non_code(
            "src/ops/exec/core.rs",
            crate_source("src/ops/exec/core.rs").as_str(),
        )
        .expect("core source classification");

        for token in [
            removed_surface_fixtures::CREATE_OR_MIGRATE_DEFAULT_ACCOUNT,
            removed_surface_fixtures::ACCOUNT_CREATE_MODE,
            removed_surface_fixtures::MIGRATE_LEGACY_IDENTITY_FILE,
            removed_surface_fixtures::CONFIG_IDENTITY_PATH_EXISTS,
        ] {
            assert!(
                !account_source.contains(token),
                "CLI account runtime must not contain implicit account-create identity-ingestion token `{token}`"
            );
        }
        for token in [
            removed_surface_fixtures::ACCOUNT_CREATE_MODE,
            removed_surface_fixtures::MIGRATED_OUTPUT_JSON,
        ] {
            assert!(
                !core_source.contains(token),
                "CLI account create output must not contain removed account-create token `{token}`"
            );
        }
    }

    #[test]
    fn migrated_cli_paths_are_guarded_against_workflow_bypasses() {
        for guard in MIGRATED_CLI_PATH_GUARDS {
            let source = crate_source(guard.path);
            assert_migrated_path(
                guard.label,
                source_segment(&source, guard.start, guard.end),
                guard.required_tokens,
            );
        }
    }

    #[test]
    fn migrated_path_root_alias_scanner_preserves_status_helpers() {
        assert!(
            root_trade_alias_findings(
                "allowed",
                "fn trade_status_for_locator() { RadrootsSdkError::trade_status_limit_invalid(0, 1, 100); }",
            )
            .is_empty()
        );

        let findings = root_trade_alias_findings(
            "forbidden",
            "sdk.trade_status (request); RadrootsClient::trade_resync(&sdk);",
        );

        for alias in ["trade_status", "trade_resync"] {
            assert!(
                findings.iter().any(|finding| finding.contains(alias)),
                "migrated path root alias scanner must reject `{alias}`"
            );
        }
    }

    #[test]
    fn cli_production_sources_reject_removed_sdk_status_surfaces_repo_wide() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_rs_files(manifest_dir.join("src").as_path(), &mut files);
        files.sort();

        let findings = files
            .iter()
            .flat_map(|file| {
                let source = fs::read_to_string(file).expect("read cli source");
                let relative_path = relative_source_path(manifest_dir, file.as_path());
                match production_source_without_tests(&relative_path, &source) {
                    Ok(production_source) => {
                        let mut findings = removed_sdk_status_surface_findings(
                            &relative_path,
                            production_source.as_str(),
                        );
                        findings.extend(root_trade_alias_findings(
                            &relative_path,
                            production_source.as_str(),
                        ));
                        findings
                    }
                    Err(error) => vec![error],
                }
            })
            .collect::<Vec<_>>();

        assert!(
            findings.is_empty(),
            "CLI production sources contain removed SDK surfaces:\n{}",
            findings.join("\n")
        );
    }

    #[test]
    fn cli_production_source_scanner_strips_test_modules() {
        let inline = concat!(
            "fn production() {}\n",
            "#[cfg(test)] mod tests { fn test_only() { sdk.status_client(); } }\n",
        );
        let multiline = concat!(
            "fn production() {}\n",
            "#[cfg(test)]\n",
            "#[doc(hidden)]\n",
            "mod tests {\n",
            "    fn test_only() { let _ = TradeValidationClient; }\n",
            "    const BRACE: &str = \"}\";\n",
            "}\n",
        );

        for source in [inline, multiline] {
            let production_source =
                production_source_without_tests("fixture.rs", source).expect("production source");
            assert!(
                removed_sdk_status_surface_findings("fixture.rs", production_source.as_str())
                    .is_empty()
            );
            assert!(root_trade_alias_findings("fixture.rs", production_source.as_str()).is_empty());
        }
    }

    #[test]
    fn cli_production_source_scanner_strips_cfg_test_functions() {
        let source = concat!(
            "fn production() {}\n",
            "#[cfg(test)]\n",
            "fn test_only() { let _ = TradeValidationClient; }\n",
            "fn after_tests() {}\n",
        );
        let production_source =
            production_source_without_tests("fixture.rs", source).expect("production source");

        assert!(!production_source.contains("TradeValidationClient"));
        assert!(production_source.contains("fn production()"));
        assert!(production_source.contains("fn after_tests()"));
        assert!(
            removed_sdk_status_surface_findings("fixture.rs", production_source.as_str())
                .is_empty()
        );
    }

    #[test]
    fn cli_production_source_scanner_strips_cfg_test_fragments() {
        let source = concat!(
            "enum Provider {",
            "#[cfg(test)] TestOnly,",
            "Production",
            "}\n",
            "fn production(provider: Provider) { match provider {",
            "#[cfg(test)] Provider::TestOnly => sdk.status_client(),",
            "Provider::Production => {}",
            "} }\n",
        );
        let production_source =
            production_source_without_tests("fixture.rs", source).expect("production source");

        assert!(!production_source.contains("TestOnly"));
        assert!(
            removed_sdk_status_surface_findings("fixture.rs", production_source.as_str())
                .is_empty()
        );
    }

    #[test]
    fn cli_production_source_scanner_reports_malformed_cfg_test_items() {
        let source = concat!(
            "fn production() {}\n",
            "#[cfg(test)]\n",
            "mod tests { fn hidden() { let _ = TradeValidationClient; }\n",
            "fn after_tests() { sdk.status_client(); }\n",
        );
        let error =
            production_source_without_tests("fixture.rs", source).expect_err("classification");

        assert!(error.contains("fixture.rs:2"));
        assert!(error.contains("cfg(test) item is not closed"));
    }

    #[test]
    fn removed_surface_scanner_ignores_comments_and_literals() {
        let source = concat!(
            "fn production() {\n",
            "    let literal = \"status_client(\";\n",
            "    let raw = r#\"RadrootsClient::trade_resync(&sdk)\"#;\n",
            "    let character = 'x';\n",
            "}\n",
            "// TradeValidationClient::new(root)\n",
            "/* sdk.trade_status(request) */\n",
        );
        let production_source =
            production_source_without_tests("fixture.rs", source).expect("production source");

        assert!(
            removed_sdk_status_surface_findings("fixture.rs", production_source.as_str())
                .is_empty()
        );
        assert!(root_trade_alias_findings("fixture.rs", production_source.as_str()).is_empty());
    }

    #[test]
    fn repo_wide_removed_surface_scanner_reports_production_violations() {
        let source = "fn production() { sdk.status_client(); RadrootsClient::trade_resync(&sdk); }";
        let status_findings = removed_sdk_status_surface_findings("fixture.rs", source);
        let alias_findings = root_trade_alias_findings("fixture.rs", source);

        assert!(
            status_findings
                .iter()
                .any(|finding| finding.contains("status_client("))
        );
        assert!(
            alias_findings
                .iter()
                .any(|finding| finding.contains("trade_resync"))
        );
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
                                .filter(|path| {
                                    path.contains("../lib/crates")
                                        || path.contains("domains/radroots/lib/crates")
                                })
                                .map(|_| format!("{section}:{name}"))
                        })
                    })
            })
            .collect()
    }

    fn direct_rr_rs_dependency_key(dependency: &DirectRrRsDependency) -> String {
        format!("{}:{}", dependency.section, dependency.name)
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
        let source =
            rust_code_without_non_code(label, source).expect("migrated path source classification");

        for token in required_tokens {
            assert!(
                source.as_str().contains(token),
                "{label} does not contain required SDK token `{token}`"
            );
        }

        for token in MIGRATED_PATH_DISALLOWED_TOKENS {
            assert!(
                !source.as_str().contains(token),
                "{label} contains disallowed migrated-path token `{token}`"
            );
        }

        let findings = root_trade_alias_findings(label, source.as_str());
        assert!(
            findings.is_empty(),
            "{label} contains removed SDK root trade aliases:\n{}",
            findings.join("\n")
        );
    }

    fn root_trade_alias_findings(label: &str, source: &str) -> Vec<String> {
        let mut findings = Vec::new();

        for alias in REMOVED_SDK_ROOT_TRADE_ALIAS_NAMES {
            for (index, _) in source.match_indices(alias) {
                let before = source[..index].chars().next_back();
                let after_index = index + alias.len();
                let after = source[after_index..].chars().next();

                if before.is_some_and(is_rust_identifier_character)
                    || after.is_some_and(is_rust_identifier_character)
                {
                    continue;
                }

                if source[after_index..]
                    .chars()
                    .find(|character| !character.is_whitespace())
                    != Some('(')
                {
                    continue;
                }

                let prefix = source[..index].trim_end();
                if prefix.ends_with('.') || prefix.ends_with("::") {
                    findings.push(format!(
                        "{label}:{} uses removed SDK root trade alias `{alias}`",
                        line_number(source, index)
                    ));
                }
            }
        }

        findings
    }

    fn removed_sdk_status_surface_findings(label: &str, source: &str) -> Vec<String> {
        REMOVED_SDK_STATUS_SURFACE_TOKENS
            .iter()
            .flat_map(|token| {
                source.match_indices(token).map(move |(index, _)| {
                    format!(
                        "{label}:{} uses removed SDK surface `{token}`",
                        line_number(source, index)
                    )
                })
            })
            .collect()
    }

    fn nostr_relay_fetch_findings(label: &str, source: &str) -> Vec<String> {
        NOSTR_RELAY_FETCH_DISALLOWED_TOKENS
            .iter()
            .flat_map(|token| {
                source.match_indices(token).map(move |(index, _)| {
                    format!(
                        "{label}:{} uses unowned Nostr relay fetch token `{token}`",
                        line_number(source, index)
                    )
                })
            })
            .collect()
    }

    fn sdk_outcome_label_contract_findings(
        label: &str,
        raw_source: &str,
        production_source: &str,
    ) -> Vec<String> {
        let mut findings = SDK_OUTCOME_LABEL_SOURCE_DISALLOWED_TOKENS
            .iter()
            .flat_map(|(token, reason)| {
                production_source.match_indices(token).map(move |(index, _)| {
                    format!(
                        "{label}:{} uses forbidden SDK outcome label token `{token}` for {reason}",
                        line_number(production_source, index)
                    )
                })
            })
            .collect::<Vec<_>>();

        findings.extend(sdk_outcome_label_helper_findings(label, raw_source));

        findings
    }

    fn sdk_outcome_label_helper_findings(label: &str, source: &str) -> Vec<String> {
        let mut findings = Vec::new();
        for guard in SDK_OUTCOME_LABEL_HELPER_GUARDS {
            let (start_index, end_index) =
                match optional_source_segment_bounds(source, guard.start, guard.end) {
                    Ok(Some(bounds)) => bounds,
                    Ok(None) => continue,
                    Err(error) => {
                        findings.push(format!("{label}: {error}"));
                        continue;
                    }
                };
            let raw_segment = &source[start_index..end_index];
            let code_segment =
                rust_code_without_non_code(label, raw_segment).expect("SDK label helper source");
            if !code_segment.contains("kind.as_str().to_owned()") {
                findings.push(format!(
                    "{label}:{} {helper} does not delegate to SDK enum as_str labels",
                    line_number(source, start_index),
                    helper = guard.label
                ));
            }
            if let Some(index) = code_segment.find("match kind") {
                findings.push(format!(
                    "{label}:{} {helper} matches SDK outcome variants locally",
                    line_number(source, start_index + index),
                    helper = guard.label
                ));
            }
            if let Some(index) = code_segment.find("_ =>") {
                findings.push(format!(
                    "{label}:{} {helper} uses a wildcard outcome label arm",
                    line_number(source, start_index + index),
                    helper = guard.label
                ));
            }
        }
        findings
    }

    fn sdk_outcome_label_helper_source(transport_body: &str, extra_source: &str) -> String {
        format!(
            "pub fn sdk_transport_outcome_kind_label(kind: PushOutboxTransportOutcomeKind) -> String {{\n\
{transport_body}\
}}\n\
pub fn sdk_target_outcome_kind_label(kind: PushOutboxTargetOutcomeKind) -> String {{\n\
    kind.as_str().to_owned()\n\
}}\n\
#[derive(Debug, Clone, PartialEq, Eq)]\n\
struct FixtureConfig;\n\
{extra_source}"
        )
    }

    fn optional_source_segment_bounds(
        source: &str,
        start: &str,
        end: &str,
    ) -> Result<Option<(usize, usize)>, String> {
        let Some(start_index) = source.find(start) else {
            return Ok(None);
        };
        let Some(end_index) = source[start_index..]
            .find(end)
            .map(|index| start_index + index)
        else {
            return Err(format!(
                "SDK outcome label helper source segment starting `{start}` is missing end marker `{end}`"
            ));
        };
        Ok(Some((start_index, end_index)))
    }

    fn production_source_without_tests(path: &str, source: &str) -> Result<String, String> {
        let code_source = rust_code_without_non_code(path, source)?;
        let mut production_source = String::with_capacity(code_source.len());
        let mut cursor = 0;

        while let Some((attribute_start, attribute_end)) =
            find_cfg_test_attribute(code_source.as_str(), cursor)
        {
            let item_end =
                find_cfg_test_item_end(path, code_source.as_str(), attribute_start, attribute_end)?;

            production_source.push_str(&code_source[cursor..attribute_start]);
            push_masked_source(
                &mut production_source,
                &code_source[attribute_start..item_end],
            );
            cursor = item_end;
        }

        production_source.push_str(&code_source[cursor..]);
        Ok(production_source)
    }

    fn rust_code_without_non_code(path: &str, source: &str) -> Result<String, String> {
        let bytes = source.as_bytes();
        let mut code = String::with_capacity(source.len());
        let mut cursor = 0;

        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' => {
                    let end = skip_quoted_rust_literal(source, cursor, b'"').ok_or_else(|| {
                        classification_error(path, source, cursor, "unterminated string literal")
                    })?;
                    push_masked_source(&mut code, &source[cursor..end]);
                    cursor = end;
                }
                b'\'' => {
                    if let Some(end) = skip_rust_char_literal(source, cursor) {
                        push_masked_source(&mut code, &source[cursor..end]);
                        cursor = end;
                    } else {
                        let character = source[cursor..].chars().next().expect("quote");
                        code.push(character);
                        cursor += character.len_utf8();
                    }
                }
                b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                    let end = source[cursor..]
                        .find('\n')
                        .map_or(source.len(), |newline| cursor + newline);
                    push_masked_source(&mut code, &source[cursor..end]);
                    cursor = end;
                }
                b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                    let end = skip_rust_block_comment(source, cursor).ok_or_else(|| {
                        classification_error(path, source, cursor, "unterminated block comment")
                    })?;
                    push_masked_source(&mut code, &source[cursor..end]);
                    cursor = end;
                }
                b'r' => {
                    if let Some(end) = skip_raw_rust_string(source, cursor) {
                        push_masked_source(&mut code, &source[cursor..end]);
                        cursor = end;
                    } else {
                        code.push('r');
                        cursor += 1;
                    }
                }
                _ => {
                    let character = source[cursor..].chars().next().expect("source character");
                    code.push(character);
                    cursor += character.len_utf8();
                }
            }
        }

        Ok(code)
    }

    fn push_masked_source(output: &mut String, source: &str) {
        for character in source.chars() {
            if character == '\n' {
                output.push('\n');
            } else {
                output.push(' ');
            }
        }
    }

    fn find_cfg_test_attribute(source: &str, start: usize) -> Option<(usize, usize)> {
        let mut cursor = start;
        while let Some(relative_start) = source[cursor..].find("#[") {
            let attribute_start = cursor + relative_start;
            let content_start = attribute_start + 2;
            let content_end = source[content_start..]
                .find(']')
                .map(|relative_end| content_start + relative_end)?;
            let normalized = source[content_start..content_end]
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>();
            let attribute_end = content_end + 1;
            if normalized == "cfg(test)" {
                return Some((attribute_start, attribute_end));
            }
            cursor = attribute_end;
        }
        None
    }

    fn find_cfg_test_item_end(
        path: &str,
        source: &str,
        attribute_start: usize,
        attribute_end: usize,
    ) -> Result<usize, String> {
        let mut cursor = skip_rust_whitespace(source, attribute_end);
        while source[cursor..].starts_with("#[") {
            let attribute_end = source[cursor..]
                .find(']')
                .map(|end| cursor + end + 1)
                .ok_or_else(|| {
                    classification_error(path, source, cursor, "unterminated attribute")
                })?;
            cursor = skip_rust_whitespace(source, attribute_end);
        }

        cursor = skip_optional_visibility(path, source, cursor)?;
        find_rust_item_end(path, source, attribute_start, cursor)
    }

    fn skip_optional_visibility(path: &str, source: &str, cursor: usize) -> Result<usize, String> {
        if !starts_with_rust_keyword(source, cursor, "pub") {
            return Ok(cursor);
        }

        let mut cursor = skip_rust_whitespace(source, cursor + "pub".len());
        if source[cursor..].starts_with('(') {
            cursor = skip_balanced_parentheses(source, cursor).ok_or_else(|| {
                classification_error(path, source, cursor, "malformed visibility")
            })?;
            cursor = skip_rust_whitespace(source, cursor);
        }
        Ok(cursor)
    }

    fn find_rust_item_end(
        path: &str,
        source: &str,
        attribute_start: usize,
        item_start: usize,
    ) -> Result<usize, String> {
        let bytes = source.as_bytes();
        let mut cursor = item_start;
        let mut brace_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut saw_brace = false;

        while cursor < bytes.len() {
            match bytes[cursor] {
                b'(' => paren_depth += 1,
                b')' => {
                    paren_depth = paren_depth.checked_sub(1).ok_or_else(|| {
                        classification_error(path, source, cursor, "unbalanced closing parenthesis")
                    })?;
                }
                b'[' => bracket_depth += 1,
                b']' => {
                    bracket_depth = bracket_depth.checked_sub(1).ok_or_else(|| {
                        classification_error(path, source, cursor, "unbalanced closing bracket")
                    })?;
                }
                b'{' if paren_depth == 0 && bracket_depth == 0 => {
                    brace_depth += 1;
                    saw_brace = true;
                }
                b'}' if paren_depth == 0 && bracket_depth == 0 => {
                    brace_depth = brace_depth.checked_sub(1).ok_or_else(|| {
                        classification_error(path, source, cursor, "unbalanced closing brace")
                    })?;
                    cursor += 1;
                    if saw_brace && brace_depth == 0 {
                        return Ok(cursor);
                    }
                    continue;
                }
                b';' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return Ok(cursor + 1);
                }
                b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return Ok(cursor + 1);
                }
                _ => {}
            }
            cursor += 1;
        }

        Err(classification_error(
            path,
            source,
            attribute_start,
            "cfg(test) item is not closed",
        ))
    }

    fn skip_balanced_parentheses(source: &str, open_index: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (relative_index, character) in source[open_index..].char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(open_index + relative_index + character.len_utf8());
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn skip_rust_whitespace(source: &str, mut cursor: usize) -> usize {
        while cursor < source.len() {
            let Some(character) = source[cursor..].chars().next() else {
                return cursor;
            };
            if !character.is_whitespace() {
                return cursor;
            }
            cursor += character.len_utf8();
        }
        cursor
    }

    fn starts_with_rust_keyword(source: &str, cursor: usize, keyword: &str) -> bool {
        source[cursor..].starts_with(keyword)
            && source[cursor + keyword.len()..]
                .chars()
                .next()
                .is_none_or(|character| !is_rust_identifier_character(character))
    }

    fn skip_rust_block_comment(source: &str, start: usize) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut cursor = start;
        let mut depth = 0usize;

        while cursor + 1 < bytes.len() {
            if bytes[cursor] == b'/' && bytes[cursor + 1] == b'*' {
                depth += 1;
                cursor += 2;
                continue;
            }
            if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
                depth = depth.checked_sub(1)?;
                cursor += 2;
                if depth == 0 {
                    return Some(cursor);
                }
                continue;
            }
            cursor += 1;
        }

        None
    }

    fn skip_quoted_rust_literal(source: &str, start: usize, delimiter: u8) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut cursor = start + 1;
        let mut escaped = false;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == delimiter {
                return Some(cursor + 1);
            }
            cursor += 1;
        }
        None
    }

    fn skip_rust_char_literal(source: &str, start: usize) -> Option<usize> {
        let end = skip_quoted_rust_literal(source, start, b'\'')?;
        let literal = &source[start + 1..end - 1];
        if literal.starts_with('\\') || literal.chars().count() == 1 {
            Some(end)
        } else {
            None
        }
    }

    fn skip_raw_rust_string(source: &str, start: usize) -> Option<usize> {
        let bytes = source.as_bytes();
        let mut cursor = start + 1;
        let mut hashes = 0usize;

        while bytes.get(cursor) == Some(&b'#') {
            hashes += 1;
            cursor += 1;
        }

        if bytes.get(cursor) != Some(&b'"') {
            return None;
        }
        cursor += 1;

        while cursor < bytes.len() {
            if bytes[cursor] == b'"' {
                let mut matched = true;
                for offset in 0..hashes {
                    if bytes.get(cursor + 1 + offset) != Some(&b'#') {
                        matched = false;
                        break;
                    }
                }
                if matched {
                    return Some(cursor + 1 + hashes);
                }
            }
            cursor += 1;
        }

        None
    }

    fn is_rust_identifier_character(character: char) -> bool {
        character == '_' || character.is_ascii_alphanumeric()
    }

    fn line_number(source: &str, index: usize) -> usize {
        source[..index]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1
    }

    fn classification_error(path: &str, source: &str, index: usize, reason: &str) -> String {
        format!(
            "{path}:{} source classification failed: {reason}",
            line_number(source, index)
        )
    }

    fn sample_config(root: &Path, relays: Vec<String>) -> RuntimeConfig {
        let data = root.join("data");
        let cache = root.join("cache");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Json,
                verbosity: Verbosity::Normal,
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
                shared_cache_root: cache.clone(),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
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
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".to_owned(),
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
            transport: crate::runtime::config::TransportConfig::from_nostr_relay_urls(
                relays.clone(),
            ),
            local: LocalConfig {
                root: data.join("apps/cli/replica"),
                replica_store_path: data.join("apps/cli/replica/replica.sqlite"),
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
            mesh: crate::runtime::config::MeshConfig::disabled(),
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".to_owned(),
            },
            rhi: RhiConfig {
                validator_set: None,
                require_cryptographic_proof: false,
            },
            capability_bindings: Vec::new(),
        }
    }
}
