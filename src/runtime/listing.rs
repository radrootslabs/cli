use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_core::{
    RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreDiscount, RadrootsCoreDiscountScope,
    RadrootsCoreDiscountThreshold, RadrootsCoreDiscountValue, RadrootsCoreMoney,
    RadrootsCorePercent, RadrootsCoreQuantity, RadrootsCoreQuantityPrice, RadrootsCoreUnit,
};
use radroots_events::RadrootsNostrEvent;
use radroots_events::farm::RadrootsFarmRef;
use radroots_events::kinds::{KIND_LISTING, KIND_LISTING_DRAFT};
use radroots_events::listing::{
    RadrootsListing, RadrootsListingAvailability, RadrootsListingBin,
    RadrootsListingDeliveryMethod, RadrootsListingLocation, RadrootsListingProduct,
    RadrootsListingStatus,
};
use radroots_events::trade::RadrootsTradeListingValidationError;
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::listing::encode::to_wire_parts_with_kind;
use radroots_events_codec::wire::WireEventParts;
use radroots_nostr::prelude::{RadrootsNostrEvent as SignedNostrEvent, radroots_event_from_nostr};
use radroots_replica_db::{ReplicaSql, migrations};
use radroots_replica_sync::{RadrootsReplicaIngestOutcome, radroots_replica_ingest_event};
use radroots_sql_core::SqliteExecutor;
use radroots_trade::listing::publish::validate_listing_for_seller;
use radroots_trade::listing::validation::validate_listing_event;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::domain::runtime::{
    FindPriceView, FindQuantityView, FindResultProvenanceView, ListingGetView, ListingListView,
    ListingMutationEventView, ListingMutationLocalReplicaView, ListingMutationView, ListingNewView,
    ListingRebindView, ListingSummaryView, ListingValidateView, ListingValidationIssueView,
    MarketReadinessView, RelayFailureView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::{
    PublishMode, RADROOTSD_PUBLISH_DEFERRED_REASON, RuntimeConfig, SignerBackend,
};
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayPublishError, DirectRelayPublishReceipt,
    publish_signed_event_with_identity, sign_parts_with_identity,
};
use crate::runtime::farm_config;
use crate::runtime::local_events::{
    append_local_work, append_signed_event, mark_signed_event_acknowledged,
    mark_signed_event_failed_for_publish_error,
};
use crate::runtime::signer::{ActorWriteBindingError, resolve_actor_write_authority};
use crate::runtime::sync::{
    RelayIngestScope, freshness_for_scope_from_executor, market_refresh, missing_freshness,
};
use crate::runtime_args::{
    ListingCreateArgs, ListingFileArgs, ListingMutationArgs, ListingRebindArgs, RecordLookupArgs,
};

const DRAFT_KIND: &str = "listing_draft_v1";
const LISTING_SOURCE: &str = "local draft · local first";
const LISTING_READ_SOURCE: &str = "local replica · local first";
const RELAY_LISTING_WRITE_SOURCE: &str = "direct Nostr relay publish · local key";
const RADROOTSD_LISTING_WRITE_SOURCE: &str = "radrootsd publish transport · deferred";
const LISTING_DRAFTS_DIR: &str = "listings/drafts";
const LISTING_SELLER_ACTOR_SOURCE_FARM_CONFIG: &str = "farm_config";
const LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT: &str = "resolved_account";
const LISTING_SELLER_ACTOR_SOURCE_REBIND: &str = "listing_rebind";

