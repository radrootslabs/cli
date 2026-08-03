//! CLI-owned composition of canonical local and NIP-46 signer adapters.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nostr_sdk::prelude::{
    Client as RelayClient, Filter, JsonUtil, Kind, RelayPoolNotification, Timestamp,
};
use radroots_event::{SignedEvent, wire::Nip01EventWire};
use radroots_nostr_connect::{
    Client as NostrConnectClient, Error as NostrConnectError, Request, Response,
    client::{
        CancellationToken, ClientEvent, Completion, Progress, Receive, Target, Transport,
        TransportFuture,
    },
    message::{RequestId, UnsignedEvent},
    uri::{BunkerUri, Uri},
};
use radroots_sdk::signing::Provider;
use radroots_signing::{
    Error as SigningError, SignReceipt, SignRequest, Signer, SignerStatus,
    capability::{CancellationSupport, SignerCapability, SignerKind},
    error::Kind as SigningErrorKind,
    signer::BoxFuture,
    status::{AuthChallenge, SignProgress, SignProgressStage, SignerAvailability},
};
use tokio::{sync::broadcast, time::Instant};
use url::Url;
use zeroize::Zeroizing;

use crate::runtime::{
    RuntimeError, account,
    config::{
        CapabilityBindingTargetKind, RuntimeConfig, SIGNER_REMOTE_NIP46_CAPABILITY, SignerBackend,
    },
};

pub(crate) const MYC_NIP46_SESSION_SECRET_SERVICE: &str = "org.radroots.cli.myc-nip46-session";

pub(crate) fn validate_for_actor(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
    actor_label: &str,
) -> Result<(), RuntimeError> {
    match config.signer.backend {
        SignerBackend::Local => {
            let signing = local_signer(config, actor_account_id)?;
            let signer_pubkey = signing.signer.public_key().to_hex();
            if !signer_pubkey.eq_ignore_ascii_case(actor_pubkey) {
                return Err(account::AccountRuntimeFailure::mismatch(format!(
                    "{actor_label} public key `{actor_pubkey}` does not match local signer account `{}` public key `{signer_pubkey}`",
                    signing.account.record.id()
                ))
                .into());
            }
            Ok(())
        }
        SignerBackend::Myc => remote_input(config, actor_account_id, actor_pubkey).map(|_| ()),
    }
}

pub(crate) async fn provider_for_actor(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
    actor_label: &str,
) -> Result<Provider, RuntimeError> {
    match config.signer.backend {
        SignerBackend::Local => {
            let signing = local_signer(config, actor_account_id)?;
            let signer_pubkey = signing.signer.public_key().to_hex();
            if !signer_pubkey.eq_ignore_ascii_case(actor_pubkey) {
                return Err(account::AccountRuntimeFailure::mismatch(format!(
                    "{actor_label} public key `{actor_pubkey}` does not match local signer account `{}` public key `{signer_pubkey}`",
                    signing.account.record.id()
                ))
                .into());
            }
            Ok(Provider::local(signing.signer))
        }
        SignerBackend::Myc => {
            let input = remote_input(config, actor_account_id, actor_pubkey)?;
            let signer = Nip46Signer::connect(input).await?;
            Ok(Provider::nip46(Arc::new(signer)))
        }
    }
}

fn local_signer(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
) -> Result<account::AccountLocalSigner, RuntimeError> {
    match actor_account_id {
        Some(account_id) => account::resolve_local_signing_identity_for_account(config, account_id),
        None => account::resolve_local_signing_identity(config),
    }
}

struct RemoteInput {
    session_secret: Zeroizing<String>,
    bunker: BunkerUri,
    request_timeout: Duration,
}

