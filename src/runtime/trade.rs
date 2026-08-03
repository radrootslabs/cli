use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_core::unit::convert_unit_decimal;
use radroots_core::{Currency, Decimal, Unit};
use radroots_event::contract::AuthorRole;
use radroots_event::id::{
    CandidateId, ClassifiedListingAddress, DTag, InventoryBinId, MutationId, TradeId,
};
use radroots_event::trade::{
    FulfillmentProfileV1, RADROOTS_TRADE_PROPOSAL_CONTRACT_ID, RADROOTS_TRADE_SCHEMA_VERSION,
    TradeCancellationProfileV1, TradeCandidateLineV1, TradeCandidateTermsV1,
    TradeEconomicsProfileV1, TradeMutationBodyV1, TradeMutationEnvelopeV1,
    canonical_trade_mutation_content,
};
use radroots_identity::PublicKey;
use radroots_replica_schema::nostr_event_head::{
    INostrEventHeadFindOne, INostrEventHeadFindOneArgs, NostrEventHeadQueryBindValues,
};
use radroots_replica_schema::trade_product::{ITradeProductFieldsFilter, ITradeProductFindMany};
use radroots_replica_store::{ReplicaSql, nostr_event_head, trade_product};
use radroots_sdk::trade::{self as sdk_trade, Plan as TradePlan};
use radroots_signing::{Actor, actor::ActorSource};
use radroots_sql_core::SqlxSqliteExecutor;
use serde::Serialize;

use crate::runtime::RuntimeError;
use crate::runtime::account;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::sdk::{CliSdkAdapterError, validate_configured_signer_for_actor};

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
    pub artifact_kind: Option<String>,
    pub schema_id: Option<String>,
    pub retention_class: Option<String>,
    pub output: String,
    pub bytes_written: usize,
    pub created_at_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradePreparedView {
    pub state: String,
    pub operation: String,
    pub trade_id: String,
    pub mutation_id: String,
    pub mutation_kind: String,
    pub event_id: String,
    pub event_kind: u32,
    pub author: String,
    pub required_actions: Vec<String>,
    pub idempotency_key: Option<String>,
    pub reason: Option<String>,
    pub actions: Vec<String>,
}