static D_TAG_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftDocument {
    version: u32,
    kind: String,
    listing: ListingDraftMeta,
    seller_actor: ListingDraftSellerActor,
    product: ListingDraftProduct,
    primary_bin: ListingDraftPrimaryBin,
    inventory: ListingDraftInventory,
    availability: ListingDraftAvailability,
    delivery: ListingDraftDelivery,
    location: ListingDraftLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    discounts: Vec<ListingDraftDiscount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftMeta {
    d_tag: String,
    farm_d_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftSellerActor {
    account_id: String,
    pubkey: String,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftProduct {
    key: String,
    title: String,
    category: String,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftPrimaryBin {
    bin_id: String,
    quantity_amount: String,
    quantity_unit: String,
    price_amount: String,
    price_currency: String,
    price_per_amount: String,
    price_per_unit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftInventory {
    available: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftAvailability {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftDelivery {
    method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftLocation {
    primary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    country: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftDiscount {
    id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    label: String,
    kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    value: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    amount: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min_bin_count: Option<u32>,
}

#[derive(Debug, Clone)]
struct ListingValidationContext {
    farm_setup_action: String,
}

#[derive(Debug, Clone)]
struct ListingAuthoringDefaults {
    farm_config_present: bool,
    farm_defaults_ready: bool,
    farm_next_action: Option<String>,
    farm_reason: Option<String>,
    farm_name: Option<String>,
    seller_account_id: String,
    seller_pubkey: String,
    seller_actor_source: String,
    selected_farm_d_tag: Option<String>,
    delivery_method: Option<String>,
    location: Option<ListingDraftLocation>,
}

#[derive(Debug, Clone)]
struct CanonicalListingDraft {
    listing_id: String,
    seller_account_id: String,
    seller_pubkey: String,
    seller_actor_source: String,
    farm_d_tag: String,
    listing: RadrootsListing,
}

#[derive(Debug, Clone)]
struct ListingMutationEventDraft {
    event: ListingMutationEventView,
    parts: WireEventParts,
}

#[derive(Debug, Clone)]
struct LoadedListingDraft {
    file: PathBuf,
    updated_at_unix: u64,
    contents: String,
    document: ListingDraftDocument,
}

#[derive(Debug, Clone)]
enum ListingDraftValidationError {
    Issue(ListingValidationIssueView),
    MissingSellerAccount(ListingValidationIssueView),
}

impl ListingDraftValidationError {
    fn into_issue(self) -> ListingValidationIssueView {
        match self {
            Self::Issue(issue) | Self::MissingSellerAccount(issue) => issue,
        }
    }
}

impl From<ListingValidationIssueView> for ListingDraftValidationError {
    fn from(issue: ListingValidationIssueView) -> Self {
        Self::Issue(issue)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ListingMutationOperation {
    Publish,
    Update,
    Archive,
}

impl ListingMutationOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Update => "update",
            Self::Archive => "archive",
        }
    }
}

pub fn scaffold(
    config: &RuntimeConfig,
    args: &ListingCreateArgs,
) -> Result<ListingNewView, RuntimeError> {
    let (draft, defaults) = build_listing_draft(config, args)?;
    let output_path = listing_output_path(config, args.output.as_ref(), &draft.listing.d_tag)?;
    write_listing_draft(&output_path, &draft, false)?;
    append_listing_local_work(config, output_path.as_path(), &draft)?;

    let mut actions = vec![format!(
        "radroots listing validate {}",
        output_path.display()
    )];
    if let Some(action) = &defaults.farm_next_action {
        actions.push(action.clone());
    }

    Ok(ListingNewView {
        state: "draft created".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: output_path.display().to_string(),
        listing_id: draft.listing.d_tag,
        key: non_empty(draft.product.key.clone()),
        seller_account_id: Some(defaults.seller_account_id),
        seller_pubkey: Some(defaults.seller_pubkey),
        seller_actor_source: Some(defaults.seller_actor_source),
        farm_d_tag: defaults.selected_farm_d_tag,
        delivery_method: non_empty(draft.delivery.method.clone()),
        location_primary: non_empty(draft.location.primary.clone()),
        reason: defaults.farm_reason,
        actions,
    })
}

pub fn scaffold_preflight(
    config: &RuntimeConfig,
    args: &ListingCreateArgs,
) -> Result<ListingNewView, RuntimeError> {
    let (draft, defaults) = build_listing_draft(config, args)?;
    let output_path = listing_output_path(config, args.output.as_ref(), &draft.listing.d_tag)?;
    validate_listing_output_target(&output_path)?;

    let mut actions = vec![format!(
        "radroots listing validate {}",
        output_path.display()
    )];
    if let Some(action) = &defaults.farm_next_action {
        actions.push(action.clone());
    }

    Ok(ListingNewView {
        state: "dry_run".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: output_path.display().to_string(),
        listing_id: draft.listing.d_tag,
        key: non_empty(draft.product.key.clone()),
        seller_account_id: Some(defaults.seller_account_id),
        seller_pubkey: Some(defaults.seller_pubkey),
        seller_actor_source: Some(defaults.seller_actor_source),
        farm_d_tag: defaults.selected_farm_d_tag,
        delivery_method: non_empty(draft.delivery.method.clone()),
        location_primary: non_empty(draft.location.primary.clone()),
        reason: Some("dry run requested; listing draft was not written".to_owned()),
        actions,
    })
}

fn build_listing_draft(
    config: &RuntimeConfig,
    args: &ListingCreateArgs,
) -> Result<(ListingDraftDocument, ListingAuthoringDefaults), RuntimeError> {
    let defaults = authoring_defaults(config)?;
    let quantity_unit = args.quantity_unit.clone().unwrap_or_else(|| "g".to_owned());
    let draft = ListingDraftDocument {
        version: 1,
        kind: DRAFT_KIND.to_owned(),
        listing: ListingDraftMeta {
            d_tag: generate_d_tag(),
            farm_d_tag: defaults.selected_farm_d_tag.clone().unwrap_or_default(),
        },
        seller_actor: ListingDraftSellerActor {
            account_id: defaults.seller_account_id.clone(),
            pubkey: defaults.seller_pubkey.clone(),
            source: defaults.seller_actor_source.clone(),
        },
        product: ListingDraftProduct {
            key: args.key.clone().unwrap_or_default(),
            title: args.title.clone().unwrap_or_default(),
            category: args.category.clone().unwrap_or_default(),
            summary: args.summary.clone().unwrap_or_default(),
        },
        primary_bin: ListingDraftPrimaryBin {
            bin_id: args.bin_id.clone().unwrap_or_else(|| "bin-1".to_owned()),
            quantity_amount: args
                .quantity_amount
                .clone()
                .unwrap_or_else(|| "1000".to_owned()),
            quantity_unit: quantity_unit.clone(),
            price_amount: args
                .price_amount
                .clone()
                .unwrap_or_else(|| "0.01".to_owned()),
            price_currency: args
                .price_currency
                .clone()
                .unwrap_or_else(|| "USD".to_owned()),
            price_per_amount: args
                .price_per_amount
                .clone()
                .unwrap_or_else(|| "1".to_owned()),
            price_per_unit: args
                .price_per_unit
                .clone()
                .unwrap_or_else(|| quantity_unit.clone()),
            label: args.label.clone().unwrap_or_default(),
        },
        inventory: ListingDraftInventory {
            available: args.available.clone().unwrap_or_else(|| "1".to_owned()),
        },
        availability: ListingDraftAvailability {
            kind: "status".to_owned(),
            status: "active".to_owned(),
            start: None,
            end: None,
        },
        delivery: ListingDraftDelivery {
            method: defaults.delivery_method.clone().unwrap_or_default(),
        },
        location: defaults.location.clone().unwrap_or(ListingDraftLocation {
            primary: String::new(),
            city: None,
            region: None,
            country: None,
        }),
        discounts: listing_discount_drafts_from_args(args),
    };
    Ok((draft, defaults))
}

fn listing_discount_drafts_from_args(args: &ListingCreateArgs) -> Vec<ListingDraftDiscount> {
    let has_discount = args.discount_id.is_some()
        || args.discount_label.is_some()
        || args.discount_kind.is_some()
        || args.discount_value.is_some()
        || args.discount_amount.is_some()
        || args.discount_currency.is_some();
    if !has_discount {
        return Vec::new();
    }
    let kind = args.discount_kind.clone().unwrap_or_else(|| {
        if args.discount_amount.is_some() {
            "amount".to_owned()
        } else {
            "percent".to_owned()
        }
    });
    vec![ListingDraftDiscount {
        id: args
            .discount_id
            .clone()
            .unwrap_or_else(|| "discount_1".to_owned()),
        label: args.discount_label.clone().unwrap_or_default(),
        kind,
        value: args.discount_value.clone().unwrap_or_default(),
        amount: args.discount_amount.clone().unwrap_or_default(),
        currency: args.discount_currency.clone().unwrap_or_default(),
        bin_id: None,
        min_bin_count: None,
    }]
}

fn listing_output_path(
    config: &RuntimeConfig,
    explicit: Option<&std::path::PathBuf>,
    listing_id: &str,
) -> Result<std::path::PathBuf, RuntimeError> {
    match explicit {
        Some(path) => Ok(path.clone()),
        None => Ok(drafts_dir(config).join(format!("{listing_id}.toml"))),
    }
}

fn write_listing_draft(
    output_path: &Path,
    draft: &ListingDraftDocument,
    overwrite: bool,
) -> Result<(), RuntimeError> {
    if !overwrite {
        validate_listing_output_target(output_path)?;
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, scaffold_contents(draft)?)?;
    Ok(())
}

fn append_listing_local_work(
    config: &RuntimeConfig,
    path: &Path,
    draft: &ListingDraftDocument,
) -> Result<(), RuntimeError> {
    let listing_id = draft.listing.d_tag.trim();
    let seller_pubkey = draft.seller_actor.pubkey.trim();
    let listing_addr = if seller_pubkey.is_empty() || listing_id.is_empty() {
        None
    } else {
        Some(listing_addr(seller_pubkey, listing_id))
    };
    let payload = json!({
        "record_kind": DRAFT_KIND,
        "path": path.display().to_string(),
        "document": draft,
    });
    let subject = format!("listing:{}", draft.listing.d_tag);
    append_local_work(
        config,
        subject.as_str(),
        non_empty(draft.seller_actor.account_id.clone()),
        non_empty(draft.seller_actor.pubkey.clone()),
        non_empty(draft.listing.farm_d_tag.clone()),
        listing_addr,
        payload,
    )?;
    Ok(())
}

fn validate_listing_output_target(output_path: &Path) -> Result<(), RuntimeError> {
    if output_path.exists() {
        return Err(RuntimeError::Config(format!(
            "listing draft output {} must not already exist",
            output_path.display()
        )));
    }
    if let Some(parent) = output_path.parent() {
        if parent.exists() && !parent.is_dir() {
            return Err(RuntimeError::Config(format!(
                "listing draft parent {} is not a directory",
                parent.display()
            )));
        }
    }
    Ok(())
}

pub fn validate(
    config: &RuntimeConfig,
    args: &ListingFileArgs,
) -> Result<ListingValidateView, RuntimeError> {
    let contents = fs::read_to_string(&args.file)?;
    let context = validation_context(config)?;

    let parsed = match toml::from_str::<ListingDraftDocument>(&contents) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Ok(ListingValidateView {
                state: "invalid".to_owned(),
                source: LISTING_SOURCE.to_owned(),
                file: args.file.display().to_string(),
                valid: false,
                listing_id: None,
                seller_account_id: None,
                seller_pubkey: None,
                seller_actor_source: None,
                farm_d_tag: None,
                issues: vec![ListingValidationIssueView {
                    field: "toml".to_owned(),
                    message: error.to_string(),
                    line: error
                        .span()
                        .map(|span| line_for_offset(&contents, span.start + 1)),
                }],
                actions: vec![format!("edit {}", args.file.display())],
            });
        }
    };

    match canonicalize_draft(&parsed, &contents, &context) {
        Ok(canonical) => {
            let parts = match to_wire_parts_with_kind(&canonical.listing, KIND_LISTING_DRAFT) {
                Ok(parts) => parts,
                Err(error) => {
                    return Ok(invalid_validation_view(
                        args.file.as_path(),
                        &parsed,
                        &context,
                        ListingValidationIssueView {
                            field: "listing".to_owned(),
                            message: format!("invalid listing contract: {error}"),
                            line: None,
                        },
                    ));
                }
            };
            if let Some(issue) = listing_bound_account_issue(config, &canonical, &contents)? {
                return Ok(invalid_validation_view(
                    args.file.as_path(),
                    &parsed,
                    &context,
                    issue,
                ));
            }
            let event = RadrootsNostrEvent {
                id: String::new(),
                author: canonical.seller_pubkey.clone(),
                created_at: 0,
                kind: KIND_LISTING_DRAFT,
                tags: parts.tags,
                content: parts.content,
                sig: String::new(),
            };
            match validate_listing_event(&event) {
                Ok(_) => Ok(ListingValidateView {
                    state: "valid".to_owned(),
                    source: LISTING_SOURCE.to_owned(),
                    file: args.file.display().to_string(),
                    valid: true,
                    listing_id: Some(canonical.listing_id),
                    seller_account_id: Some(canonical.seller_account_id),
                    seller_pubkey: Some(canonical.seller_pubkey),
                    seller_actor_source: Some(canonical.seller_actor_source),
                    farm_d_tag: Some(canonical.farm_d_tag),
                    issues: Vec::new(),
                    actions: vec![format!("radroots listing publish {}", args.file.display())],
                }),
                Err(error) => Ok(invalid_validation_view(
                    args.file.as_path(),
                    &parsed,
                    &context,
                    issue_from_trade_validation(error, &contents),
                )),
            }
        }
        Err(error) => Ok(invalid_validation_view(
            args.file.as_path(),
            &parsed,
            &context,
            error.into_issue(),
        )),
    }
}

pub fn list(config: &RuntimeConfig) -> Result<ListingListView, RuntimeError> {
    let dir = drafts_dir(config);
    if !dir.exists() {
        return Ok(ListingListView {
            state: "empty".to_owned(),
            source: LISTING_SOURCE.to_owned(),
            count: 0,
            draft_dir: dir.display().to_string(),
            listings: Vec::new(),
            actions: vec!["radroots listing create".to_owned()],
        });
    }

    let context = validation_context(config).map_err(|error| error.to_string());
    let mut listings = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        match load_listing_draft(path.as_path()) {
            Ok(loaded) => listings.push(summary_from_loaded(config, &loaded, context.as_ref())),
            Err(issue) => listings.push(summary_for_invalid_file(path.as_path(), issue)),
        }
    }

    listings.sort_by(|left, right| {
        right
            .updated_at_unix
            .cmp(&left.updated_at_unix)
            .then_with(|| left.id.cmp(&right.id))
    });

    let state = if listings.is_empty() {
        "empty"
    } else if listings.iter().any(|listing| listing.state == "error") {
        "degraded"
    } else {
        "ready"
    };
    let actions = if listings.is_empty() {
        vec!["radroots listing create".to_owned()]
    } else {
        Vec::new()
    };

    Ok(ListingListView {
        state: state.to_owned(),
        source: LISTING_SOURCE.to_owned(),
        count: listings.len(),
        draft_dir: dir.display().to_string(),
        listings,
        actions,
    })
}

pub fn rebind(
    config: &RuntimeConfig,
    args: &ListingRebindArgs,
) -> Result<ListingRebindView, RuntimeError> {
    rebind_inner(config, args, false)
}

pub fn rebind_preflight(
    config: &RuntimeConfig,
    args: &ListingRebindArgs,
) -> Result<ListingRebindView, RuntimeError> {
    rebind_inner(config, args, true)
}

fn rebind_inner(
    config: &RuntimeConfig,
    args: &ListingRebindArgs,
    dry_run: bool,
) -> Result<ListingRebindView, RuntimeError> {
    let contents = fs::read_to_string(&args.file)?;
    let mut draft = toml::from_str::<ListingDraftDocument>(&contents).map_err(|error| {
        RuntimeError::Config(format!(
            "invalid listing draft {}: {error}",
            args.file.display()
        ))
    })?;
    let listing_id = draft.listing.d_tag.trim().to_owned();
    if !is_d_tag_base64url(&listing_id) {
        return Err(RuntimeError::Config(format!(
            "invalid listing draft {}: listing d_tag must be a 22-character base64url identifier",
            args.file.display()
        )));
    }

    let target_account = accounts::resolve_account_selector(config, args.selector.as_str())
        .map_err(|error| listing_rebind_selector_error(args.selector.as_str(), error))?;
    let from_seller_account_id = non_empty(draft.seller_actor.account_id.clone());
    let from_seller_pubkey = non_empty(draft.seller_actor.pubkey.clone());
    let from_seller_actor_source = non_empty(draft.seller_actor.source.clone());
    let from_farm_d_tag = non_empty(draft.listing.farm_d_tag.clone());
    let target_account_id = target_account.record.account_id.to_string();
    let target_pubkey = target_account.record.public_identity.public_key_hex.clone();
    let target_farm_d_tag = resolve_rebind_farm_d_tag(
        config,
        args,
        from_seller_account_id.as_deref(),
        from_farm_d_tag.as_deref(),
        target_account_id.as_str(),
    )?;
    let from_listing_addr = from_seller_pubkey
        .as_ref()
        .map(|pubkey| listing_addr(pubkey, listing_id.as_str()));
    let to_listing_addr = listing_addr(target_pubkey.as_str(), listing_id.as_str());
    let seller_pubkey_changed = from_seller_pubkey
        .as_deref()
        .map(|pubkey| !pubkey.eq_ignore_ascii_case(target_pubkey.as_str()));
    let listing_addr_changed = from_listing_addr
        .as_deref()
        .map(|addr| addr != to_listing_addr.as_str());
    let farm_d_tag_changed = from_farm_d_tag
        .as_deref()
        .map(|d_tag| d_tag != target_farm_d_tag.as_str());

    draft.seller_actor.account_id = target_account_id.clone();
    draft.seller_actor.pubkey = target_pubkey.clone();
    draft.seller_actor.source = LISTING_SELLER_ACTOR_SOURCE_REBIND.to_owned();
    draft.listing.farm_d_tag = target_farm_d_tag.clone();

    if !dry_run {
        write_listing_draft(args.file.as_path(), &draft, true)?;
        append_listing_local_work(config, args.file.as_path(), &draft)?;
    }

    Ok(ListingRebindView {
        state: if dry_run { "dry_run" } else { "rebound" }.to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: args.file.display().to_string(),
        listing_id,
        dry_run,
        from_seller_account_id,
        from_seller_pubkey,
        from_seller_actor_source,
        to_seller_account_id: target_account_id,
        to_seller_pubkey: target_pubkey,
        to_seller_actor_source: LISTING_SELLER_ACTOR_SOURCE_REBIND.to_owned(),
        seller_pubkey_changed,
        from_listing_addr,
        to_listing_addr,
        listing_addr_changed,
        from_farm_d_tag,
        to_farm_d_tag: target_farm_d_tag,
        farm_d_tag_changed,
        reason: Some(if dry_run {
            "dry run requested; listing seller actor binding was not written".to_owned()
        } else {
            "listing seller actor binding updated".to_owned()
        }),
        actions: if dry_run {
            vec![format!(
                "radroots --approval-token approve listing rebind {} {}",
                args.file.display(),
                args.selector
            )]
        } else {
            vec![format!("radroots listing validate {}", args.file.display())]
        },
    })
}

fn resolve_rebind_farm_d_tag(
    config: &RuntimeConfig,
    args: &ListingRebindArgs,
    from_seller_account_id: Option<&str>,
    from_farm_d_tag: Option<&str>,
    target_account_id: &str,
) -> Result<String, RuntimeError> {
    if let Some(explicit) = args
        .farm_d_tag
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !is_d_tag_base64url(explicit) {
            return Err(RuntimeError::Config(
                "listing rebind --farm-d-tag must be a 22-character base64url identifier"
                    .to_owned(),
            ));
        }
        return Ok(explicit.to_owned());
    }
    if from_seller_account_id == Some(target_account_id)
        && let Some(existing) = from_farm_d_tag
    {
        return Ok(existing.to_owned());
    }
    if let Some(resolved) = farm_config::load(config, None)?
        && resolved.document.selection.account == target_account_id
    {
        return Ok(resolved.document.selection.farm_d_tag);
    }
    Err(RuntimeError::Config(format!(
        "listing rebind requires --farm-d-tag when target account `{target_account_id}` is not bound by the selected farm config"
    )))
}

fn listing_rebind_selector_error(selector: &str, error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::Account(accounts::AccountRuntimeFailure::Unresolved(issue)) => {
            accounts::AccountRuntimeFailure::unresolved_with_detail(
                issue.message().to_owned(),
                json!({
                    "seller_actor_source": LISTING_SELLER_ACTOR_SOURCE_REBIND,
                    "selector": selector,
                    "actions": [
                        "radroots account import <path>",
                        "radroots account create",
                    ],
                }),
            )
            .into()
        }
        other => other,
    }
}

fn listing_addr(seller_pubkey: &str, listing_id: &str) -> String {
    format!("{KIND_LISTING}:{seller_pubkey}:{listing_id}")
}

fn load_listing_draft(path: &Path) -> Result<LoadedListingDraft, ListingValidationIssueView> {
    let contents = fs::read_to_string(path).map_err(|error| ListingValidationIssueView {
        field: "file".to_owned(),
        message: format!("read listing draft {}: {error}", path.display()),
        line: None,
    })?;
    let document = toml::from_str::<ListingDraftDocument>(contents.as_str()).map_err(|error| {
        ListingValidationIssueView {
            field: "toml".to_owned(),
            message: error.to_string(),
            line: error
                .span()
                .map(|span| line_for_offset(contents.as_str(), span.start + 1)),
        }
    })?;
    Ok(LoadedListingDraft {
        file: path.to_path_buf(),
        updated_at_unix: modified_unix(path).unwrap_or_default(),
        contents,
        document,
    })
}

fn summary_from_loaded(
    config: &RuntimeConfig,
    loaded: &LoadedListingDraft,
    context: Result<&ListingValidationContext, &String>,
) -> ListingSummaryView {
    let mut seller_account_id = non_empty(loaded.document.seller_actor.account_id.clone());
    let mut seller_pubkey = non_empty(loaded.document.seller_actor.pubkey.clone());
    let mut seller_actor_source = non_empty(loaded.document.seller_actor.source.clone());
    let mut farm_d_tag = non_empty(loaded.document.listing.farm_d_tag.clone());
    let mut issues = Vec::new();
    let mut state = "draft";

    match context {
        Ok(context) => {
            match canonicalize_draft(&loaded.document, loaded.contents.as_str(), context) {
                Ok(canonical) => {
                    seller_account_id = Some(canonical.seller_account_id.clone());
                    seller_pubkey = Some(canonical.seller_pubkey.clone());
                    seller_actor_source = Some(canonical.seller_actor_source.clone());
                    farm_d_tag = Some(canonical.farm_d_tag.clone());
                    issues = listing_ready_issues(&canonical, loaded.contents.as_str());
                    if let Ok(Some(issue)) =
                        listing_bound_account_issue(config, &canonical, loaded.contents.as_str())
                    {
                        issues.push(issue);
                    }
                    if issues.is_empty() {
                        state = "ready";
                    }
                }
                Err(error) => issues.push(error.into_issue()),
            }
        }
        Err(reason) => issues.push(ListingValidationIssueView {
            field: "context".to_owned(),
            message: reason.to_string(),
            line: None,
        }),
    }

    ListingSummaryView {
        id: non_empty(loaded.document.listing.d_tag.clone())
            .unwrap_or_else(|| file_stem(loaded.file.as_path())),
        state: state.to_owned(),
        file: loaded.file.display().to_string(),
        product_key: non_empty(loaded.document.product.key.clone()),
        title: non_empty(loaded.document.product.title.clone()),
        category: non_empty(loaded.document.product.category.clone()),
        seller_account_id,
        seller_pubkey,
        seller_actor_source,
        farm_d_tag,
        location_primary: non_empty(loaded.document.location.primary.clone()),
        updated_at_unix: loaded.updated_at_unix,
        issues,
    }
}

fn listing_ready_issues(
    canonical: &CanonicalListingDraft,
    contents: &str,
) -> Vec<ListingValidationIssueView> {
    let parts = match to_wire_parts_with_kind(&canonical.listing, KIND_LISTING_DRAFT) {
        Ok(parts) => parts,
        Err(error) => {
            return vec![ListingValidationIssueView {
                field: "listing".to_owned(),
                message: format!("invalid listing contract: {error}"),
                line: None,
            }];
        }
    };
    let event = RadrootsNostrEvent {
        id: String::new(),
        author: canonical.seller_pubkey.clone(),
        created_at: 0,
        kind: KIND_LISTING_DRAFT,
        tags: parts.tags,
        content: parts.content,
        sig: String::new(),
    };
    match validate_listing_event(&event) {
        Ok(_) => Vec::new(),
        Err(error) => vec![issue_from_trade_validation(error, contents)],
    }
}

fn summary_for_invalid_file(path: &Path, issue: ListingValidationIssueView) -> ListingSummaryView {
    ListingSummaryView {
        id: file_stem(path),
        state: "error".to_owned(),
        file: path.display().to_string(),
        product_key: None,
        title: None,
        category: None,
        seller_account_id: None,
        seller_pubkey: None,
        seller_actor_source: None,
        farm_d_tag: None,
        location_primary: None,
        updated_at_unix: modified_unix(path).unwrap_or_default(),
        issues: vec![issue],
    }
}

pub fn get(
    config: &RuntimeConfig,
    args: &RecordLookupArgs,
) -> Result<ListingGetView, RuntimeError> {
    refresh_market_listing_if_needed(config)?;
    let freshness = if config.local.replica_db_path.exists() {
        let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
        freshness_for_scope_from_executor(config, &executor, RelayIngestScope::MarketRefresh)?
    } else {
        missing_freshness()
    };
    let provenance = FindResultProvenanceView {
        origin: "local_replica.trade_product".to_owned(),
        freshness: freshness.display.clone(),
        relay_count: config.relay.urls.len(),
    };

    if !config.local.replica_db_path.exists() {
        return Ok(ListingGetView {
            state: "unconfigured".to_owned(),
            source: LISTING_READ_SOURCE.to_owned(),
            lookup: args.key.clone(),
            readiness: MarketReadinessView::unavailable("local_replica_not_initialized"),
            listing_id: None,
            product_key: None,
            listing_addr: None,
            title: None,
            category: None,
            description: None,
            location_primary: None,
            available: None,
            price: None,
            provenance,
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        });
    }

    let db = ReplicaSql::new(SqliteExecutor::open(&config.local.replica_db_path)?);
    let rows = db.trade_product_lookup(args.key.as_str())?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(ListingGetView {
            state: "missing".to_owned(),
            source: LISTING_READ_SOURCE.to_owned(),
            lookup: args.key.clone(),
            readiness: MarketReadinessView::unavailable("market_listing_missing"),
            listing_id: None,
            product_key: None,
            listing_addr: None,
            title: None,
            category: None,
            description: None,
            location_primary: None,
            available: None,
            price: None,
            provenance,
            reason: Some(format!(
                "listing `{}` is not available in the local replica",
                args.key
            )),
            actions: vec![
                "radroots sync pull".to_owned(),
                format!("radroots market product search {}", args.key),
            ],
        });
    };

    let listing_addr = row.listing_addr.and_then(non_empty);
    let available_amount = row.qty_avail;
    let price_amount = row.price_amt;
    let price_currency = row.price_currency;
    let price_per_amount = row.price_qty_amt;
    let readiness = MarketReadinessView::from_market_projection(
        listing_addr.as_deref(),
        Some(row.title.as_str()),
        Some(row.category.as_str()),
        available_amount,
        price_amount,
        price_currency.as_str(),
        price_per_amount,
    );

    Ok(ListingGetView {
        state: "ready".to_owned(),
        source: LISTING_READ_SOURCE.to_owned(),
        lookup: args.key.clone(),
        readiness,
        listing_id: Some(row.id),
        product_key: Some(row.key),
        listing_addr,
        title: Some(row.title),
        category: Some(row.category),
        description: non_empty(row.summary),
        location_primary: row.location_primary.and_then(non_empty),
        available: Some(FindQuantityView {
            total_amount: row.qty_amt,
            total_unit: row.qty_unit,
            label: row.qty_label.and_then(non_empty),
            available_amount,
        }),
        price: Some(FindPriceView {
            amount: price_amount,
            currency: price_currency,
            per_amount: price_per_amount,
            per_unit: row.price_qty_unit,
        }),
        provenance,
        reason: None,
        actions: Vec::new(),
    })
}

fn refresh_market_listing_if_needed(config: &RuntimeConfig) -> Result<(), RuntimeError> {
    if !config.local.replica_db_path.exists()
        || config.output.dry_run
        || config.relay.urls.is_empty()
    {
        return Ok(());
    }
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let freshness =
        freshness_for_scope_from_executor(config, &executor, RelayIngestScope::MarketRefresh)?;
    if crate::runtime::sync::freshness_requires_refresh(&freshness) {
        let _ = market_refresh(config)?;
    }
    Ok(())
}

pub fn publish(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, RuntimeError> {
    mutate(config, args, ListingMutationOperation::Publish)
}

pub fn update(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, RuntimeError> {
    mutate(config, args, ListingMutationOperation::Update)
}

pub fn archive(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, RuntimeError> {
    mutate(config, args, ListingMutationOperation::Archive)
}

fn mutate(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
) -> Result<ListingMutationView, RuntimeError> {
    let contents = fs::read_to_string(&args.file)?;
    let parsed = toml::from_str::<ListingDraftDocument>(&contents).map_err(|error| {
        RuntimeError::Config(format!(
            "invalid listing draft {}: {error}",
            args.file.display()
        ))
    })?;
    let context = mutation_validation_context(config)?;
    let mut canonical = canonicalize_draft(&parsed, &contents, &context).map_err(|error| {
        let issue = match error {
            ListingDraftValidationError::MissingSellerAccount(issue) => {
                return accounts::AccountRuntimeFailure::unresolved_with_detail(
                    format!("{} ({})", issue.message, issue.field),
                    json!({
                        "seller_actor_source": "listing_draft",
                        "listing_file": args.file.display().to_string(),
                        "actions": listing_bound_account_recovery_actions(args.file.as_path()),
                    }),
                )
                .into();
            }
            ListingDraftValidationError::Issue(issue) => issue,
        };
        RuntimeError::Config(format!(
            "invalid listing draft {}: {} ({})",
            args.file.display(),
            issue.message,
            issue.field
        ))
    })?;
    ensure_listing_bound_account(config, &canonical, args.file.as_path())?;

    if matches!(operation, ListingMutationOperation::Archive) {
        canonical.listing.availability = Some(RadrootsListingAvailability::Status {
            status: RadrootsListingStatus::Other {
                value: "archived".to_owned(),
            },
        });
    }

    let (event_draft, listing_addr) = build_listing_event_draft(&canonical)?;

    if config.output.dry_run
        && matches!(config.publish.mode, PublishMode::NostrRelay)
        && matches!(config.signer.backend, SignerBackend::Local)
    {
        validate_local_listing_signer(config, &canonical)?;
    }

    if config.output.dry_run {
        let requested_signer_session_id = match config.publish.mode {
            PublishMode::NostrRelay => args.signer_session_id.clone(),
            PublishMode::Radrootsd => {
                return Ok(radrootsd_preflight_view(
                    config,
                    args,
                    operation,
                    &canonical,
                    listing_addr,
                    event_draft.event,
                    "unavailable",
                    RADROOTSD_PUBLISH_DEFERRED_REASON,
                ));
            }
        };
        return Ok(ListingMutationView {
            state: "dry_run".to_owned(),
            operation: operation.as_str().to_owned(),
            source: listing_write_source(config).to_owned(),
            file: args.file.display().to_string(),
            listing_id: canonical.listing_id.clone(),
            listing_addr: listing_addr.clone(),
            seller_account_id: canonical.seller_account_id.clone(),
            seller_pubkey: canonical.seller_pubkey.clone(),
            seller_actor_source: canonical.seller_actor_source.clone(),
            event_kind: KIND_LISTING,
            dry_run: true,
            deduplicated: false,
            target_relays: Vec::new(),
            connected_relays: Vec::new(),
            acknowledged_relays: Vec::new(),
            failed_relays: Vec::new(),
            job_id: None,
            job_status: None,
            signer_mode: dry_run_signer_mode(config),
            event_id: None,
            event_addr: Some(listing_addr.clone()),
            idempotency_key: args.idempotency_key.clone(),
            signer_session_id: None,
            requested_signer_session_id,
            local_replica: None,
            reason: Some(dry_run_reason(config)),
            job: None,
            event: args.print_event.then_some(event_draft.event),
            actions: vec![format!(
                "radroots listing {} {}",
                operation.as_str(),
                args.file.display()
            )],
        });
    }

    match config.publish.mode {
        PublishMode::NostrRelay => mutate_via_direct_relay(
            config,
            args,
            operation,
            &canonical,
            listing_addr,
            event_draft,
        ),
        PublishMode::Radrootsd => Ok(radrootsd_preflight_view(
            config,
            args,
            operation,
            &canonical,
            listing_addr,
            event_draft.event,
            "unavailable",
            RADROOTSD_PUBLISH_DEFERRED_REASON,
        )),
    }
}

fn mutate_via_direct_relay(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    event_draft: ListingMutationEventDraft,
) -> Result<ListingMutationView, RuntimeError> {
    let signing = if matches!(config.signer.backend, SignerBackend::Local) {
        resolve_listing_signing_identity(config, canonical)?
    } else {
        match resolve_actor_write_authority(config, "seller", canonical.seller_pubkey.as_str()) {
            Ok(_) => {
                return Ok(binding_error_view(
                    config,
                    args,
                    operation,
                    canonical,
                    listing_addr,
                    event_draft.event,
                    ActorWriteBindingError::Unconfigured(
                        "listing publish requires signer mode `local`".to_owned(),
                    ),
                ));
            }
            Err(error) => {
                return Ok(binding_error_view(
                    config,
                    args,
                    operation,
                    canonical,
                    listing_addr,
                    event_draft.event,
                    error,
                ));
            }
        }
    };

    if config.relay.urls.is_empty() {
        return Ok(direct_relay_error_view(
            config,
            args,
            operation,
            canonical,
            listing_addr,
            event_draft.event,
            DirectRelayPublishError::MissingRelays,
        ));
    }

    let signed_event = sign_parts_with_identity(&signing.identity, event_draft.parts)
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    let record = append_signed_event(
        config,
        format!("listing:{}", canonical.listing_id).as_str(),
        Some(canonical.seller_account_id.clone()),
        Some(canonical.seller_pubkey.clone()),
        Some(canonical.farm_d_tag.clone()),
        Some(listing_addr.clone()),
        &signed_event,
    )?;
    let receipt = match publish_signed_event_with_identity(
        &signing.identity,
        &config.relay.urls,
        signed_event,
    ) {
        Ok(receipt) => {
            mark_signed_event_acknowledged(
                config,
                record.record_id.as_str(),
                receipt.target_relays.clone(),
                receipt.connected_relays.clone(),
                receipt.acknowledged_relays.clone(),
                receipt.failed_relays.clone(),
            )?;
            receipt
        }
        Err(
            error @ (DirectRelayPublishError::RelayConfig { .. }
            | DirectRelayPublishError::Connect { .. }
            | DirectRelayPublishError::Publish { .. }),
        ) => {
            mark_signed_event_failed_for_publish_error(config, record.record_id.as_str(), &error)?;
            let mut event = event_draft.event;
            event.event_id = record.event_id.clone();
            event.created_at = record
                .event_created_at
                .and_then(|created_at| u32::try_from(created_at).ok());
            event.signature = record.event_sig.clone();
            return Ok(direct_relay_error_view(
                config,
                args,
                operation,
                canonical,
                listing_addr,
                event,
                error,
            ));
        }
        Err(error) => {
            mark_signed_event_failed_for_publish_error(config, record.record_id.as_str(), &error)?;
            return Err(RuntimeError::Network(error.to_string()));
        }
    };

    Ok(published_mutation_view(
        config,
        args,
        operation,
        canonical,
        listing_addr,
        event_draft.event,
        receipt,
    ))
}

fn listing_write_source(config: &RuntimeConfig) -> &'static str {
    match config.publish.mode {
        PublishMode::NostrRelay => RELAY_LISTING_WRITE_SOURCE,
        PublishMode::Radrootsd => RADROOTSD_LISTING_WRITE_SOURCE,
    }
}

fn dry_run_reason(config: &RuntimeConfig) -> String {
    match config.publish.mode {
        PublishMode::NostrRelay => "dry run requested; relay publish skipped".to_owned(),
        PublishMode::Radrootsd => "dry run requested; radrootsd submission skipped".to_owned(),
    }
}

fn dry_run_signer_mode(config: &RuntimeConfig) -> Option<String> {
    match config.publish.mode {
        PublishMode::NostrRelay => None,
        PublishMode::Radrootsd => Some("nip46".to_owned()),
    }
}

fn scaffold_contents(draft: &ListingDraftDocument) -> Result<String, RuntimeError> {
    let toml = toml::to_string_pretty(draft).map_err(|error| {
        RuntimeError::Config(format!("failed to render listing draft: {error}"))
    })?;
    Ok(format!(
        "# radroots listing draft v1\n# this scaffold applies selected farm defaults and provided product inputs when available\n# review any remaining empty fields, then run `radroots listing validate <file>`\n\n{toml}"
    ))
}

fn validation_context(config: &RuntimeConfig) -> Result<ListingValidationContext, RuntimeError> {
    Ok(ListingValidationContext {
        farm_setup_action: farm_setup_action(config)?,
    })
}

fn mutation_validation_context(
    config: &RuntimeConfig,
) -> Result<ListingValidationContext, RuntimeError> {
    match config.publish.mode {
        PublishMode::NostrRelay => validation_context(config),
        PublishMode::Radrootsd => radrootsd_mutation_validation_context(config),
    }
}

fn radrootsd_mutation_validation_context(
    config: &RuntimeConfig,
) -> Result<ListingValidationContext, RuntimeError> {
    Ok(ListingValidationContext {
        farm_setup_action: farm_setup_action(config)?,
    })
}

fn canonicalize_draft(
    draft: &ListingDraftDocument,
    contents: &str,
    _context: &ListingValidationContext,
) -> Result<CanonicalListingDraft, ListingDraftValidationError> {
    if draft.version != 1 {
        return Err(issue_for_field(
            contents,
            "version",
            format!("unsupported listing draft version `{}`", draft.version),
        )
        .into());
    }
    if draft.kind.trim() != DRAFT_KIND {
        return Err(issue_for_field(
            contents,
            "kind",
            format!("unsupported listing draft kind `{}`", draft.kind),
        )
        .into());
    }

    let listing_id = draft.listing.d_tag.trim().to_owned();
    if !is_d_tag_base64url(&listing_id) {
        return Err(issue_for_field(
            contents,
            "listing.d_tag",
            "listing d_tag must be a 22-character base64url identifier",
        )
        .into());
    }

    let seller_account_id =
        if let Some(account_id) = non_empty(draft.seller_actor.account_id.clone()) {
            account_id
        } else {
            return Err(ListingDraftValidationError::MissingSellerAccount(
                issue_for_field(
                    contents,
                    "seller_actor.account_id",
                    "missing listing seller_actor account_id",
                ),
            ));
        };

    let seller_pubkey = if let Some(pubkey) = non_empty(draft.seller_actor.pubkey.clone()) {
        pubkey
    } else {
        return Err(ListingDraftValidationError::MissingSellerAccount(
            issue_for_field(
                contents,
                "seller_actor.pubkey",
                "missing listing seller_actor pubkey",
            ),
        ));
    };

    let seller_actor_source = if let Some(source) = non_empty(draft.seller_actor.source.clone()) {
        source
    } else {
        return Err(ListingDraftValidationError::MissingSellerAccount(
            issue_for_field(
                contents,
                "seller_actor.source",
                "missing listing seller_actor source",
            ),
        ));
    };
    if !matches!(
        seller_actor_source.as_str(),
        LISTING_SELLER_ACTOR_SOURCE_FARM_CONFIG
            | LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT
            | LISTING_SELLER_ACTOR_SOURCE_REBIND
    ) {
        return Err(issue_for_field(
            contents,
            "seller_actor.source",
            format!("unsupported listing seller_actor source `{seller_actor_source}`"),
        )
        .into());
    }

    let farm_d_tag = if let Some(d_tag) = non_empty(draft.listing.farm_d_tag.clone()) {
        d_tag
    } else {
        return Err(
            issue_for_field(contents, "listing.farm_d_tag", "missing listing farm_d_tag").into(),
        );
    };
    if !is_d_tag_base64url(&farm_d_tag) {
        return Err(issue_for_field(
            contents,
            "listing.farm_d_tag",
            "farm_d_tag must be a 22-character base64url identifier",
        )
        .into());
    }

    let quantity_amount = parse_decimal_field(
        draft.primary_bin.quantity_amount.as_str(),
        contents,
        "primary_bin.quantity_amount",
    )?;
    let quantity_unit = parse_unit_field(
        draft.primary_bin.quantity_unit.as_str(),
        contents,
        "primary_bin.quantity_unit",
    )?;
    let quantity = RadrootsCoreQuantity::new(quantity_amount, quantity_unit)
        .with_optional_label(non_empty(draft.primary_bin.label.clone()))
        .to_canonical()
        .map_err(|error| {
            issue_for_field(
                contents,
                "primary_bin.quantity_unit",
                format!("invalid primary_bin quantity unit conversion: {error}"),
            )
        })?;

    let price_amount = parse_decimal_field(
        draft.primary_bin.price_amount.as_str(),
        contents,
        "primary_bin.price_amount",
    )?;
    let price_currency = parse_currency_field(
        draft.primary_bin.price_currency.as_str(),
        contents,
        "primary_bin.price_currency",
    )?;
    let price_per_amount = parse_decimal_field(
        draft.primary_bin.price_per_amount.as_str(),
        contents,
        "primary_bin.price_per_amount",
    )?;
    let price_per_unit = parse_unit_field(
        draft.primary_bin.price_per_unit.as_str(),
        contents,
        "primary_bin.price_per_unit",
    )?;
    let price = RadrootsCoreQuantityPrice::new(
        RadrootsCoreMoney::new(price_amount, price_currency),
        RadrootsCoreQuantity::new(price_per_amount, price_per_unit),
    )
    .try_to_canonical_unit_price()
    .map_err(|error| {
        issue_for_field(
            contents,
            "primary_bin.price_per_unit",
            format!("invalid primary_bin price definition: {error:?}"),
        )
    })?;

    let inventory_available = parse_decimal_field(
        draft.inventory.available.as_str(),
        contents,
        "inventory.available",
    )?;
    let availability = build_availability(draft, contents)?;
    let delivery_method = build_delivery_method(draft, contents)?;
    let location = build_location(draft);
    let discounts = build_listing_discounts(
        draft,
        contents,
        draft.primary_bin.bin_id.trim(),
        price_currency,
    )?;

    let listing = RadrootsListing {
        d_tag: listing_id.clone(),
        farm: RadrootsFarmRef {
            pubkey: seller_pubkey.clone(),
            d_tag: farm_d_tag.clone(),
        },
        product: RadrootsListingProduct {
            key: draft.product.key.trim().to_owned(),
            title: draft.product.title.trim().to_owned(),
            category: draft.product.category.trim().to_owned(),
            summary: non_empty(draft.product.summary.clone()),
            process: None,
            lot: None,
            location: None,
            profile: None,
            year: None,
        },
        primary_bin_id: draft.primary_bin.bin_id.trim().to_owned(),
        bins: vec![RadrootsListingBin {
            bin_id: draft.primary_bin.bin_id.trim().to_owned(),
            quantity,
            price_per_canonical_unit: price,
            display_amount: None,
            display_unit: None,
            display_label: non_empty(draft.primary_bin.label.clone()),
            display_price: None,
            display_price_unit: None,
        }],
        resource_area: None,
        plot: None,
        discounts,
        inventory_available: Some(inventory_available),
        availability: Some(availability),
        delivery_method: Some(delivery_method),
        location: Some(location),
        images: None,
    };

    Ok(CanonicalListingDraft {
        listing_id,
        seller_account_id,
        seller_pubkey,
        seller_actor_source,
        farm_d_tag,
        listing,
    })
}

fn build_availability(
    draft: &ListingDraftDocument,
    contents: &str,
) -> Result<RadrootsListingAvailability, ListingValidationIssueView> {
    let kind = if draft.availability.kind.trim().is_empty() {
        if draft.availability.start.is_some() || draft.availability.end.is_some() {
            "window"
        } else {
            "status"
        }
    } else {
        draft.availability.kind.trim()
    };

    match kind {
        "status" => {
            let status = draft.availability.status.trim();
            if status.is_empty() {
                return Err(issue_for_field(
                    contents,
                    "availability.status",
                    "missing availability status",
                ));
            }
            Ok(RadrootsListingAvailability::Status {
                status: match status {
                    "active" => RadrootsListingStatus::Active,
                    "sold" => RadrootsListingStatus::Sold,
                    other => RadrootsListingStatus::Other {
                        value: other.to_owned(),
                    },
                },
            })
        }
        "window" => Ok(RadrootsListingAvailability::Window {
            start: draft.availability.start,
            end: draft.availability.end,
        }),
        _ => Err(issue_for_field(
            contents,
            "availability.kind",
            format!("unsupported availability kind `{kind}`"),
        )),
    }
}

fn build_delivery_method(
    draft: &ListingDraftDocument,
    contents: &str,
) -> Result<RadrootsListingDeliveryMethod, ListingValidationIssueView> {
    let method = draft.delivery.method.trim();
    if method.is_empty() {
        return Err(issue_for_field(
            contents,
            "delivery.method",
            "missing delivery method",
        ));
    }

    Ok(match method {
        "pickup" => RadrootsListingDeliveryMethod::Pickup,
        "local_delivery" => RadrootsListingDeliveryMethod::LocalDelivery,
        "shipping" => RadrootsListingDeliveryMethod::Shipping,
        other => RadrootsListingDeliveryMethod::Other {
            method: other.to_owned(),
        },
    })
}

fn build_location(draft: &ListingDraftDocument) -> RadrootsListingLocation {
    RadrootsListingLocation {
        primary: draft.location.primary.trim().to_owned(),
        city: draft.location.city.clone().and_then(non_empty),
        region: draft.location.region.clone().and_then(non_empty),
        country: draft.location.country.clone().and_then(non_empty),
        lat: None,
        lng: None,
        geohash: None,
    }
}

fn build_listing_discounts(
    draft: &ListingDraftDocument,
    contents: &str,
    primary_bin_id: &str,
    price_currency: RadrootsCoreCurrency,
) -> Result<Option<Vec<RadrootsCoreDiscount>>, ListingValidationIssueView> {
    let mut discounts = Vec::new();
    for (index, discount) in draft.discounts.iter().enumerate() {
        let field_prefix = format!("discounts.{index}");
        if discount.id.trim().is_empty() {
            return Err(issue_for_field(
                contents,
                field_prefix.as_str(),
                "discount id must not be empty",
            ));
        }
        let bin_id = discount
            .bin_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(primary_bin_id)
            .to_owned();
        let min = discount.min_bin_count.unwrap_or(1);
        if min == 0 {
            return Err(issue_for_field(
                contents,
                field_prefix.as_str(),
                "discount min_bin_count must be greater than zero",
            ));
        }
        let value = match discount.kind.trim() {
            "percent" => {
                let raw = discount.value.trim();
                if raw.is_empty() {
                    return Err(issue_for_field(
                        contents,
                        field_prefix.as_str(),
                        "percent discount requires value",
                    ));
                }
                let percent = raw.parse::<RadrootsCorePercent>().map_err(|error| {
                    issue_for_field(
                        contents,
                        field_prefix.as_str(),
                        format!("percent discount value is invalid: {error}"),
                    )
                })?;
                RadrootsCoreDiscountValue::Percent(percent)
            }
            "amount" => {
                let raw_amount = discount.amount.trim();
                if raw_amount.is_empty() {
                    return Err(issue_for_field(
                        contents,
                        field_prefix.as_str(),
                        "amount discount requires amount",
                    ));
                }
                let amount = parse_decimal_field(raw_amount, contents, field_prefix.as_str())?;
                let currency = if discount.currency.trim().is_empty() {
                    price_currency
                } else {
                    parse_currency_field(
                        discount.currency.as_str(),
                        contents,
                        field_prefix.as_str(),
                    )?
                };
                RadrootsCoreDiscountValue::MoneyPerBin(RadrootsCoreMoney::new(amount, currency))
            }
            other => {
                return Err(issue_for_field(
                    contents,
                    field_prefix.as_str(),
                    format!("unsupported discount kind `{other}`"),
                ));
            }
        };
        let discount = RadrootsCoreDiscount {
            scope: RadrootsCoreDiscountScope::Bin,
            threshold: RadrootsCoreDiscountThreshold::BinCount { bin_id, min },
            value,
        };
        if !discount.is_non_negative() {
            return Err(issue_for_field(
                contents,
                field_prefix.as_str(),
                "discount value must not be negative",
            ));
        }
        discounts.push(discount);
    }
    Ok((!discounts.is_empty()).then_some(discounts))
}

fn listing_bound_account_issue(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
    contents: &str,
) -> Result<Option<ListingValidationIssueView>, RuntimeError> {
    let Some(account) = configured_account(config, &canonical.seller_account_id)? else {
        return Ok(Some(issue_for_field(
            contents,
            "seller_actor.account_id",
            format!(
                "listing seller_actor account_id `{}` is not present in the local account store",
                canonical.seller_account_id
            ),
        )));
    };
    let account_pubkey = account.record.public_identity.public_key_hex;
    if !account_pubkey.eq_ignore_ascii_case(canonical.seller_pubkey.as_str()) {
        return Ok(Some(issue_for_field(
            contents,
            "seller_actor.pubkey",
            format!(
                "listing seller_actor pubkey `{}` does not match account `{}` pubkey `{account_pubkey}`",
                canonical.seller_pubkey, canonical.seller_account_id
            ),
        )));
    }
    Ok(None)
}

fn ensure_listing_bound_account(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
    file: &Path,
) -> Result<(), RuntimeError> {
    validate_invocation_account_matches_bound(config, canonical, file)?;
    let Some(account) = configured_account(config, &canonical.seller_account_id)? else {
        return Err(accounts::AccountRuntimeFailure::unresolved_with_detail(
            format!(
                "listing-bound seller account `{}` is not present in the local account store",
                canonical.seller_account_id
            ),
            json!({
                "seller_actor_source": canonical.seller_actor_source,
                "listing_seller_account_id": canonical.seller_account_id,
                "listing_file": file.display().to_string(),
                "actions": listing_bound_account_recovery_actions(file),
            }),
        )
        .into());
    };
    let account_pubkey = account.record.public_identity.public_key_hex;
    if !account_pubkey.eq_ignore_ascii_case(canonical.seller_pubkey.as_str()) {
        return Err(accounts::AccountRuntimeFailure::mismatch_with_detail(
            format!(
                "account mismatch: listing-bound seller account `{}` pubkey `{account_pubkey}` cannot sign listing seller_pubkey `{}`",
                canonical.seller_account_id, canonical.seller_pubkey
            ),
            json!({
                "seller_actor_source": canonical.seller_actor_source,
                "listing_seller_account_id": canonical.seller_account_id,
                "listing_seller_pubkey": canonical.seller_pubkey,
                "account_pubkey": account_pubkey,
                "listing_file": file.display().to_string(),
                "actions": listing_bound_account_recovery_actions(file),
            }),
        )
        .into());
    }
    Ok(())
}

fn validate_invocation_account_matches_bound(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
    file: &Path,
) -> Result<(), RuntimeError> {
    let Some(selector) = config
        .account
        .selector
        .as_deref()
        .map(str::trim)
        .filter(|selector| !selector.is_empty())
    else {
        return Ok(());
    };
    let attempted = accounts::resolve_account_selector(config, selector)?;
    if attempted.record.account_id.to_string() == canonical.seller_account_id {
        return Ok(());
    }
    Err(accounts::AccountRuntimeFailure::mismatch_with_detail(
        format!(
            "account mismatch: listing draft is bound to seller account `{}`; invocation selected `{}`",
            canonical.seller_account_id, attempted.record.account_id
        ),
        json!({
            "seller_actor_source": canonical.seller_actor_source,
            "listing_seller_account_id": canonical.seller_account_id,
            "attempted_seller_account_id": attempted.record.account_id.to_string(),
            "listing_file": file.display().to_string(),
            "actions": listing_bound_account_recovery_actions(file),
        }),
    )
    .into())
}

fn listing_bound_account_recovery_actions(file: &Path) -> Vec<String> {
    vec![
        "radroots account import <path>".to_owned(),
        format!("radroots listing rebind {} <selector>", file.display()),
    ]
}

fn invalid_validation_view(
    file: &Path,
    draft: &ListingDraftDocument,
    context: &ListingValidationContext,
    issue: ListingValidationIssueView,
) -> ListingValidateView {
    let mut actions = vec![format!("edit {}", file.display())];
    if draft.seller_actor.account_id.trim().is_empty() {
        actions.push("radroots account create".to_owned());
    } else {
        actions.push(format!(
            "radroots listing rebind {} <selector>",
            file.display()
        ));
    }
    if draft.listing.farm_d_tag.trim().is_empty() {
        actions.push(context.farm_setup_action.clone());
    }

    ListingValidateView {
        state: "invalid".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: file.display().to_string(),
        valid: false,
        listing_id: non_empty(draft.listing.d_tag.clone()),
        seller_account_id: non_empty(draft.seller_actor.account_id.clone()),
        seller_pubkey: non_empty(draft.seller_actor.pubkey.clone()),
        seller_actor_source: non_empty(draft.seller_actor.source.clone()),
        farm_d_tag: non_empty(draft.listing.farm_d_tag.clone()),
        issues: vec![issue],
        actions,
    }
}

fn build_listing_event_draft(
    canonical: &CanonicalListingDraft,
) -> Result<(ListingMutationEventDraft, String), RuntimeError> {
    let parts = to_wire_parts_with_kind(&canonical.listing, KIND_LISTING)
        .map_err(|error| RuntimeError::Config(format!("invalid listing contract: {error}")))?;
    let validated = validate_listing_for_seller(
        canonical.listing.clone(),
        canonical.seller_pubkey.as_str(),
        KIND_LISTING,
    )
    .map_err(|error| RuntimeError::Config(format!("invalid listing contract: {error}")))?;
    Ok((
        ListingMutationEventDraft {
            event: ListingMutationEventView {
                kind: KIND_LISTING,
                author: canonical.seller_pubkey.clone(),
                created_at: None,
                content: parts.content.clone(),
                tags: parts.tags.clone(),
                event_id: None,
                signature: None,
                event_addr: validated.listing_addr.clone(),
            },
            parts,
        },
        validated.listing_addr,
    ))
}

fn radrootsd_preflight_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    event_preview: ListingMutationEventView,
    state: &str,
    reason: impl Into<String>,
) -> ListingMutationView {
    ListingMutationView {
        state: state.to_owned(),
        operation: operation.as_str().to_owned(),
        source: listing_write_source(config).to_owned(),
        file: args.file.display().to_string(),
        listing_id: canonical.listing_id.clone(),
        listing_addr: listing_addr.clone(),
        seller_account_id: canonical.seller_account_id.clone(),
        seller_pubkey: canonical.seller_pubkey.clone(),
        seller_actor_source: canonical.seller_actor_source.clone(),
        event_kind: KIND_LISTING,
        dry_run: false,
        deduplicated: false,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        job_id: None,
        job_status: None,
        signer_mode: Some("deferred".to_owned()),
        event_id: None,
        event_addr: Some(listing_addr),
        idempotency_key: args.idempotency_key.clone(),
        signer_session_id: None,
        requested_signer_session_id: args.signer_session_id.clone(),
        local_replica: None,
        reason: Some(reason.into()),
        job: None,
        event: args.print_event.then_some(event_preview),
        actions: vec![format!(
            "radroots --publish-mode nostr_relay --relay wss://relay.example.com listing {} {}",
            operation.as_str(),
            args.file.display()
        )],
    }
}

fn direct_relay_error_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    mut event_preview: ListingMutationEventView,
    error: DirectRelayPublishError,
) -> ListingMutationView {
    let parts = direct_relay_error_view_parts(config.relay.urls.as_slice(), error);
    let event_id = parts.event_id.or_else(|| event_preview.event_id.clone());
    event_preview.event_id = event_id.clone();

    ListingMutationView {
        state: "unavailable".to_owned(),
        operation: operation.as_str().to_owned(),
        source: listing_write_source(config).to_owned(),
        file: args.file.display().to_string(),
        listing_id: canonical.listing_id.clone(),
        listing_addr: listing_addr.clone(),
        seller_account_id: canonical.seller_account_id.clone(),
        seller_pubkey: canonical.seller_pubkey.clone(),
        seller_actor_source: canonical.seller_actor_source.clone(),
        event_kind: KIND_LISTING,
        dry_run: false,
        deduplicated: false,
        target_relays: parts.target_relays,
        connected_relays: parts.connected_relays,
        acknowledged_relays: Vec::new(),
        failed_relays: parts.failed_relays,
        job_id: None,
        job_status: None,
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        event_id,
        event_addr: Some(listing_addr),
        idempotency_key: args.idempotency_key.clone(),
        signer_session_id: None,
        requested_signer_session_id: args.signer_session_id.clone(),
        local_replica: None,
        reason: Some(parts.reason),
        job: None,
        event: args.print_event.then_some(event_preview),
        actions: Vec::new(),
    }
}

#[derive(Debug, Clone)]
struct DirectRelayErrorViewParts {
    reason: String,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<RelayFailureView>,
    event_id: Option<String>,
}

fn direct_relay_error_view_parts(
    configured_relays: &[String],
    error: DirectRelayPublishError,
) -> DirectRelayErrorViewParts {
    let (reason, target_relays, connected_relays, failed_relays, event_id) = match error {
        DirectRelayPublishError::MissingRelays => (
            "direct relay publish requires at least one configured relay".to_owned(),
            configured_relays.to_vec(),
            Vec::new(),
            Vec::new(),
            None,
        ),
        DirectRelayPublishError::RelayConfig { relay, source } => (
            format!("failed to configure relay `{relay}` for direct relay publish: {source}"),
            configured_relays.to_vec(),
            Vec::new(),
            vec![RelayFailureView {
                relay,
                reason: source.to_string(),
            }],
            None,
        ),
        DirectRelayPublishError::Connect {
            reason,
            target_relays,
            connected_relays,
            failed_relays,
        } => (
            format!("direct relay connection failed: {reason}"),
            target_relays,
            connected_relays,
            relay_failures(failed_relays),
            None,
        ),
        DirectRelayPublishError::Publish {
            event_id,
            reason,
            target_relays,
            connected_relays,
            failed_relays,
        } => (
            format!("direct relay publish failed for event `{event_id}`: {reason}"),
            target_relays,
            connected_relays,
            relay_failures(failed_relays),
            Some(event_id),
        ),
        DirectRelayPublishError::Runtime(_)
        | DirectRelayPublishError::Build(_)
        | DirectRelayPublishError::Sign(_) => unreachable!(),
    };
    DirectRelayErrorViewParts {
        reason,
        target_relays,
        connected_relays,
        failed_relays,
        event_id,
    }
}

fn validate_local_listing_signer(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
) -> Result<(), RuntimeError> {
    resolve_listing_signing_identity(config, canonical).map(|_| ())
}

fn resolve_listing_signing_identity(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
) -> Result<accounts::AccountSigningIdentity, RuntimeError> {
    let signing = accounts::resolve_local_signing_identity_for_account(
        config,
        canonical.seller_account_id.as_str(),
    )
    .map_err(|error| listing_bound_signing_error(error, canonical))?;
    let account_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !account_pubkey.eq_ignore_ascii_case(canonical.seller_pubkey.as_str()) {
        return Err(accounts::AccountRuntimeFailure::mismatch_with_detail(
            format!(
                "account mismatch: listing-bound seller account `{}` pubkey `{account_pubkey}` cannot sign listing seller_pubkey `{}`",
                canonical.seller_account_id, canonical.seller_pubkey
            ),
            json!({
                "seller_actor_source": canonical.seller_actor_source,
                "listing_seller_account_id": canonical.seller_account_id,
                "listing_seller_pubkey": canonical.seller_pubkey,
                "account_pubkey": account_pubkey,
                "actions": [
                    "radroots account import <path>",
                    "radroots account attach-secret <account-id> <path>",
                ],
            }),
        )
        .into());
    }
    Ok(signing)
}

fn listing_bound_signing_error(
    error: RuntimeError,
    canonical: &CanonicalListingDraft,
) -> RuntimeError {
    match error {
        RuntimeError::Account(accounts::AccountRuntimeFailure::Unresolved(issue)) => {
            accounts::AccountRuntimeFailure::unresolved_with_detail(
                issue.message().to_owned(),
                json!({
                    "seller_actor_source": canonical.seller_actor_source,
                    "listing_seller_account_id": canonical.seller_account_id,
                    "listing_seller_pubkey": canonical.seller_pubkey,
                    "actions": [
                        "radroots account import <path>",
                        format!("radroots listing rebind <file> {}", canonical.seller_account_id),
                    ],
                }),
            )
            .into()
        }
        RuntimeError::Account(accounts::AccountRuntimeFailure::WatchOnly(issue)) => {
            accounts::AccountRuntimeFailure::watch_only_with_detail(
                &canonical.seller_account_id,
                json!({
                    "seller_actor_source": canonical.seller_actor_source,
                    "listing_seller_account_id": canonical.seller_account_id,
                    "listing_seller_pubkey": canonical.seller_pubkey,
                    "reason": issue.message(),
                    "actions": [
                        format!("radroots account attach-secret {} <path>", canonical.seller_account_id),
                    ],
                }),
            )
            .into()
        }
        other => other,
    }
}

fn binding_error_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    event_preview: ListingMutationEventView,
    error: ActorWriteBindingError,
) -> ListingMutationView {
    let reason = error.reason();
    let state = "unconfigured".to_owned();
    let actions = vec!["run radroots signer status get".to_owned()];

    ListingMutationView {
        state: state.clone(),
        operation: operation.as_str().to_owned(),
        source: listing_write_source(config).to_owned(),
        file: args.file.display().to_string(),
        listing_id: canonical.listing_id.clone(),
        listing_addr,
        seller_account_id: canonical.seller_account_id.clone(),
        seller_pubkey: canonical.seller_pubkey.clone(),
        seller_actor_source: canonical.seller_actor_source.clone(),
        event_kind: KIND_LISTING,
        dry_run: false,
        deduplicated: false,
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        job_id: None,
        job_status: None,
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        event_id: None,
        event_addr: None,
        idempotency_key: args.idempotency_key.clone(),
        requested_signer_session_id: args.signer_session_id.clone(),
        local_replica: None,
        reason: Some(reason),
        job: None,
        event: args.print_event.then_some(event_preview),
        actions,
    }
}

fn published_mutation_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    mut event: ListingMutationEventView,
    receipt: DirectRelayPublishReceipt,
) -> ListingMutationView {
    let DirectRelayPublishReceipt {
        event: published_event,
        event_id,
        created_at,
        signature,
        target_relays,
        connected_relays,
        acknowledged_relays,
        failed_relays,
    } = receipt;
    debug_assert_eq!(event_id, published_event.id.to_hex());
    debug_assert_eq!(signature, published_event.sig.to_string());
    let local_replica =
        listing_local_replica_ingest_view(config, &published_event, Some(listing_addr.clone()));
    event.event_id = Some(event_id.clone());
    event.created_at = Some(created_at);
    event.signature = Some(signature);
    ListingMutationView {
        state: match operation {
            ListingMutationOperation::Archive => "archived",
            ListingMutationOperation::Publish | ListingMutationOperation::Update => "published",
        }
        .to_owned(),
        operation: operation.as_str().to_owned(),
        source: listing_write_source(config).to_owned(),
        file: args.file.display().to_string(),
        listing_id: canonical.listing_id.clone(),
        listing_addr: listing_addr.clone(),
        seller_account_id: canonical.seller_account_id.clone(),
        seller_pubkey: canonical.seller_pubkey.clone(),
        seller_actor_source: canonical.seller_actor_source.clone(),
        event_kind: KIND_LISTING,
        dry_run: false,
        deduplicated: false,
        target_relays,
        connected_relays,
        acknowledged_relays,
        failed_relays: relay_failures(failed_relays),
        job_id: None,
        job_status: None,
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        event_id: Some(event_id),
        event_addr: Some(listing_addr),
        idempotency_key: args.idempotency_key.clone(),
        requested_signer_session_id: args.signer_session_id.clone(),
        local_replica: Some(local_replica),
        reason: None,
        job: None,
        event: args.print_event.then_some(event),
        actions: Vec::new(),
    }
}