fn remote_input(
    config: &RuntimeConfig,
    actor_account_id: Option<&str>,
    actor_pubkey: &str,
) -> Result<RemoteInput, RuntimeError> {
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
    let secret = account::load_secret_backend_secret(
        config,
        signer_session_ref,
        MYC_NIP46_SESSION_SECRET_SERVICE,
    )?
    .ok_or_else(|| {
        RuntimeError::Config(format!(
            "signer.remote_nip46 signer_session_ref `{signer_session_ref}` was not found in the account secret backend"
        ))
    })?;
    if config.myc.status_timeout_ms == 0 {
        return Err(RuntimeError::Config(
            "RADROOTS_CLI_MYC_STATUS_TIMEOUT_MS must be greater than zero".to_owned(),
        ));
    }
    let bunker = parse_target(binding.target.as_str())?;
    if bunker.remote_signer_public_key().to_hex() != actor_pubkey {
        return Err(account::AccountRuntimeFailure::mismatch(format!(
            "remote signer public key `{}` does not match actor public key `{actor_pubkey}`",
            bunker.remote_signer_public_key().to_hex()
        ))
        .into());
    }
    Ok(RemoteInput {
        session_secret: Zeroizing::new(secret),
        bunker,
        request_timeout: Duration::from_millis(config.myc.status_timeout_ms),
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

fn parse_target(value: &str) -> Result<BunkerUri, RuntimeError> {
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
        Url::parse(trimmed)
            .map_err(|error| {
                RuntimeError::Config(format!("signer.remote_nip46 target is invalid: {error}"))
            })?
            .query_pairs()
            .find(|(key, _)| key == "uri")
            .map(|(_, uri)| uri.into_owned())
            .ok_or_else(|| {
                RuntimeError::Config(
                    "signer.remote_nip46 discovery target is missing `uri` query parameter"
                        .to_owned(),
                )
            })?
    };
    match Uri::parse(bunker_uri.as_str())
        .map_err(|error| RuntimeError::Config(format!("signer.remote_nip46 target is invalid: {error}")))?
    {
        Uri::Bunker(bunker) => Ok(bunker),
        Uri::Client(_) => Err(RuntimeError::Config(
            "signer.remote_nip46 target must resolve to a bunker URI; raw nostrconnect client URIs are signer-side only"
                .to_owned(),
        )),
    }
}

struct Nip46Signer {
    client: NostrConnectClient,
    target: Target,
    request_timeout: Duration,
}

impl Nip46Signer {
    async fn connect(input: RemoteInput) -> Result<Self, RuntimeError> {
        let target = Target::try_new(
            input.bunker.remote_signer_public_key(),
            input.bunker.relays().to_vec(),
        )
        .map_err(|error| RuntimeError::Config(error.to_string()))?;
        let client = NostrConnectClient::from_secret(input.session_secret.trim(), target.clone())
            .map_err(|_| {
            RuntimeError::Config("NIP-46 client session secret is invalid".to_owned())
        })?;
        Ok(Self {
            client,
            target,
            request_timeout: input.request_timeout,
        })
    }
}

impl Signer for Nip46Signer {
    fn status(&self) -> BoxFuture<'_, Result<SignerStatus, SigningError>> {
        Box::pin(async {
            Ok(SignerStatus::new(
                SignerAvailability::Ready,
                vec![SignerCapability::new(
                    SignerKind::Remote,
                    CancellationSupport::BeforeAndAfterPublication,
                    true,
                    true,
                )],
                None,
            ))
        })
    }

    fn sign(&self, request: SignRequest) -> BoxFuture<'_, Result<SignReceipt, SigningError>> {
        Box::pin(async move {
            let now =
                unix_time().map_err(|_| SigningError::new(SigningErrorKind::InternalError))?;
            let remaining = request.policy().deadline_unix().saturating_sub(now);
            if remaining == 0 {
                return Err(SigningError::new(SigningErrorKind::DeadlineExceeded));
            }
            request.report_progress(&SignProgress::stage(SignProgressStage::Validating)?);
            let unsigned_json = serde_json::json!({
                "pubkey": request.draft().expected_pubkey().to_hex(),
                "created_at": request.draft().created_at_u64(),
                "kind": request.draft().kind_u32(),
                "tags": request.draft().tags_as_vec(),
                "content": request.draft().content(),
            })
            .to_string();
            let unsigned = UnsignedEvent::from_json(unsigned_json.as_str())
                .map_err(|error| signing_source(SigningErrorKind::InvalidArgument, error))?;
            let mut transport = RelayTransport::connect(
                &self.client,
                &self.target,
                self.request_timeout.min(Duration::from_secs(remaining)),
            )
            .await
            .map_err(|error| signing_source(SigningErrorKind::SignerUnavailable, error))?;
            let request_id = random_request_id()
                .map_err(|error| signing_source(SigningErrorKind::InternalError, error))?;
            request.report_progress(&SignProgress::stage(SignProgressStage::RequestPublished)?);
            let cancellation = CancellationToken::new();
            let completion = self
                .client
                .execute(
                    request_id,
                    Request::SignEvent(unsigned),
                    &mut transport,
                    &cancellation,
                    |progress| report_remote_progress(&request, progress),
                )
                .await
                .map_err(normalize_nip46_error)?;
            finish_remote_signing(&request, completion)
        })
    }
}

fn report_remote_progress(
    request: &SignRequest,
    progress: Progress,
) -> Result<(), NostrConnectError> {
    match progress {
        Progress::AuthChallenge { url } => {
            let now = unix_time().map_err(|error| NostrConnectError::Transport {
                reason: error.to_string(),
            })?;
            let challenge = AuthChallenge::new(url, now, Some(request.policy().deadline_unix()))
                .map_err(|_| NostrConnectError::InvalidClientState {
                    reason: "remote authentication challenge is invalid",
                })?;
            request.report_progress(&SignProgress::authentication(challenge));
        }
    }
    Ok(())
}

