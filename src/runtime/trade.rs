use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_authority::RadrootsActorContext;
use radroots_core::{
    RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreUnit, convert_unit_decimal,
};
use radroots_event::contract::RadrootsActorRole;
use radroots_event::ids::{
    RadrootsClassifiedListingAddress, RadrootsDTag, RadrootsInventoryBinId, RadrootsPublicKey,
    RadrootsTradeCandidateId, RadrootsTradeId, RadrootsTradeMutationId,
};
use radroots_event::trade::{
    RADROOTS_TRADE_PROPOSAL_CONTRACT_ID, RADROOTS_TRADE_SCHEMA_VERSION,
    RadrootsFulfillmentProfileV1, RadrootsTradeCancellationProfileV1, RadrootsTradeCandidateLineV1,
    RadrootsTradeCandidateTermsV1, RadrootsTradeEconomicsProfileV1, RadrootsTradeMutationBodyV1,
    RadrootsTradeMutationEnvelopeV1, canonical_trade_mutation_content,
};
use radroots_replica_schema::nostr_event_head::{
    INostrEventHeadFindOne, INostrEventHeadFindOneArgs, NostrEventHeadQueryBindValues,
};
use radroots_replica_schema::trade_product::{ITradeProductFieldsFilter, ITradeProductFindMany};
use radroots_replica_store::{ReplicaSql, nostr_event_head, trade_product};
use radroots_sdk::{
    CancelTradeRequest, DecideCandidateRequest, EvidenceRefreshReceipt, EvidenceView,
    GetTradeRequest, InspectEvidenceRequest, ListTradesRequest, Page, ProposeRevisionRequest,
    RefreshTradeEvidenceRequest, ResumeOperationRequest, SdkIdempotencyKey, SubmitProposalRequest,
    TradeCommandReceipt, TradePrivateArtifactDeleteReceipt, TradePrivateArtifactDeleteRequest,
    TradePrivateArtifactKind, TradePrivateArtifactOpenReceipt, TradePrivateArtifactOpenRequest,
    TradePrivateArtifactSealReceipt, TradePrivateArtifactSealRequest, TradeStatusView,
    TradeSummaryView,
};
use radroots_sql_core::SqlxSqliteExecutor;
use serde::Serialize;

use crate::runtime::RuntimeError;
use crate::runtime::account;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::sdk::{CliSdkAdapterError, CliSdkSession, sdk_target_policy};

const TRADE_CANDIDATE_DRAFTS_DIR: &str = "trades/candidates";
const TRADE_CANDIDATE_DRAFT_SOURCE: &str = "SDK trade proposal candidate draft";
const DEFAULT_PROPOSAL_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
const DEFAULT_FULFILLMENT_START_OFFSET_SECONDS: u64 = 24 * 60 * 60;
const DEFAULT_FULFILLMENT_WINDOW_SECONDS: u64 = 2 * 60 * 60;