fn listing_local_replica_ingest_view(
    config: &RuntimeConfig,
    event: &SignedNostrEvent,
    event_addr: Option<String>,
) -> ListingMutationLocalReplicaView {
    ingest_listing_event_into_local_replica(
        config.local.replica_db_path.as_path(),
        event,
        event_addr,
    )
}

fn ingest_listing_event_into_local_replica(
    replica_db_path: &Path,
    event: &SignedNostrEvent,
    event_addr: Option<String>,
) -> ListingMutationLocalReplicaView {
    let event_id = event.id.to_hex();
    if !replica_db_path.exists() {
        return ListingMutationLocalReplicaView {
            state: "unconfigured".to_owned(),
            store_state: "missing".to_owned(),
            ingest_outcome: None,
            event_id: Some(event_id),
            event_addr,
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        };
    }

    let executor = match SqliteExecutor::open(replica_db_path) {
        Ok(executor) => executor,
        Err(error) => {
            return listing_local_replica_failed_view(
                event_id,
                event_addr,
                format!("failed to open local replica database: {error}"),
            );
        }
    };
    if let Err(error) = migrations::run_all_up(&executor) {
        return listing_local_replica_failed_view(
            event_id,
            event_addr,
            format!("failed to migrate local replica database: {error}"),
        );
    }

    let event = radroots_event_from_nostr(event);
    match radroots_replica_ingest_event(&executor, &event) {
        Ok(RadrootsReplicaIngestOutcome::Applied) => ListingMutationLocalReplicaView {
            state: "applied".to_owned(),
            store_state: "ready".to_owned(),
            ingest_outcome: Some("applied".to_owned()),
            event_id: Some(event_id),
            event_addr,
            reason: None,
            actions: Vec::new(),
        },
        Ok(RadrootsReplicaIngestOutcome::Skipped) => ListingMutationLocalReplicaView {
            state: "skipped".to_owned(),
            store_state: "ready".to_owned(),
            ingest_outcome: Some("skipped".to_owned()),
            event_id: Some(event_id),
            event_addr,
            reason: Some("shared replica ingest skipped the event".to_owned()),
            actions: Vec::new(),
        },
        Err(error) => listing_local_replica_failed_view(
            event_id,
            event_addr,
            format!("failed to ingest listing event into local replica: {error}"),
        ),
    }
}