fn finish_remote_signing(
    request: &SignRequest,
    completion: Completion,
) -> Result<SignReceipt, SigningError> {
    let Completion::Response(response) = completion else {
        return Err(SigningError::new(SigningErrorKind::SignerCancelled));
    };
    let Response::SignedEvent(event) = *response else {
        return Err(SigningError::new(SigningErrorKind::SignerOutputInvalid));
    };
    request.report_progress(&SignProgress::stage(SignProgressStage::VerifyingOutput)?);
    let raw_json = event.as_json();
    let wire = Nip01EventWire::parse_json(raw_json.as_str())
        .map_err(|error| signing_source(SigningErrorKind::SignerOutputInvalid, error))?;
    let signed = SignedEvent::from_wire_verified_id(wire, raw_json)
        .map_err(|error| signing_source(SigningErrorKind::SignerOutputInvalid, error))?;
    let completed_at =
        unix_time().map_err(|error| signing_source(SigningErrorKind::InternalError, error))?;
    let receipt = SignReceipt::from_signed_event(request, signed, completed_at)?;
    request.report_progress(&SignProgress::stage(SignProgressStage::Complete)?);
    Ok(receipt)
}

fn normalize_nip46_error(error: NostrConnectError) -> SigningError {
    let kind = match error {
        NostrConnectError::RequestTimedOut => SigningErrorKind::SignerTimeout,
        NostrConnectError::WrongRequestId
        | NostrConnectError::WrongResponseSigner
        | NostrConnectError::ReplayedResponse
        | NostrConnectError::InvalidResponseEnvelope { .. }
        | NostrConnectError::InvalidResponsePayload { .. }
        | NostrConnectError::InvalidClientEvent => SigningErrorKind::SignerOutputInvalid,
        NostrConnectError::Transport { .. } => SigningErrorKind::SignerUnavailable,
        _ => SigningErrorKind::InternalError,
    };
    signing_source(kind, error)
}

fn signing_source<E>(kind: SigningErrorKind, source: E) -> SigningError
where
    E: std::error::Error + Send + Sync + 'static,
{
    SigningError::with_source(kind, source)
}

fn random_request_id() -> Result<RequestId, getrandom::Error> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)?;
    let value = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(RequestId::parse(value).expect("hex request ID is bounded and valid"))
}

fn unix_time() -> Result<u64, std::time::SystemTimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
}

struct RelayTransport {
    client: RelayClient,
    notifications: broadcast::Receiver<RelayPoolNotification>,
    request_timeout: Duration,
    deadline: Option<Instant>,
}

impl RelayTransport {
    async fn connect(
        protocol_client: &NostrConnectClient,
        target: &Target,
        request_timeout: Duration,
    ) -> Result<Self, NostrConnectError> {
        let client = RelayClient::default();
        client.automatic_authentication(false);
        for relay in target.relays() {
            client
                .add_relay(relay.to_string())
                .await
                .map_err(transport_error)?;
        }
        let connected = client.try_connect(request_timeout).await;
        if connected.success.is_empty() {
            return Err(NostrConnectError::Transport {
                reason: "no NIP-46 relay accepted the connection".to_owned(),
            });
        }
        let client_public_key = protocol_client.public_key().map_err(|error| error)?;
        let client_public_key = nostr_sdk::prelude::PublicKey::from_slice(
            client_public_key.as_bytes(),
        )
        .map_err(|_| NostrConnectError::InvalidClientState {
            reason: "client public key is invalid",
        })?;
        let filter = Filter::new()
            .kind(Kind::Custom(radroots_nostr_connect::message::RPC_KIND))
            .pubkey(client_public_key)
            .since(Timestamp::now());
        let notifications = client.notifications();
        let subscribed = client
            .subscribe(filter, None)
            .await
            .map_err(transport_error)?;
        if subscribed.success.is_empty() {
            return Err(NostrConnectError::Transport {
                reason: "no NIP-46 relay accepted the response subscription".to_owned(),
            });
        }
        Ok(Self {
            client,
            notifications,
            request_timeout,
            deadline: None,
        })
    }
}