pub fn submit_proposal(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradePreparedView, CliSdkAdapterError> {
    prepare_trade_command(config, args, "trade.submit_proposal.v1")
}

pub fn propose_revision(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradePreparedView, CliSdkAdapterError> {
    prepare_trade_command(config, args, "trade.propose_revision.v1")
}

pub fn decide_candidate(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradePreparedView, CliSdkAdapterError> {
    prepare_trade_command(config, args, "trade.decide_candidate.v1")
}

pub fn cancel_trade(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradePreparedView, CliSdkAdapterError> {
    prepare_trade_command(config, args, "trade.cancel.v1")
}

pub fn resume_operation(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
) -> Result<TradePreparedView, CliSdkAdapterError> {
    let operation_kind = args.operation_kind.as_deref().ok_or_else(|| {
        RuntimeError::Config("trade operation resume requires `--operation-kind`".to_owned())
    })?;
    prepare_trade_command(config, args, resume_operation_kind(operation_kind)?)
}

fn prepare_trade_command(
    config: &RuntimeConfig,
    args: &TradeEnvelopeFileRuntimeArgs,
    operation: &str,
) -> Result<TradePreparedView, CliSdkAdapterError> {
    let envelope = load_trade_envelope(args.file.as_path())?;
    let actor = actor_for_envelope(config, &envelope, operation)?;
    let idempotency_key = args
        .idempotency_key
        .as_deref()
        .and_then(non_empty_ref)
        .map(radroots_storage::journal::IdempotencyKey::parse)
        .transpose()
        .map_err(|error| RuntimeError::Config(format!("invalid idempotency key: {error}")))?;
    let plan = sdk_trade::prepare(sdk_trade::PrepareRequest::new(actor, envelope))
        .map_err(|error| RuntimeError::Config(format!("invalid SDK trade plan: {error}")))?;
    if matches!(operation, "trade.decide_candidate.v1")
        && plan.workflow().private_terms().is_some()
        && !args.acknowledge_private_terms
    {
        return Err(RuntimeError::Config(
            "trade decision requires explicit private-terms acknowledgement".to_owned(),
        )
        .into());
    }
    Ok(trade_prepared_view(operation, args, &plan, idempotency_key))
}

fn trade_prepared_view(
    operation: &str,
    args: &TradeEnvelopeFileRuntimeArgs,
    plan: &TradePlan,
    idempotency_key: Option<radroots_storage::journal::IdempotencyKey>,
) -> TradePreparedView {
    TradePreparedView {
        state: "prepared".to_owned(),
        operation: operation.to_owned(),
        trade_id: plan.workflow().trade_id().to_string(),
        mutation_id: plan.workflow().mutation_id().to_string(),
        mutation_kind: format!("{:?}", plan.workflow().kind()),
        event_id: plan.draft().expected_event_id().to_string(),
        event_kind: plan.draft().kind_u32(),
        author: plan.draft().expected_pubkey().to_hex(),
        required_actions: plan
            .workflow()
            .required_actions()
            .iter()
            .map(|action| format!("{action:?}"))
            .collect(),
        idempotency_key: idempotency_key.map(|value| value.as_str().to_owned()),
        reason: Some(
            "trade plan validated; durable enqueue requires the configured shared sync engine"
                .to_owned(),
        ),
        actions: vec![format!(
            "radroots trade operation resume {}",
            args.file.display()
        )],
    }
}

pub fn get_trade(
    _config: &RuntimeConfig,
    args: &TradeIdRuntimeArgs,
) -> Result<serde_json::Value, CliSdkAdapterError> {
    let _ = trade_id(args.trade_id.as_str(), "trade_id")?;
    Err(RuntimeError::Config(
        "trade queries require the configured SDK storage and projection adapter".to_owned(),
    )
    .into())
}

pub fn list_trades(
    _config: &RuntimeConfig,
    args: &TradePageRuntimeArgs,
) -> Result<serde_json::Value, CliSdkAdapterError> {
    if matches!(args.limit, Some(0)) {
        return Err(RuntimeError::Config("trade list limit must be positive".to_owned()).into());
    }
    let _ = args.cursor.as_deref().and_then(non_empty_ref);
    Err(RuntimeError::Config(
        "trade queries require the configured SDK storage and projection adapter".to_owned(),
    )
    .into())
}

pub fn refresh_evidence(
    _config: &RuntimeConfig,
    args: &TradeIdRuntimeArgs,
) -> Result<serde_json::Value, CliSdkAdapterError> {
    let _ = trade_id(args.trade_id.as_str(), "trade_id")?;
    Err(RuntimeError::Config(
        "trade projection refresh requires the configured shared sync engine".to_owned(),
    )
    .into())
}

pub fn inspect_evidence(
    _config: &RuntimeConfig,
    args: &TradeEvidenceInspectRuntimeArgs,
) -> Result<serde_json::Value, CliSdkAdapterError> {
    let _ = trade_id(args.trade_id.as_str(), "trade_id")?;
    if matches!(args.limit, Some(0)) {
        return Err(
            RuntimeError::Config("trade evidence limit must be positive".to_owned()).into(),
        );
    }
    let _ = args.cursor.as_deref().and_then(non_empty_ref);
    Err(RuntimeError::Config(
        "trade evidence queries require the configured SDK storage adapter".to_owned(),
    )
    .into())
}

pub fn seal_private_artifact(
    _config: &RuntimeConfig,
    args: &TradePrivateArtifactSealRuntimeArgs,
) -> Result<serde_json::Value, CliSdkAdapterError> {
    let _ = trade_id(args.trade_id.as_str(), "trade_id")?;
    let _ = private_artifact_kind(args.kind.as_str())?;
    let _ = fs::metadata(args.input.as_path()).map_err(RuntimeError::from)?;
    Err(RuntimeError::Config(
        "private trade artifact sealing requires a host-owned secret adapter".to_owned(),
    )
    .into())
}

pub fn open_private_artifact(
    _config: &RuntimeConfig,
    args: &TradePrivateArtifactOpenRuntimeArgs,
) -> Result<TradePrivateArtifactOpenView, CliSdkAdapterError> {
    Err(RuntimeError::Config(format!(
        "private trade artifact `{}` requires a host-owned secret adapter",
        args.artifact_id
    ))
    .into())
}

pub fn delete_private_artifact(
    _config: &RuntimeConfig,
    args: &TradePrivateArtifactDeleteRuntimeArgs,
) -> Result<serde_json::Value, CliSdkAdapterError> {
    Err(RuntimeError::Config(format!(
        "private trade artifact `{}` requires a host-owned secret adapter",
        args.artifact_id
    ))
    .into())
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
    let buyer_public_key_hex = buyer.record.public_identity().public_key().to_hex();
    let buyer_pubkey = pubkey(buyer_public_key_hex.as_str(), "buyer_pubkey")?;
    let seller_pubkey = pubkey(parsed_listing.seller_pubkey.as_str(), "seller_pubkey")?;
    let candidate = candidate_terms(
        &product,
        &listing_state,
        buyer_pubkey.clone(),
        seller_pubkey.clone(),
        farm_id.clone(),
        args.quantity,
    )?;
    let envelope = TradeMutationEnvelopeV1 {
        mutation_id: None,
        contract_id: RADROOTS_TRADE_PROPOSAL_CONTRACT_ID.to_owned(),
        schema_version: RADROOTS_TRADE_SCHEMA_VERSION,
        trade_id: next_trade_id()?,
        root_mutation_id: None,
        buyer_pubkey,
        seller_pubkey: seller_pubkey.clone(),
        farm_id: farm_id.clone(),
        parent_mutation_ids: Vec::new(),
        author_pubkey: pubkey(buyer_public_key_hex.as_str(), "author_pubkey")?,
        counterparty_pubkey: seller_pubkey,
        authored_at_unix_s: now_unix(),
        body: TradeMutationBodyV1::Proposal { candidate },
    };
    let canonical = canonical_trade_mutation_content(envelope)
        .map_err(|error| RuntimeError::Config(format!("build trade proposal envelope: {error}")))?;
    let file = candidate_draft_file(config, canonical.envelope.trade_id.to_string().as_str());
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
        buyer_pubkey: buyer_public_key_hex,
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

fn load_trade_envelope(path: &Path) -> Result<TradeMutationEnvelopeV1, RuntimeError> {
    let contents = fs::read_to_string(path)?;
    serde_json::from_str(contents.as_str()).map_err(|error| {
        RuntimeError::Config(format!(
            "invalid trade envelope {}: {error}",
            path.display()
        ))
    })
}

fn actor_for_envelope(
    config: &RuntimeConfig,
    envelope: &TradeMutationEnvelopeV1,
    operation: &str,
) -> Result<Actor, CliSdkAdapterError> {
    let account = account::resolve_account(config)?.ok_or_else(|| {
        RuntimeError::Config(format!("{operation} requires a selected signer account"))
    })?;
    let author_pubkey = envelope.author_pubkey.to_hex();
    let account_pubkey = account.record.public_identity().public_key().to_hex();
    if !account_pubkey.eq_ignore_ascii_case(author_pubkey.as_str()) {
        return Err(RuntimeError::Config(format!(
            "{operation} envelope author `{author_pubkey}` does not match selected account `{}` public key `{account_pubkey}`",
            account.record.id()
        ))
        .into());
    }
    let role = if envelope
        .buyer_pubkey
        .to_hex()
        .eq_ignore_ascii_case(author_pubkey.as_str())
    {
        AuthorRole::Buyer
    } else if envelope
        .seller_pubkey
        .to_hex()
        .eq_ignore_ascii_case(author_pubkey.as_str())
    {
        AuthorRole::Seller
    } else {
        return Err(RuntimeError::Config(format!(
            "{operation} envelope author must be the buyer or seller"
        ))
        .into());
    };
    let actor = Actor::from_public_key_hex(
        author_pubkey.as_str(),
        ActorSource::ExplicitPublicKey,
        [role],
    )
    .map_err(|error| RuntimeError::Config(format!("invalid trade SDK actor: {error}")))?;
    validate_configured_signer_for_actor(
        config,
        Some(account.record.id().to_hex().as_str()),
        author_pubkey.as_str(),
        operation,
    )?;
    Ok(actor)
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

fn private_artifact_kind(value: &str) -> Result<&'static str, RuntimeError> {
    match value {
        "binding_terms" => Ok("binding_terms"),
        "message" => Ok("message"),
        "contact_bundle" => Ok("contact_bundle"),
        "delivery_instruction" => Ok("delivery_instruction"),
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
    buyer_pubkey: PublicKey,
    seller_pubkey: PublicKey,
    farm_id: DTag,
    bin_count: u32,
) -> Result<TradeCandidateTermsV1, RuntimeError> {
    let currency = product
        .price_currency
        .parse::<Currency>()
        .map_err(|error| {
            RuntimeError::Config(format!("listing price_currency is invalid: {error}"))
        })?;
    let quantity_amount = exact_positive_decimal(product.qty_amt_exact.as_str(), "qty_amt_exact")?
        .checked_mul(Decimal::from(bin_count))
        .map_err(|error| RuntimeError::Config(format!("trade quantity overflow: {error}")))?;
    let quantity_unit = product
        .qty_unit
        .parse::<Unit>()
        .map_err(|error| RuntimeError::Config(format!("listing qty_unit is invalid: {error}")))?;
    let price_amount = exact_positive_decimal(product.price_amt_exact.as_str(), "price_amt_exact")?;
    let price_quantity_amount =
        exact_positive_decimal(product.price_qty_amt_exact.as_str(), "price_qty_amt_exact")?;
    let price_unit = product.price_qty_unit.parse::<Unit>().map_err(|error| {
        RuntimeError::Config(format!("listing price_qty_unit is invalid: {error}"))
    })?;
    let quantity_unit_in_price_units =
        convert_unit_decimal(Decimal::ONE, quantity_unit, price_unit).map_err(|error| {
            RuntimeError::Config(format!(
                "listing quantity and price units are incompatible: {error}"
            ))
        })?;
    let unit_price_amount = price_amount
        .checked_div(price_quantity_amount)
        .and_then(|value| value.checked_mul(quantity_unit_in_price_units))
        .map_err(|error| RuntimeError::Config(format!("trade unit price is invalid: {error}")))?;
    let subtotal = unit_price_amount
        .checked_mul(quantity_amount)
        .map_err(|error| RuntimeError::Config(format!("trade subtotal overflow: {error}")))?;
    let quantity_scale = u8::try_from(quantity_amount.scale())
        .map_err(|_| RuntimeError::Config("trade quantity scale exceeds u8".to_owned()))?;
    let currency_exponent = currency.minor_unit_exponent();
    let subtotal_mantissa = decimal_mantissa_at_scale(subtotal, currency_exponent);
    let line = TradeCandidateLineV1 {
        line_id: DTag::parse("line-1")
            .map_err(|error| RuntimeError::Config(format!("invalid line id: {error}")))?,
        listing_addr: ClassifiedListingAddress::parse(product.listing_addr.as_str())
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
    Ok(TradeCandidateTermsV1 {
        candidate_id: None,
        schema_version: RADROOTS_TRADE_SCHEMA_VERSION,
        base_candidate_id: None,
        supersession_intent: None,
        buyer_pubkey,
        seller_pubkey,
        farm_id,
        lines: vec![line],
        line_tombstones: Vec::new(),
        economics: TradeEconomicsProfileV1 {
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
        fulfillment: FulfillmentProfileV1 {
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
        cancellation: TradeCancellationProfileV1 {
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
    MutationId::parse(state.content_hash.as_str()).map_err(|error| {
        RuntimeError::Config(format!(
            "listing content hash is not a 32-byte hex digest: {error}"
        ))
    })?;
    state
        .last_event_id
        .parse::<radroots_event::id::RadrootsEventId>()
        .map_err(|error| {
            RuntimeError::Config(format!("listing latest event id is invalid: {error}"))
        })?;
    Ok(ActiveListingState {
        last_event_id: state.last_event_id,
        content_hash: state.content_hash,
    })
}

fn resolve_farm_id(config: &RuntimeConfig, seller_pubkey: &str) -> Result<DTag, RuntimeError> {
    let db = ReplicaSql::new(SqlxSqliteExecutor::open(&config.local.replica_store_path)?);
    let d_tag = db.farm_unique_d_tag_by_pubkey(seller_pubkey)?.ok_or_else(|| {
        RuntimeError::Config(format!(
            "seller `{seller_pubkey}` must have exactly one farm profile in the local replica before creating a trade proposal draft"
        ))
    })?;
    DTag::parse(d_tag.as_str())
        .map_err(|error| RuntimeError::Config(format!("farm d tag is invalid: {error}")))
}

fn parse_listing_addr(raw: &str) -> Result<ParsedListingAddress, RuntimeError> {
    let parsed = ClassifiedListingAddress::parse(raw)
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

fn proposal_economics_view(envelope: &TradeMutationEnvelopeV1) -> TradeCandidateDraftEconomicsView {
    let TradeMutationBodyV1::Proposal { candidate } = &envelope.body else {
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

fn proposal_candidate_id(envelope: &TradeMutationEnvelopeV1) -> Option<String> {
    let TradeMutationBodyV1::Proposal { candidate } = &envelope.body else {
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

fn exact_positive_decimal(value: &str, field: &str) -> Result<Decimal, RuntimeError> {
    let parsed = value
        .trim()
        .parse::<Decimal>()
        .map_err(|error| RuntimeError::Config(format!("listing {field} is invalid: {error}")))?;
    if parsed.is_zero() || parsed.is_sign_negative() {
        return Err(RuntimeError::Config(format!(
            "listing {field} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn decimal_mantissa_at_scale(mut value: Decimal, scale: u32) -> String {
    value.rescale(scale);
    value.to_string().replace('.', "")
}

fn candidate_draft_file(config: &RuntimeConfig, trade_id: &str) -> PathBuf {
    config
        .paths
        .app_data_root
        .join(TRADE_CANDIDATE_DRAFTS_DIR)
        .join(format!("{trade_id}.json"))
}

fn next_trade_id() -> Result<TradeId, RuntimeError> {
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

fn trade_id(value: &str, field: &str) -> Result<TradeId, RuntimeError> {
    TradeId::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn trade_candidate_id(value: &str, field: &str) -> Result<CandidateId, RuntimeError> {
    CandidateId::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn inventory_bin_id(value: &str, field: &str) -> Result<InventoryBinId, RuntimeError> {
    InventoryBinId::parse(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn pubkey(value: &str, field: &str) -> Result<PublicKey, RuntimeError> {
    PublicKey::from_hex(value)
        .map_err(|error| RuntimeError::Config(format!("{field} is invalid: {error}")))
}

fn non_empty_ref(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}