fn listing_local_replica_failed_view(
    event_id: String,
    event_addr: Option<String>,
    reason: String,
) -> ListingMutationLocalReplicaView {
    ListingMutationLocalReplicaView {
        state: "failed".to_owned(),
        store_state: "unavailable".to_owned(),
        ingest_outcome: None,
        event_id: Some(event_id),
        event_addr,
        reason: Some(reason),
        actions: vec!["radroots store status get".to_owned()],
    }
}

fn relay_failures(failures: Vec<DirectRelayFailure>) -> Vec<RelayFailureView> {
    failures
        .into_iter()
        .map(|failure| RelayFailureView {
            relay: failure.relay,
            reason: failure.reason,
        })
        .collect()
}

fn issue_from_trade_validation(
    error: RadrootsTradeListingValidationError,
    contents: &str,
) -> ListingValidationIssueView {
    match error {
        RadrootsTradeListingValidationError::InvalidSeller => issue_for_field(
            contents,
            "seller_actor.pubkey",
            "listing author does not match the farm pubkey",
        ),
        RadrootsTradeListingValidationError::MissingTitle => {
            issue_for_field(contents, "product.title", "missing listing title")
        }
        RadrootsTradeListingValidationError::MissingDescription => {
            issue_for_field(contents, "product.summary", "missing listing description")
        }
        RadrootsTradeListingValidationError::MissingProductType => {
            issue_for_field(contents, "product.category", "missing listing product type")
        }
        RadrootsTradeListingValidationError::MissingBins
        | RadrootsTradeListingValidationError::MissingPrimaryBin
        | RadrootsTradeListingValidationError::InvalidBin => {
            issue_for_field(contents, "primary_bin.bin_id", error.to_string())
        }
        RadrootsTradeListingValidationError::InvalidPrice => issue_for_field(
            contents,
            "primary_bin.price_amount",
            "invalid listing price",
        ),
        RadrootsTradeListingValidationError::MissingInventory
        | RadrootsTradeListingValidationError::InvalidInventory => {
            issue_for_field(contents, "inventory.available", error.to_string())
        }
        RadrootsTradeListingValidationError::MissingAvailability => issue_for_field(
            contents,
            "availability.status",
            "missing listing availability",
        ),
        RadrootsTradeListingValidationError::MissingLocation => {
            issue_for_field(contents, "location.primary", "missing listing location")
        }
        RadrootsTradeListingValidationError::MissingDeliveryMethod => issue_for_field(
            contents,
            "delivery.method",
            "missing listing delivery method",
        ),
        other => issue_for_field(contents, "listing", other.to_string()),
    }
}