impl Transport for RelayTransport {
    fn publish<'a>(&'a mut self, event: ClientEvent) -> TransportFuture<'a, ()> {
        Box::pin(async move {
            self.deadline = Some(Instant::now() + self.request_timeout);
            let event = nostr_sdk::prelude::Event::from_json(event.as_json())
                .map_err(|_| NostrConnectError::InvalidClientEvent)?;
            let output = self
                .client
                .send_event(&event)
                .await
                .map_err(transport_error)?;
            if output.success.is_empty() {
                return Err(NostrConnectError::Transport {
                    reason: "no NIP-46 relay accepted the request".to_owned(),
                });
            }
            Ok(())
        })
    }

    fn receive<'a>(
        &'a mut self,
        cancellation: &'a CancellationToken,
    ) -> TransportFuture<'a, Receive> {
        Box::pin(async move {
            loop {
                if cancellation.is_cancelled() {
                    return Ok(Receive::Cancelled);
                }
                let Some(deadline) = self.deadline else {
                    return Err(NostrConnectError::InvalidClientState {
                        reason: "request deadline is not initialized",
                    });
                };
                let now = Instant::now();
                if now >= deadline {
                    return Ok(Receive::TimedOut);
                }
                let poll = (deadline - now).min(Duration::from_millis(50));
                match tokio::time::timeout(poll, self.notifications.recv()).await {
                    Err(_) => continue,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        return Err(NostrConnectError::Transport {
                            reason: "NIP-46 relay notification stream closed".to_owned(),
                        });
                    }
                    Ok(Ok(RelayPoolNotification::Event { event, .. })) => {
                        return ClientEvent::from_json(event.as_json().as_str())
                            .map(Receive::event);
                    }
                    Ok(Ok(_)) => continue,
                }
            }
        })
    }
}

fn transport_error(error: impl std::fmt::Display) -> NostrConnectError {
    NostrConnectError::Transport {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use radroots_event::{EventDraft, contract::AuthorRole, envelope::kind::KIND_GEOCHAT};
    use radroots_protocol::runtime::v1::OperationId;
    use radroots_signing::{
        Actor,
        actor::ActorSource,
        request::{CancellationPolicy, ProgressObserver, SignPolicy},
    };

    struct RecordingObserver(Mutex<Vec<SignProgressStage>>);

    impl ProgressObserver for RecordingObserver {
        fn on_progress(&self, progress: &SignProgress) {
            self.0
                .lock()
                .expect("progress lock")
                .push(progress.stage_value());
        }
    }

    fn local_request() -> (radroots_nostr::signing::LocalSigner, SignRequest) {
        let signer = radroots_nostr::signing::LocalSigner::generate().expect("local signer");
        let public_key = signer.public_key();
        let actor = Actor::new(
            public_key,
            ActorSource::ExplicitPublicKey,
            [AuthorRole::Any],
        )
        .expect("actor");
        let now = unix_time().expect("time");
        let draft = EventDraft::new(
            "radroots.social.geochat.v1",
            KIND_GEOCHAT,
            now,
            Vec::new(),
            "CLI NIP-46 fixture",
            public_key.to_hex(),
        )
        .expect("draft");
        let request = SignRequest::new(
            OperationId::SyncPush,
            actor,
            draft,
            SignPolicy::new(now + 120, CancellationPolicy::PreservePublishedRequest)
                .expect("policy"),
        )
        .expect("request");
        (signer, request)
    }

    #[tokio::test]
    async fn remote_happy_response_becomes_an_exact_verified_receipt() {
        let (local, request) = local_request();
        let signed = local.sign(request.clone()).await.expect("local fixture");
        let wire = radroots_nostr_connect::message::SignedEvent::from_json(
            signed.signed_event().raw_json(),
        )
        .expect("NIP-46 signed event");

        let receipt =
            finish_remote_signing(&request, Completion::response(Response::SignedEvent(wire)))
                .expect("remote receipt");

        assert_eq!(receipt.operation_id(), OperationId::SyncPush);
        assert_eq!(
            receipt.signed_event().id_str(),
            request.draft().expected_event_id_hex()
        );
    }

    #[test]
    fn remote_auth_challenge_is_presented_through_the_canonical_observer() {
        let (_, request) = local_request();
        let observer = Arc::new(RecordingObserver(Mutex::new(Vec::new())));
        let request = request.with_progress_observer(observer.clone());

        report_remote_progress(
            &request,
            Progress::AuthChallenge {
                url: "https://signer.example/approve".to_owned(),
            },
        )
        .expect("auth progress");

        assert_eq!(
            observer.0.lock().expect("progress lock").as_slice(),
            &[SignProgressStage::AwaitingAuthentication]
        );
    }

    #[test]
    fn timeout_and_wrong_response_errors_map_to_stable_signing_kinds() {
        assert_eq!(
            normalize_nip46_error(NostrConnectError::RequestTimedOut).kind(),
            SigningErrorKind::SignerTimeout
        );
        assert_eq!(
            normalize_nip46_error(NostrConnectError::WrongResponseSigner).kind(),
            SigningErrorKind::SignerOutputInvalid
        );
    }

    #[test]
    fn exact_account_or_pubkey_binding_is_required() {
        assert!(myc_managed_account_ref_matches(
            "account-a",
            Some("account-a"),
            "aa"
        ));
        assert!(myc_managed_account_ref_matches(
            "aa",
            Some("account-a"),
            "aa"
        ));
        assert!(!myc_managed_account_ref_matches(
            "account-b",
            Some("account-a"),
            "aa"
        ));
    }
}