static TRADE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct TradeEnvelopeFileRuntimeArgs {
    pub file: PathBuf,
    pub idempotency_key: Option<String>,
    pub acknowledge_private_terms: bool,
    pub operation_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TradeIdRuntimeArgs {
    pub trade_id: String,
}

#[derive(Debug, Clone)]
pub struct TradePageRuntimeArgs {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TradeEvidenceInspectRuntimeArgs {
    pub trade_id: String,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TradePrivateArtifactSealRuntimeArgs {
    pub trade_id: String,
    pub artifact_id: String,
    pub schema_id: String,
    pub input: PathBuf,
    pub kind: String,
    pub candidate_id: Option<String>,
    pub retention_class: Option<String>,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TradePrivateArtifactOpenRuntimeArgs {
    pub artifact_id: String,
    pub output: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TradePrivateArtifactDeleteRuntimeArgs {
    pub artifact_id: String,
}

#[derive(Debug, Clone)]
pub struct TradeCandidateDraftCreateArgs {
    pub listing: Option<String>,
    pub listing_addr: Option<String>,
    pub bin_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeCandidateDraftView {
    pub state: String,
    pub source: String,
    pub trade_id: String,
    pub candidate_id: Option<String>,
    pub mutation_id: Option<String>,
    pub file: String,
    pub listing_addr: String,
    pub listing_event_id: String,
    pub listing_snapshot_sha256: String,
    pub buyer_pubkey: String,
    pub seller_pubkey: String,
    pub farm_id: String,
    pub ready_for_submit: bool,
    pub economics: TradeCandidateDraftEconomicsView,
    pub issues: Vec<TradeCandidateDraftIssue>,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeCandidateDraftEconomicsView {
    pub currency_code: String,
    pub currency_exponent: u32,
    pub subtotal_mantissa: String,
    pub discount_total_mantissa: String,
    pub adjustment_total_mantissa: String,
    pub total_mantissa: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeCandidateDraftIssue {
    pub code: String,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradePrivateArtifactOpenView {
    pub state: String,
    pub artifact_id: String,
    pub trade_id: Option<String>,
    pub candidate_id: Option<String>,
    pub artifact_kind: Option<TradePrivateArtifactKind>,
    pub schema_id: Option<String>,
    pub retention_class: Option<String>,
    pub output: String,
    pub bytes_written: usize,
    pub created_at_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub deleted_at_ms: Option<i64>,
}

pub fn submit_proposal(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradeCommandReceipt, CliSdkAdapterError> {
    let envelope = load_trade_envelope(args.file.as_path())?;
    let (actor, session) = actor_session_for_envelope(config, &envelope, "trade proposal")?;
    let mut request = SubmitProposalRequest::new(actor, envelope, sdk_target_policy(config));
    if let Some(idempotency_key) = idempotency_key(args.idempotency_key.as_deref())? {
        request = request.with_idempotency_key(idempotency_key);
    }
    Ok(session.block_on(session.sdk().trades().commands().submit_proposal(request))?)
}

pub fn propose_revision(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradeCommandReceipt, CliSdkAdapterError> {
    let envelope = load_trade_envelope(args.file.as_path())?;
    let (actor, session) = actor_session_for_envelope(config, &envelope, "trade revision")?;
    let mut request = ProposeRevisionRequest::new(actor, envelope, sdk_target_policy(config));
    if let Some(idempotency_key) = idempotency_key(args.idempotency_key.as_deref())? {
        request = request.with_idempotency_key(idempotency_key);
    }
    Ok(session.block_on(session.sdk().trades().commands().propose_revision(request))?)
}

pub fn decide_candidate(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradeCommandReceipt, CliSdkAdapterError> {
    let envelope = load_trade_envelope(args.file.as_path())?;
    let (actor, session) =
        actor_session_for_envelope(config, &envelope, "trade candidate decision")?;
    let mut request = DecideCandidateRequest::new(actor, envelope, sdk_target_policy(config));
    if args.acknowledge_private_terms {
        request = request.acknowledge_private_terms();
    }
    if let Some(idempotency_key) = idempotency_key(args.idempotency_key.as_deref())? {
        request = request.with_idempotency_key(idempotency_key);
    }
    Ok(session.block_on(session.sdk().trades().commands().decide_candidate(request))?)
}

pub fn cancel_trade(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradeCommandReceipt, CliSdkAdapterError> {
    let envelope = load_trade_envelope(args.file.as_path())?;
    let (actor, session) = actor_session_for_envelope(config, &envelope, "trade cancellation")?;
    let mut request = CancelTradeRequest::new(actor, envelope, sdk_target_policy(config));
    if let Some(idempotency_key) = idempotency_key(args.idempotency_key.as_deref())? {
        request = request.with_idempotency_key(idempotency_key);
    }
    Ok(session.block_on(session.sdk().trades().commands().cancel_trade(request))?)
}

pub fn resume_operation(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradeCommandReceipt, CliSdkAdapterError> {
    let operation_kind = args.operation_kind.as_deref().ok_or_else(|| {
        RuntimeError::Config("trade operation resume requires `--operation-kind`".to_owned())
    })?;
    let envelope = load_trade_envelope(args.file.as_path())?;
    let (actor, session) = actor_session_for_envelope(config, &envelope, "trade operation resume")?;
    let mut request = ResumeOperationRequest::new(
        actor,
        envelope,
        resume_operation_kind(operation_kind)?,
        sdk_target_policy(config),
    );
    if args.acknowledge_private_terms {
        request = request.acknowledge_private_terms();
    }
    if let Some(idempotency_key) = idempotency_key(args.idempotency_key.as_deref())? {
        request = request.with_idempotency_key(idempotency_key);
    }
    Ok(session.block_on(session.sdk().trades().commands().resume_operation(request))?)
}

pub fn get_trade(
    config: &RuntimeConfig,
    args: &TradeIdRuntimeArgs,
) -> Result<TradeStatusView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let trade_id = trade_id(args.trade_id.as_str(), "trade_id")?;
    let request = GetTradeRequest::new(trade_id);
    Ok(session.block_on(session.sdk().trades().queries().get_trade(request))?)
}

pub fn list_trades(
    config: &RuntimeConfig,
    args: &TradePageRuntimeArgs,
) -> Result<Page<TradeSummaryView>, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let mut request = ListTradesRequest::new();
    if let Some(limit) = args.limit {
        request = request.with_limit(limit);
    }
    if let Some(cursor) = args.cursor.as_deref().and_then(non_empty_ref) {
        request = request.with_cursor(cursor);
    }
    Ok(session.block_on(session.sdk().trades().queries().list_trades(request))?)
}

pub fn refresh_evidence(
    config: &RuntimeConfig,
    args: &TradeIdRuntimeArgs,
) -> Result<EvidenceRefreshReceipt, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let trade_id = trade_id(args.trade_id.as_str(), "trade_id")?;
    let request = RefreshTradeEvidenceRequest::new(trade_id);
    Ok(session.block_on(session.sdk().trades().queries().refresh_evidence(request))?)
}

pub fn inspect_evidence(
    config: &RuntimeConfig,
    args: &TradeEvidenceInspectRuntimeArgs,
) -> Result<Page<EvidenceView>, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let mut request = InspectEvidenceRequest::new(trade_id(args.trade_id.as_str(), "trade_id")?);
    if let Some(limit) = args.limit {
        request = request.with_limit(limit);
    }
    if let Some(cursor) = args.cursor.as_deref().and_then(non_empty_ref) {
        request = request.with_cursor(cursor);
    }
    Ok(session.block_on(session.sdk().trades().queries().inspect_evidence(request))?)
}

pub fn seal_private_artifact(
    config: &RuntimeConfig,
    args: &TradePrivateArtifactSealRuntimeArgs,
) -> Result<TradePrivateArtifactSealReceipt, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let plaintext = fs::read(args.input.as_path()).map_err(RuntimeError::from)?;
    let mut request = TradePrivateArtifactSealRequest::binding_terms(
        args.artifact_id.as_str(),
        trade_id(args.trade_id.as_str(), "trade_id")?,
        args.schema_id.as_str(),
        plaintext,
    );
    request.artifact_kind = private_artifact_kind(args.kind.as_str())?;
    if let Some(candidate_id) = args.candidate_id.as_deref().and_then(non_empty_ref) {
        request = request.with_candidate_id(trade_candidate_id(candidate_id, "candidate_id")?);
    }
    if let Some(retention_class) = args.retention_class.as_deref().and_then(non_empty_ref) {
        request = request.with_retention_class(retention_class);
    }
    if let Some(expires_at_ms) = args.expires_at_ms {
        request = request.with_expires_at_ms(expires_at_ms);
    }
    Ok(session.block_on(session.sdk().trades().seal_private_artifact(request))?)
}

pub fn open_private_artifact(
    config: &RuntimeConfig,
    args: &TradePrivateArtifactOpenRuntimeArgs,
) -> Result<TradePrivateArtifactOpenView, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let request = TradePrivateArtifactOpenRequest::new(args.artifact_id.as_str());
    let receipt = session.block_on(session.sdk().trades().open_private_artifact(request))?;
    Ok(match receipt {
        Some(receipt) => private_artifact_open_view(args, receipt)?,
        None => TradePrivateArtifactOpenView {
            state: "missing".to_owned(),
            artifact_id: args.artifact_id.clone(),
            trade_id: None,
            candidate_id: None,
            artifact_kind: None,
            schema_id: None,
            retention_class: None,
            output: args.output.display().to_string(),
            bytes_written: 0,
            created_at_ms: None,
            expires_at_ms: None,
            deleted_at_ms: None,
        },
    })
}

pub fn delete_private_artifact(
    config: &RuntimeConfig,
    args: &TradePrivateArtifactDeleteRuntimeArgs,
) -> Result<TradePrivateArtifactDeleteReceipt, CliSdkAdapterError> {
    let session = CliSdkSession::connect(config)?;
    let request = TradePrivateArtifactDeleteRequest::new(args.artifact_id.as_str());
    Ok(session.block_on(session.sdk().trades().delete_private_artifact(request))?)
}

pub fn scaffold_proposal_draft(
    config: &RuntimeConfig,
    args: &TradeCandidateDraftCreateArgs,
) -> Result<TradeCandidateDraftView, RuntimeError> {
    scaffold_proposal_draft_inner(config, args, false)
}

pub fn scaffold_proposal_draft_preflight(
    config: &RuntimeConfig,
    args: &TradeCandidateDraftCreateArgs,
) -> Result<TradeCandidateDraftView, RuntimeError> {
    scaffold_proposal_draft_inner(config, args, true)
}

fn scaffold_proposal_draft_inner(
    config: &RuntimeConfig,
    args: &TradeCandidateDraftCreateArgs,
    dry_run: bool,
) -> Result<TradeCandidateDraftView, RuntimeError> {
    if args.quantity == 0 {
        return Err(RuntimeError::Config(
            "basket item quantity must be greater than zero".to_owned(),
        ));
    }
    let buyer = account::resolve_account(config)?.ok_or_else(|| {
        RuntimeError::Config("trade proposal draft requires a selected buyer account".to_owned())
    })?;
    let product = resolve_product(config, args)?;
    let parsed_listing = parse_listing_addr(product.listing_addr.as_str())?;
    let listing_state =
        resolve_active_listing_state(config, product.listing_addr.as_str(), &parsed_listing)?;
    let farm_id = resolve_farm_id(config, parsed_listing.seller_pubkey.as_str())?;
    let buyer_pubkey = pubkey(
        buyer.record.public_identity.public_key_hex.as_str(),
        "buyer_pubkey",
    )?;
    let seller_pubkey = pubkey(parsed_listing.seller_pubkey.as_str(), "seller_pubkey")?;
    let candidate = candidate_terms(
        &product,
        &listing_state,
        buyer_pubkey.clone(),
        seller_pubkey.clone(),
        farm_id.clone(),
        args.quantity,
    )?;
    let envelope = RadrootsTradeMutationEnvelopeV1 {
        mutation_id: None,
        contract_id: RADROOTS_TRADE_PROPOSAL_CONTRACT_ID.to_owned(),
        schema_version: RADROOTS_TRADE_SCHEMA_VERSION,
        trade_id: next_trade_id()?,
        root_mutation_id: None,
        buyer_pubkey,
        seller_pubkey: seller_pubkey.clone(),
        farm_id: farm_id.clone(),
        parent_mutation_ids: Vec::new(),
        author_pubkey: pubkey(
            buyer.record.public_identity.public_key_hex.as_str(),
            "author_pubkey",
        )?,
        counterparty_pubkey: seller_pubkey,
        authored_at_unix_s: now_unix(),
        body: RadrootsTradeMutationBodyV1::Proposal { candidate },
    };
    let canonical = canonical_trade_mutation_content(envelope)
        .map_err(|error| RuntimeError::Config(format!("build trade proposal envelope: {error}")))?;
    let file = candidate_draft_file(config, canonical.envelope.trade_id.as_str());
    if !dry_run {
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(&canonical.envelope)?;
        fs::write(file.as_path(), contents)?;
    }
    let economics = proposal_economics_view(&canonical.envelope);
    let candidate_id = proposal_candidate_id(&canonical.envelope);
    Ok(TradeCandidateDraftView {
        state: if dry_run { "dry_run" } else { "drafted" }.to_owned(),
        source: TRADE_CANDIDATE_DRAFT_SOURCE.to_owned(),
        trade_id: canonical.envelope.trade_id.to_string(),
        candidate_id,
        mutation_id: canonical
            .envelope
            .mutation_id
            .as_ref()
            .map(ToString::to_string),
        file: file.display().to_string(),
        listing_addr: product.listing_addr,
        listing_event_id: listing_state.last_event_id,
        listing_snapshot_sha256: listing_state.content_hash,
        buyer_pubkey: buyer.record.public_identity.public_key_hex,
        seller_pubkey: parsed_listing.seller_pubkey,
        farm_id: farm_id.to_string(),
        ready_for_submit: true,
        economics,
        issues: Vec::new(),
        actions: if dry_run {
            Vec::new()
        } else {
            vec![format!("radroots trade proposal submit {}", file.display())]
        },
    })
}

fn load_trade_envelope(path: &Path) -> Result<RadrootsTradeMutationEnvelopeV1, RuntimeError> {
    let contents = fs::read_to_string(path)?;
    serde_json::from_str(contents.as_str()).map_err(|error| {
        RuntimeError::Config(format!(
            "invalid trade envelope {}: {error}",
            path.display()
        ))
    })
}

fn actor_session_for_envelope(
    config: &RuntimeConfig,
    envelope: &RadrootsTradeMutationEnvelopeV1,
    operation: &str,
) -> Result<(RadrootsActorContext, CliSdkSession), CliSdkAdapterError> {
    let account = account::resolve_account(config)?.ok_or_else(|| {
        RuntimeError::Config(format!("{operation} requires a selected signer account"))
    })?;
    let author_pubkey = envelope.author_pubkey.as_str();
    let account_pubkey = account.record.public_identity.public_key_hex.as_str();
    if !account_pubkey.eq_ignore_ascii_case(author_pubkey) {
        return Err(RuntimeError::Config(format!(
            "{operation} envelope author `{author_pubkey}` does not match selected account `{}` public key `{account_pubkey}`",
            account.record.account_id
        ))
        .into());
    }
    let role = if envelope
        .buyer_pubkey
        .as_str()
        .eq_ignore_ascii_case(author_pubkey)
    {
        RadrootsActorRole::Buyer
    } else if envelope
        .seller_pubkey
        .as_str()
        .eq_ignore_ascii_case(author_pubkey)
    {
        RadrootsActorRole::Seller
    } else {
        return Err(RuntimeError::Config(format!(
            "{operation} envelope author must be the buyer or seller"
        ))
        .into());
    };
    let actor = RadrootsActorContext::local_account(
        author_pubkey,
        account.record.account_id.to_string(),
        [role],
    )
    .map_err(|error| RuntimeError::Config(format!("invalid trade SDK actor: {error}")))?;
    let session = CliSdkSession::connect_for_actor(
        config,
        Some(account.record.account_id.as_str()),
        author_pubkey,
        operation,
    )?;
    Ok((actor, session))
}

fn idempotency_key(value: Option<&str>) -> Result<Option<SdkIdempotencyKey>, RuntimeError> {
    value
        .and_then(non_empty_ref)
        .map(SdkIdempotencyKey::new)
        .transpose()
        .map_err(|error| RuntimeError::Config(error.to_string()))
}

fn resume_operation_kind(value: &str) -> Result<&'static str, RuntimeError> {
    match value {
        "trade.submit_proposal.v1" => Ok("trade.submit_proposal.v1"),
        "trade.propose_revision.v1" => Ok("trade.propose_revision.v1"),
        "trade.decide_candidate.v1" => Ok("trade.decide_candidate.v1"),
        "trade.cancel.v1" => Ok("trade.cancel.v1"),
        other => Err(RuntimeError::Config(format!(
            "unsupported trade operation kind `{other}`"
        ))),
    }
}

fn private_artifact_open_view(
    args: &TradePrivateArtifactOpenRuntimeArgs,
    receipt: TradePrivateArtifactOpenReceipt,
) -> Result<TradePrivateArtifactOpenView, RuntimeError> {
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(args.output.as_path(), &receipt.plaintext)?;
    Ok(TradePrivateArtifactOpenView {
        state: "opened".to_owned(),
        artifact_id: receipt.artifact_id,
        trade_id: Some(receipt.trade_id.to_string()),
        candidate_id: receipt.candidate_id.as_ref().map(ToString::to_string),
        artifact_kind: Some(receipt.artifact_kind),
        schema_id: Some(receipt.schema_id),
        retention_class: Some(receipt.retention_class),
        output: args.output.display().to_string(),
        bytes_written: receipt.plaintext.len(),
        created_at_ms: Some(receipt.created_at_ms),
        expires_at_ms: receipt.expires_at_ms,
        deleted_at_ms: receipt.deleted_at_ms,
    })
}

fn private_artifact_kind(value: &str) -> Result<TradePrivateArtifactKind, RuntimeError> {
    match value {
        "binding_terms" => Ok(TradePrivateArtifactKind::BindingTerms),
        "message" => Ok(TradePrivateArtifactKind::Message),
        "contact_bundle" => Ok(TradePrivateArtifactKind::ContactBundle),
        "delivery_instruction" => Ok(TradePrivateArtifactKind::DeliveryInstruction),
        other => Err(RuntimeError::Config(format!(
            "unsupported private artifact kind `{other}`"
        ))),
    }
}

#[derive(Debug, Clone)]
struct ProductFacts {
    key: String,
    qty_amt_exact: String,
    qty_unit: String,
    price_amt_exact: String,
    price_currency: String,
    price_qty_amt_exact: String,
    price_qty_unit: String,
    listing_addr: String,
    primary_bin_id: String,
}

#[derive(Debug, Clone)]
struct ParsedListingAddress {
    kind: u32,
    seller_pubkey: String,
    listing_id: String,
}

#[derive(Debug, Clone)]
struct ActiveListingState {
    last_event_id: String,
    content_hash: String,
}

fn resolve_product(
    config: &RuntimeConfig,
    args: &TradeCandidateDraftCreateArgs,
) -> Result<ProductFacts, RuntimeError> {
    if !config.local.replica_store_path.exists() {
        return Err(RuntimeError::Config(
            "trade proposal draft requires local market data; run `radroots store inspect` and `radroots market pull`".to_owned(),
        ));
    }
    if let Some(listing_addr) = args.listing_addr.as_deref().and_then(non_empty_ref) {
        let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
        let rows = trade_product::find_many(
            &executor,
            &ITradeProductFindMany {
                filter: Some(trade_product_listing_addr_filter(listing_addr)),
            },
        )
        .map_err(|error| RuntimeError::Config(format!("resolve listing product state: {error:?}")))?
        .results;
        let product = match rows.len() {
            0 => {
                return Err(RuntimeError::Config(format!(
                    "listing address `{listing_addr}` is not available in the local replica"
                )));
            }
            1 => rows.into_iter().next().expect("one row"),
            count => {
                return Err(RuntimeError::Config(format!(
                    "listing address `{listing_addr}` matched {count} local listing rows"
                )));
            }
        };
        let facts = ProductFacts {
            key: product.key,
            qty_amt_exact: required(product.qty_amt_exact, "qty_amt_exact")?,
            qty_unit: product.qty_unit,
            price_amt_exact: required(product.price_amt_exact, "price_amt_exact")?,
            price_currency: product.price_currency,
            price_qty_amt_exact: required(product.price_qty_amt_exact, "price_qty_amt_exact")?,
            price_qty_unit: product.price_qty_unit,
            listing_addr: required(product.listing_addr, "listing_addr")?,
            primary_bin_id: verified_primary_bin(
                product.primary_bin_id,
                product.verified_primary_bin_id,
            )?,
        };
        validate_product_bin(&facts, args.bin_id.as_str())?;
        return Ok(facts);
    }
    let listing = args
        .listing
        .as_deref()
        .and_then(non_empty_ref)
        .ok_or_else(|| {
            RuntimeError::Config(
                "trade proposal draft requires `listing` or `listing_addr`".to_owned(),
            )
        })?;
    let db = ReplicaSql::new(SqlxSqliteExecutor::open(&config.local.replica_store_path)?);
    let rows = db.trade_product_lookup(listing)?;
    let product = match rows.len() {
        0 => {
            return Err(RuntimeError::Config(format!(
                "listing `{listing}` is not available in the local replica"
            )));
        }
        1 => rows.into_iter().next().expect("one row"),
        count => {
            return Err(RuntimeError::Config(format!(
                "listing `{listing}` matched {count} local listings"
            )));
        }
    };
    let facts = ProductFacts {
        key: product.key,
        qty_amt_exact: required(product.qty_amt_exact, "qty_amt_exact")?,
        qty_unit: product.qty_unit,
        price_amt_exact: required(product.price_amt_exact, "price_amt_exact")?,
        price_currency: product.price_currency,
        price_qty_amt_exact: required(product.price_qty_amt_exact, "price_qty_amt_exact")?,
        price_qty_unit: product.price_qty_unit,
        listing_addr: required(product.listing_addr, "listing_addr")?,
        primary_bin_id: verified_primary_bin(
            product.primary_bin_id,
            product.verified_primary_bin_id,
        )?,
    };
    validate_product_bin(&facts, args.bin_id.as_str())?;
    Ok(facts)
}

fn candidate_terms(
    product: &ProductFacts,
    listing_state: &ActiveListingState,
    buyer_pubkey: RadrootsPublicKey,
    seller_pubkey: RadrootsPublicKey,
    farm_id: RadrootsDTag,
    bin_count: u32,
) -> Result<RadrootsTradeCandidateTermsV1, RuntimeError> {
    let currency = product
        .price_currency
        .parse::<RadrootsCoreCurrency>()
        .map_err(|error| {
            RuntimeError::Config(format!("listing price_currency is invalid: {error}"))
        })?;
    let quantity_amount = exact_positive_decimal(product.qty_amt_exact.as_str(), "qty_amt_exact")?
        * RadrootsCoreDecimal::from(bin_count);
    let quantity_unit = product
        .qty_unit
        .parse::<RadrootsCoreUnit>()
        .map_err(|error| RuntimeError::Config(format!("listing qty_unit is invalid: {error}")))?;
    let price_amount = exact_positive_decimal(product.price_amt_exact.as_str(), "price_amt_exact")?;
    let price_quantity_amount =
        exact_positive_decimal(product.price_qty_amt_exact.as_str(), "price_qty_amt_exact")?;
    let price_unit = product
        .price_qty_unit
        .parse::<RadrootsCoreUnit>()
        .map_err(|error| {
            RuntimeError::Config(format!("listing price_qty_unit is invalid: {error}"))
        })?;
    let quantity_unit_in_price_units =
        convert_unit_decimal(RadrootsCoreDecimal::ONE, quantity_unit, price_unit).map_err(
            |error| {
                RuntimeError::Config(format!(
                    "listing quantity and price units are incompatible: {error}"
                ))
            },
        )?;
    let unit_price_amount = (price_amount / price_quantity_amount) * quantity_unit_in_price_units;
    let subtotal = unit_price_amount * quantity_amount;
    let quantity_scale = u8::try_from(quantity_amount.scale())
        .map_err(|_| RuntimeError::Config("trade quantity scale exceeds u8".to_owned()))?;
    let currency_exponent = currency.minor_unit_exponent();
    let subtotal_mantissa = decimal_mantissa_at_scale(subtotal, currency_exponent);
    let line = RadrootsTradeCandidateLineV1 {
        line_id: RadrootsDTag::parse("line-1")
            .map_err(|error| RuntimeError::Config(format!("invalid line id: {error}")))?,
        listing_addr: RadrootsClassifiedListingAddress::parse(product.listing_addr.as_str())
            .map_err(|error| RuntimeError::Config(format!("invalid listing address: {error}")))?,
        listing_event_id: listing_state
            .last_event_id
            .parse()
            .map_err(|error| RuntimeError::Config(format!("invalid listing event id: {error}")))?,
        listing_snapshot_sha256: listing_state.content_hash.clone(),
        product_id: product.key.clone(),
        option_id: None,
        bin_id: inventory_bin_id(product.primary_bin_id.as_str(), "bin_id")?,
        quantity_mantissa: decimal_mantissa_at_scale(quantity_amount, u32::from(quantity_scale)),
        quantity_scale,
        unit_code: product.qty_unit.clone(),
        unit_profile: "radroots-core-unit".to_owned(),
        unit_price_mantissa: decimal_mantissa_at_scale(unit_price_amount, currency_exponent),
        currency_code: product.price_currency.clone(),
        line_subtotal_mantissa: subtotal_mantissa.clone(),
        replaces_line_id: None,
    };
    let now = now_unix();
    Ok(RadrootsTradeCandidateTermsV1 {
        candidate_id: None,
        schema_version: RADROOTS_TRADE_SCHEMA_VERSION,
        base_candidate_id: None,
        supersession_intent: None,
        buyer_pubkey,
        seller_pubkey,
        farm_id,
        lines: vec![line],
        line_tombstones: Vec::new(),
        economics: RadrootsTradeEconomicsProfileV1 {
            profile_id: "mvp-fixed".to_owned(),
            currency_code: product.price_currency.clone(),
            currency_exponent: u8::try_from(currency_exponent)
                .map_err(|_| RuntimeError::Config("currency exponent exceeds u8".to_owned()))?,
            rounding_profile: "half-even".to_owned(),
            subtotal_mantissa: subtotal_mantissa.clone(),
            discount_total_mantissa: "0".to_owned(),
            adjustment_total_mantissa: "0".to_owned(),
            total_mantissa: subtotal_mantissa,
            adjustments: Vec::new(),
        },
        fulfillment: RadrootsFulfillmentProfileV1 {
            profile_id: "market-pickup".to_owned(),
            method: "pickup".to_owned(),
            starts_at_unix_s: now + DEFAULT_FULFILLMENT_START_OFFSET_SECONDS,
            ends_at_unix_s: now
                + DEFAULT_FULFILLMENT_START_OFFSET_SECONDS
                + DEFAULT_FULFILLMENT_WINDOW_SECONDS,
            timezone: "UTC".to_owned(),
            utc_offset_seconds: 0,
            fold: 0,
            location_class: "seller_public_listing".to_owned(),
            requires_private_terms: false,
        },
        cancellation: RadrootsTradeCancellationProfileV1 {
            profile_id: "buyer-pre-agreement".to_owned(),
            buyer_pre_agreement: true,
            post_agreement_cutoff_unix_s: None,
        },
        private_terms: None,
        proposal_expires_at_unix_s: now + DEFAULT_PROPOSAL_TTL_SECONDS,
    })
}

fn resolve_active_listing_state(
    config: &RuntimeConfig,
    listing_addr: &str,
    parsed: &ParsedListingAddress,
) -> Result<ActiveListingState, RuntimeError> {
    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
    let key = format!(
        "{}:{}:{}",
        parsed.kind, parsed.seller_pubkey, parsed.listing_id
    );
    let state = nostr_event_head::find_one(
        &executor,
        &INostrEventHeadFindOne::On(INostrEventHeadFindOneArgs {
            on: NostrEventHeadQueryBindValues::Key { key },
        }),
    )
    .map_err(|error| RuntimeError::Config(format!("resolve listing event state: {error:?}")))?
    .result
    .ok_or_else(|| {
        RuntimeError::Config(format!(
            "listing address `{listing_addr}` is missing latest listing event state; run `radroots market pull`"
        ))
    })?;
    RadrootsTradeMutationId::parse(state.content_hash.as_str()).map_err(|error| {
        RuntimeError::Config(format!(
            "listing content hash is not a 32-byte hex digest: {error}"
        ))
    })?;
    state
        .last_event_id
        .parse::<radroots_event::ids::RadrootsEventId>()
        .map_err(|error| {
            RuntimeError::Config(format!("listing latest event id is invalid: {error}"))
        })?;
    Ok(ActiveListingState {
        last_event_id: state.last_event_id,
        content_hash: state.content_hash,
    })
}

fn resolve_farm_id(
    config: &RuntimeConfig,
    seller_pubkey: &str,
) -> Result<RadrootsDTag, RuntimeError> {
    let db = ReplicaSql::new(SqlxSqliteExecutor::open(&config.local.replica_store_path)?);
    let d_tag = db.farm_unique_d_tag_by_pubkey(seller_pubkey)?.ok_or_else(|| {
        RuntimeError::Config(format!(
            "seller `{seller_pubkey}` must have exactly one farm profile in the local replica before creating a trade proposal draft"
        ))
    })?;
    RadrootsDTag::parse(d_tag.as_str())
        .map_err(|error| RuntimeError::Config(format!("farm d tag is invalid: {error}")))
}

fn parse_listing_addr(raw: &str) -> Result<ParsedListingAddress, RuntimeError> {
    let parsed = RadrootsClassifiedListingAddress::parse(raw)
        .map_err(|error| RuntimeError::Config(format!("listing address is invalid: {error}")))?;
    let (kind, rest) = parsed
        .as_str()
        .split_once(':')
        .ok_or_else(|| RuntimeError::Config("listing address has invalid format".to_owned()))?;
    let (seller_pubkey, listing_id) = rest
        .split_once(':')
        .ok_or_else(|| RuntimeError::Config("listing address has invalid format".to_owned()))?;
    let kind = kind
        .parse::<u32>()
        .map_err(|_| RuntimeError::Config("listing address kind is invalid".to_owned()))?;
    Ok(ParsedListingAddress {
        kind,
        seller_pubkey: seller_pubkey.to_owned(),
        listing_id: listing_id.to_owned(),
    })
}

fn proposal_economics_view(
    envelope: &RadrootsTradeMutationEnvelopeV1,
) -> TradeCandidateDraftEconomicsView {
    let RadrootsTradeMutationBodyV1::Proposal { candidate } = &envelope.body else {
        unreachable!("proposal draft envelope is a proposal")
    };
    TradeCandidateDraftEconomicsView {
        currency_code: candidate.economics.currency_code.clone(),
        currency_exponent: u32::from(candidate.economics.currency_exponent),
        subtotal_mantissa: candidate.economics.subtotal_mantissa.clone(),
        discount_total_mantissa: candidate.economics.discount_total_mantissa.clone(),
        adjustment_total_mantissa: candidate.economics.adjustment_total_mantissa.clone(),
        total_mantissa: candidate.economics.total_mantissa.clone(),
    }
}

fn proposal_candidate_id(envelope: &RadrootsTradeMutationEnvelopeV1) -> Option<String> {
    let RadrootsTradeMutationBodyV1::Proposal { candidate } = &envelope.body else {
        return None;
    };
    candidate.candidate_id.as_ref().map(ToString::to_string)
}

fn trade_product_listing_addr_filter(listing_addr: &str) -> ITradeProductFieldsFilter {
    ITradeProductFieldsFilter {
        id: None,
        created_at: None,
        updated_at: None,
        key: None,
        category: None,
        title: None,
        summary: None,
        process: None,
        lot: None,
        profile: None,
        year: None,
        qty_amt: None,
        qty_amt_exact: None,
        qty_unit: None,
        qty_label: None,
        qty_avail: None,
        price_amt: None,
        price_amt_exact: None,
        price_currency: None,
        price_qty_amt: None,
        price_qty_amt_exact: None,
        price_qty_unit: None,
        listing_addr: Some(listing_addr.to_owned()),
        primary_bin_id: None,
        verified_primary_bin_id: None,
        notes: None,
    }
}

fn required(value: Option<String>, field: &str) -> Result<String, RuntimeError> {
    value
        .and_then(|value| non_empty_ref(value.as_str()).map(str::to_owned))
        .ok_or_else(|| RuntimeError::Config(format!("listing {field} is missing")))
}

fn verified_primary_bin(
    primary_bin_id: Option<String>,
    verified_primary_bin_id: Option<String>,
) -> Result<String, RuntimeError> {
    let primary = required(primary_bin_id, "primary_bin_id")?;
    let verified = required(verified_primary_bin_id, "verified_primary_bin_id")?;
    if primary != verified {
        return Err(RuntimeError::Config(format!(
            "listing primary bin `{primary}` does not match verified primary bin `{verified}`"
        )));
    }
    Ok(primary)
}

fn validate_product_bin(product: &ProductFacts, bin_id: &str) -> Result<(), RuntimeError> {
    if product.primary_bin_id != bin_id {
        return Err(RuntimeError::Config(format!(
            "basket bin `{bin_id}` does not match listing primary bin `{}`",
            product.primary_bin_id
        )));
    }
    Ok(())
}

fn exact_positive_decimal(value: &str, field: &str) -> Result<RadrootsCoreDecimal, RuntimeError> {
    let parsed = value
        .trim()
        .parse::<RadrootsCoreDecimal>()
        .map_err(|error| RuntimeError::Config(format!("listing {field} is invalid: {error}")))?;
    if parsed.is_zero() || parsed.is_sign_negative() {
        return Err(RuntimeError::Config(format!(
            "listing {field} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn decimal_mantissa_at_scale(mut value: RadrootsCoreDecimal, scale: u32) -> String {
    value.rescale(scale);
    value.0.mantissa().to_string()
}

fn candidate_draft_file(config: &RuntimeConfig, trade_id: &str) -> PathBuf {
    config
        .paths
        .app_data_root
        .join(TRADE_CANDIDATE_DRAFTS_DIR)
        .join(format!("{trade_id}.json"))
}

fn next_trade_id() -> Result<RadrootsTradeId, RuntimeError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = u128::from(TRADE_COUNTER.fetch_add(1, Ordering::Relaxed));
    let value = format!("{:032x}", nanos ^ counter);
    trade_id(value.as_str(), "trade_id")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn trade_id(value: &str, field: &str) -> Result<RadrootsTradeId, RuntimeError> {
    RadrootsTradeId::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn trade_candidate_id(value: &str, field: &str) -> Result<RadrootsTradeCandidateId, RuntimeError> {
    RadrootsTradeCandidateId::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn inventory_bin_id(value: &str, field: &str) -> Result<RadrootsInventoryBinId, RuntimeError> {
    RadrootsInventoryBinId::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn pubkey(value: &str, field: &str) -> Result<RadrootsPublicKey, RuntimeError> {
    RadrootsPublicKey::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn non_empty_ref(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}