fn authoring_defaults(config: &RuntimeConfig) -> Result<ListingAuthoringDefaults, RuntimeError> {
    let account_resolution = accounts::resolve_account_resolution(config)?;
    let Some(selected_account) = account_resolution.resolved_account.clone() else {
        return Err(accounts::AccountRuntimeFailure::unresolved_with_detail(
            "no resolved account is available for listing seller actor",
            json!({
                "seller_actor_source": LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT,
                "actions": [
                    "radroots account create",
                    "radroots account import <path>",
                ],
            }),
        )
        .into());
    };
    let mut defaults = ListingAuthoringDefaults {
        farm_config_present: false,
        farm_defaults_ready: false,
        farm_next_action: Some(farm_setup_action(config)?),
        farm_reason: Some(
            "selected farm draft not found; delivery, location, and farm defaults were left blank"
                .to_owned(),
        ),
        farm_name: None,
        seller_account_id: selected_account.record.account_id.to_string(),
        seller_pubkey: selected_account
            .record
            .public_identity
            .public_key_hex
            .clone(),
        seller_actor_source: LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT.to_owned(),
        selected_farm_d_tag: None,
        delivery_method: None,
        location: None,
    };

    let Some(resolved) = farm_config::load(config, None)? else {
        return Ok(defaults);
    };
    let Some(account) = configured_account(config, &resolved.document.selection.account)? else {
        let account_id = resolved.document.selection.account.clone();
        return Err(accounts::AccountRuntimeFailure::unresolved_with_detail(
            format!(
                "farm-bound seller account `{account_id}` is not present in the local account store"
            ),
            json!({
                "seller_actor_source": "farm_config",
                "farm_bound_seller_account_id": account_id,
                "actions": [
                    "radroots account import <path>",
                    "radroots farm rebind <selector>",
                ],
            }),
        )
        .into());
    };

    defaults.farm_config_present = true;
    defaults.farm_name = resolved
        .document
        .profile
        .display_name
        .clone()
        .and_then(non_empty)
        .or_else(|| non_empty(resolved.document.profile.name.clone()))
        .or_else(|| non_empty(resolved.document.farm.name.clone()));
    defaults.seller_account_id = resolved.document.selection.account.clone();
    defaults.seller_pubkey = account.record.public_identity.public_key_hex.clone();
    defaults.seller_actor_source = LISTING_SELLER_ACTOR_SOURCE_FARM_CONFIG.to_owned();
    defaults.selected_farm_d_tag = Some(resolved.document.selection.farm_d_tag.clone());
    let draft_missing = farm_config::missing_fields(&resolved.document);
    defaults.farm_defaults_ready = !draft_missing.iter().any(|field| {
        matches!(
            field,
            farm_config::FarmMissingField::Location | farm_config::FarmMissingField::Delivery
        )
    });
    if defaults.farm_defaults_ready {
        defaults.delivery_method = Some(resolved.document.listing_defaults.delivery_method.clone());
        defaults.location = Some(draft_location_from_model(
            &resolved.document.listing_defaults.location,
        ));
        defaults.farm_next_action = None;
        defaults.farm_reason = None;
    } else {
        defaults.farm_next_action = Some("radroots farm readiness check".to_owned());
        defaults.farm_reason = Some(
            "selected farm draft is missing delivery or location defaults; those fields were left blank"
                .to_owned(),
        );
    }
    Ok(defaults)
}

fn draft_location_from_model(location: &RadrootsListingLocation) -> ListingDraftLocation {
    ListingDraftLocation {
        primary: location.primary.clone(),
        city: location.city.clone(),
        region: location.region.clone(),
        country: location.country.clone(),
    }
}

fn farm_setup_action(_config: &RuntimeConfig) -> Result<String, RuntimeError> {
    Ok("radroots farm create".to_owned())
}

fn drafts_dir(config: &RuntimeConfig) -> PathBuf {
    config.paths.app_data_root.join(LISTING_DRAFTS_DIR)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_owned()
}

fn modified_unix(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs())
}

fn configured_account(
    config: &RuntimeConfig,
    account_id: &str,
) -> Result<Option<accounts::AccountRecordView>, RuntimeError> {
    let snapshot = accounts::snapshot(config)?;
    Ok(snapshot
        .accounts
        .into_iter()
        .find(|account| account.record.account_id.as_str() == account_id))
}

fn parse_decimal_field(
    value: &str,
    contents: &str,
    field: &str,
) -> Result<RadrootsCoreDecimal, ListingValidationIssueView> {
    value.trim().parse::<RadrootsCoreDecimal>().map_err(|_| {
        issue_for_field(
            contents,
            field,
            format!("`{field}` must be a valid decimal value"),
        )
    })
}

fn parse_unit_field(
    value: &str,
    contents: &str,
    field: &str,
) -> Result<RadrootsCoreUnit, ListingValidationIssueView> {
    value.parse::<RadrootsCoreUnit>().map_err(|_| {
        issue_for_field(
            contents,
            field,
            format!("`{field}` must be a valid unit code"),
        )
    })
}

fn parse_currency_field(
    value: &str,
    contents: &str,
    field: &str,
) -> Result<RadrootsCoreCurrency, ListingValidationIssueView> {
    let upper = value.trim().to_ascii_uppercase();
    RadrootsCoreCurrency::from_str_upper(&upper).map_err(|_| {
        issue_for_field(
            contents,
            field,
            format!("`{field}` must be a valid ISO currency code"),
        )
    })
}

fn issue_for_field(
    contents: &str,
    field: &str,
    message: impl Into<String>,
) -> ListingValidationIssueView {
    ListingValidationIssueView {
        field: field.to_owned(),
        message: message.into(),
        line: line_for_field(contents, field),
    }
}

fn line_for_field(contents: &str, field: &str) -> Option<usize> {
    let needles: &[&str] = match field {
        "version" => &["version ="],
        "kind" => &["kind ="],
        "listing.d_tag" => &["d_tag ="],
        "listing.farm_d_tag" => &["farm_d_tag ="],
        "seller_actor.account_id" => &["[seller_actor]", "account_id ="],
        "seller_actor.pubkey" => &["[seller_actor]", "pubkey ="],
        "seller_actor.source" => &["[seller_actor]", "source ="],
        "product.key" => &["key ="],
        "product.title" => &["title ="],
        "product.category" => &["category ="],
        "product.summary" => &["summary ="],
        "primary_bin.bin_id" => &["bin_id ="],
        "primary_bin.quantity_amount" => &["quantity_amount ="],
        "primary_bin.quantity_unit" => &["quantity_unit ="],
        "primary_bin.price_amount" => &["price_amount ="],
        "primary_bin.price_currency" => &["price_currency ="],
        "primary_bin.price_per_amount" => &["price_per_amount ="],
        "primary_bin.price_per_unit" => &["price_per_unit ="],
        "inventory.available" => &["available ="],
        "availability.kind" => &["[availability]", "kind ="],
        "availability.status" => &["status ="],
        "delivery.method" => &["method ="],
        "location.primary" => &["primary ="],
        field if field.starts_with("discounts.") => &["[[discounts]]"],
        _ => &[],
    };
    for needle in needles {
        if let Some(line) = contents.lines().position(|line| line.contains(needle)) {
            return Some(line + 1);
        }
    }
    None
}

fn line_for_offset(contents: &str, offset: usize) -> usize {
    let mut seen = 0usize;
    for (index, line) in contents.lines().enumerate() {
        seen += line.len() + 1;
        if seen >= offset {
            return index + 1;
        }
    }
    contents.lines().count().max(1)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn generate_d_tag() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = D_TAG_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    let mixed = nanos ^ counter;
    encode_base64url_no_pad(mixed.to_be_bytes())
}

fn encode_base64url_no_pad(bytes: [u8; 16]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(22);
    let mut index = 0usize;
    while index + 3 <= bytes.len() {
        let block = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
        output.push(ALPHABET[(block & 0x3f) as usize] as char);
        index += 3;
    }
    let remaining = bytes.len() - index;
    if remaining == 1 {
        let block = (bytes[index] as u32) << 16;
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
    } else if remaining == 2 {
        let block = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        DRAFT_KIND, ListingDraftDocument, direct_relay_error_view_parts, encode_base64url_no_pad,
        generate_d_tag, ingest_listing_event_into_local_replica,
    };
    use crate::runtime::direct_relay::{DirectRelayFailure, DirectRelayPublishError};
    use radroots_events_codec::d_tag::is_d_tag_base64url;
    use radroots_events_codec::wire::WireEventParts;
    use radroots_identity::RadrootsIdentity;
    use radroots_nostr::prelude::radroots_nostr_build_event;

    #[test]
    fn generated_listing_d_tag_is_valid_base64url() {
        let d_tag = generate_d_tag();
        assert!(is_d_tag_base64url(&d_tag));
    }

    #[test]
    fn base64url_encoder_produces_twenty_two_characters_for_sixteen_bytes() {
        let encoded = encode_base64url_no_pad([0u8; 16]);
        assert_eq!(encoded.len(), 22);
        assert!(is_d_tag_base64url(&encoded));
    }

    #[test]
    fn direct_relay_publish_error_parts_preserve_event_id() {
        let parts = direct_relay_error_view_parts(
            &["ws://127.0.0.1:19000".to_owned()],
            DirectRelayPublishError::Publish {
                event_id: "e".repeat(64),
                reason: "relay rejected event".to_owned(),
                target_relays: vec!["ws://127.0.0.1:19000".to_owned()],
                connected_relays: vec!["ws://127.0.0.1:19000".to_owned()],
                failed_relays: vec![DirectRelayFailure {
                    relay: "ws://127.0.0.1:19000".to_owned(),
                    reason: "relay rejected event".to_owned(),
                }],
            },
        );

        assert_eq!(parts.event_id, Some("e".repeat(64)));
        assert!(parts.reason.contains("direct relay publish failed"));
        assert_eq!(parts.failed_relays.len(), 1);
    }

    #[test]
    fn local_replica_ingest_reports_missing_store() {
        let temp = tempfile::tempdir().expect("tempdir");
        let event = signed_test_listing_event(WireEventParts {
            kind: super::KIND_LISTING,
            content: "{}".to_owned(),
            tags: vec![vec!["d".to_owned(), "listing-1".to_owned()]],
        });

        let view = ingest_listing_event_into_local_replica(
            &temp.path().join("missing.sqlite"),
            &event,
            Some("30402:pubkey:listing-1".to_owned()),
        );

        assert_eq!(view.state, "unconfigured");
        assert_eq!(view.store_state, "missing");
        assert_eq!(view.event_id, Some(event.id.to_hex()));
        assert_eq!(view.actions, vec!["radroots store init".to_owned()]);
    }

    #[test]
    fn local_replica_ingest_preserves_shared_ingest_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let replica = temp.path().join("replica.sqlite");
        std::fs::File::create(&replica).expect("replica placeholder");
        let event = signed_test_listing_event(WireEventParts {
            kind: super::KIND_LISTING,
            content: "{}".to_owned(),
            tags: vec![vec!["d".to_owned(), "listing-1".to_owned()]],
        });

        let view = ingest_listing_event_into_local_replica(
            &replica,
            &event,
            Some("30402:pubkey:listing-1".to_owned()),
        );

        assert_eq!(view.state, "failed");
        assert_eq!(view.store_state, "unavailable");
        assert_eq!(view.event_id, Some(event.id.to_hex()));
        assert!(
            view.reason
                .as_deref()
                .unwrap_or_default()
                .contains("failed to ingest listing event into local replica")
        );
    }

    #[test]
    fn local_replica_ingest_makes_listing_writes_visible_to_local_reads() {
        let temp = tempfile::tempdir().expect("tempdir");
        let replica = temp.path().join("replica.sqlite");
        std::fs::File::create(&replica).expect("replica placeholder");
        let identity = RadrootsIdentity::generate();
        let seller_pubkey = identity.public_key_hex();
        let listing_d_tag = "AAAAAAAAAAAAAAAAAAAAAQ";
        let listing_addr = format!(
            "{}:{}:{}",
            super::KIND_LISTING,
            seller_pubkey,
            listing_d_tag
        );

        let active = signed_test_listing_event_with_identity(
            &identity,
            test_listing_wire_parts(&seller_pubkey, listing_d_tag, "active", "Pasture Eggs"),
        );
        let active_view =
            ingest_listing_event_into_local_replica(&replica, &active, Some(listing_addr.clone()));
        assert_eq!(active_view.state, "applied");
        assert_eq!(active_view.store_state, "ready");
        assert_eq!(active_view.ingest_outcome.as_deref(), Some("applied"));

        let db = super::ReplicaSql::new(super::SqliteExecutor::open(&replica).expect("open db"));
        let active_rows = db
            .trade_product_search(&["eggs".to_owned()])
            .expect("search active");
        assert_eq!(active_rows.len(), 1);
        assert_eq!(active_rows[0].title, "Pasture Eggs");
        assert_eq!(
            active_rows[0].listing_addr.as_deref(),
            Some(listing_addr.as_str())
        );

        let updated = signed_test_listing_event_with_identity(
            &identity,
            test_listing_wire_parts(&seller_pubkey, listing_d_tag, "active", "Market Eggs"),
        );
        let updated_view =
            ingest_listing_event_into_local_replica(&replica, &updated, Some(listing_addr.clone()));
        assert_eq!(updated_view.state, "applied");
        let db = super::ReplicaSql::new(super::SqliteExecutor::open(&replica).expect("open db"));
        let updated_rows = db
            .trade_product_search(&["eggs".to_owned()])
            .expect("search updated");
        assert_eq!(updated_rows.len(), 1);
        assert_eq!(updated_rows[0].title, "Market Eggs");

        let archived = signed_test_listing_event_with_identity(
            &identity,
            test_listing_wire_parts(&seller_pubkey, listing_d_tag, "archived", "Market Eggs"),
        );
        let archived_view =
            ingest_listing_event_into_local_replica(&replica, &archived, Some(listing_addr));
        assert_eq!(archived_view.state, "applied");
        let db = super::ReplicaSql::new(super::SqliteExecutor::open(&replica).expect("open db"));
        let archived_rows = db
            .trade_product_search(&["eggs".to_owned()])
            .expect("search archived");
        assert!(archived_rows.is_empty());
    }

    #[test]
    fn listing_draft_kind_constant_is_stable() {
        let document = ListingDraftDocument {
            version: 1,
            kind: DRAFT_KIND.to_owned(),
            listing: super::ListingDraftMeta {
                d_tag: "AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
                farm_d_tag: "AAAAAAAAAAAAAAAAAAAAAw".to_owned(),
            },
            seller_actor: super::ListingDraftSellerActor {
                account_id: "acct_seller".to_owned(),
                pubkey: "a".repeat(64),
                source: super::LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT.to_owned(),
            },
            product: super::ListingDraftProduct {
                key: "sku".to_owned(),
                title: "Widget".to_owned(),
                category: "produce".to_owned(),
                summary: "Fresh".to_owned(),
            },
            primary_bin: super::ListingDraftPrimaryBin {
                bin_id: "bin-1".to_owned(),
                quantity_amount: "1".to_owned(),
                quantity_unit: "kg".to_owned(),
                price_amount: "12.50".to_owned(),
                price_currency: "USD".to_owned(),
                price_per_amount: "1".to_owned(),
                price_per_unit: "kg".to_owned(),
                label: "kg".to_owned(),
            },
            inventory: super::ListingDraftInventory {
                available: "2".to_owned(),
            },
            availability: super::ListingDraftAvailability {
                kind: "status".to_owned(),
                status: "active".to_owned(),
                start: None,
                end: None,
            },
            delivery: super::ListingDraftDelivery {
                method: "pickup".to_owned(),
            },
            location: super::ListingDraftLocation {
                primary: "Asheville".to_owned(),
                city: None,
                region: None,
                country: None,
            },
            discounts: Vec::new(),
        };
        let rendered = toml::to_string_pretty(&document).expect("render draft");
        assert!(rendered.contains("kind = \"listing_draft_v1\""));
    }

    #[test]
    fn listing_draft_canonicalization_preserves_discounts() {
        let seller_pubkey = "a".repeat(64);
        let document = ListingDraftDocument {
            version: 1,
            kind: DRAFT_KIND.to_owned(),
            listing: super::ListingDraftMeta {
                d_tag: "AAAAAAAAAAAAAAAAAAAAAg".to_owned(),
                farm_d_tag: "AAAAAAAAAAAAAAAAAAAAAw".to_owned(),
            },
            seller_actor: super::ListingDraftSellerActor {
                account_id: "acct_seller".to_owned(),
                pubkey: seller_pubkey.clone(),
                source: super::LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT.to_owned(),
            },
            product: super::ListingDraftProduct {
                key: "sku".to_owned(),
                title: "Widget".to_owned(),
                category: "produce".to_owned(),
                summary: "Fresh".to_owned(),
            },
            primary_bin: super::ListingDraftPrimaryBin {
                bin_id: "bin-1".to_owned(),
                quantity_amount: "1".to_owned(),
                quantity_unit: "each".to_owned(),
                price_amount: "10".to_owned(),
                price_currency: "USD".to_owned(),
                price_per_amount: "1".to_owned(),
                price_per_unit: "each".to_owned(),
                label: "each".to_owned(),
            },
            inventory: super::ListingDraftInventory {
                available: "2".to_owned(),
            },
            availability: super::ListingDraftAvailability {
                kind: "status".to_owned(),
                status: "active".to_owned(),
                start: None,
                end: None,
            },
            delivery: super::ListingDraftDelivery {
                method: "pickup".to_owned(),
            },
            location: super::ListingDraftLocation {
                primary: "Asheville".to_owned(),
                city: None,
                region: None,
                country: None,
            },
            discounts: vec![super::ListingDraftDiscount {
                id: "discount_farmstand".to_owned(),
                label: "farmstand pickup".to_owned(),
                kind: "percent".to_owned(),
                value: "10".to_owned(),
                amount: String::new(),
                currency: String::new(),
                bin_id: None,
                min_bin_count: None,
            }],
        };
        let contents = toml::to_string_pretty(&document).expect("render draft");
        let context = super::ListingValidationContext {
            farm_setup_action: "radroots farm create".to_owned(),
        };

        let canonical =
            super::canonicalize_draft(&document, contents.as_str(), &context).expect("canonical");

        assert!(contents.contains("[[discounts]]"));
        assert_eq!(
            canonical
                .listing
                .discounts
                .as_ref()
                .expect("discounts")
                .len(),
            1
        );
    }

    fn signed_test_listing_event(
        parts: WireEventParts,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        let identity = RadrootsIdentity::generate();
        signed_test_listing_event_with_identity(&identity, parts)
    }

    fn signed_test_listing_event_with_identity(
        identity: &RadrootsIdentity,
        parts: WireEventParts,
    ) -> radroots_nostr::prelude::RadrootsNostrEvent {
        radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
            .expect("event builder")
            .sign_with_keys(identity.keys())
            .expect("signed event")
    }

    fn test_listing_wire_parts(
        seller_pubkey: &str,
        listing_d_tag: &str,
        status: &str,
        title: &str,
    ) -> WireEventParts {
        let farm_d_tag = "AAAAAAAAAAAAAAAAAAAAAA";
        WireEventParts {
            kind: super::KIND_LISTING,
            content: format!("# {title}"),
            tags: vec![
                vec!["d".to_owned(), listing_d_tag.to_owned()],
                vec![
                    "a".to_owned(),
                    format!(
                        "{}:{}:{}",
                        radroots_events::kinds::KIND_FARM,
                        seller_pubkey,
                        farm_d_tag
                    ),
                ],
                vec!["p".to_owned(), seller_pubkey.to_owned()],
                vec!["key".to_owned(), "pasture-eggs".to_owned()],
                vec!["title".to_owned(), title.to_owned()],
                vec!["category".to_owned(), "eggs".to_owned()],
                vec!["summary".to_owned(), "Pasture-raised eggs".to_owned()],
                vec!["radroots:primary_bin".to_owned(), "bin-a".to_owned()],
                vec![
                    "radroots:bin".to_owned(),
                    "bin-a".to_owned(),
                    "12".to_owned(),
                    "each".to_owned(),
                    "12".to_owned(),
                    "each".to_owned(),
                    "dozen".to_owned(),
                ],
                vec![
                    "radroots:price".to_owned(),
                    "bin-a".to_owned(),
                    "6".to_owned(),
                    "USD".to_owned(),
                    "1".to_owned(),
                    "each".to_owned(),
                    "6".to_owned(),
                    "each".to_owned(),
                ],
                vec!["inventory".to_owned(), "5".to_owned()],
                vec!["status".to_owned(), status.to_owned()],
            ],
        }
    }
}
