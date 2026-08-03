use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_core::pricing::{Discount, DiscountScope, DiscountThreshold, DiscountValue};
use radroots_core::{Currency, Decimal, Money, Percent, Quantity, QuantityPrice, Unit};
use radroots_event::contract::AuthorRole;
use radroots_event::envelope::kind::KIND_CLASSIFIED_LISTING;
use radroots_event::farm::FarmRef;
use radroots_event::id::{DTag, InventoryBinId};
use radroots_event::listing::operational::{
    OperationalListing, OperationalListingAvailability, OperationalListingBin,
    OperationalListingDeliveryMethod, OperationalListingProduct, OperationalListingPublicLocation,
    OperationalListingStatus,
};
use radroots_event::trade::validation::OperationalListingValidationError;
use radroots_event_codec::d_tag::is_d_tag_base64url;
use radroots_event_codec::operational_listing::encode::to_wire_parts_with_kind;
use radroots_identity::PublicKey;
use radroots_replica_store::ReplicaSql;
use radroots_runtime_store::{RuntimeStoreRecord, RuntimeStoreRecordFamily, SourceRuntime};
use radroots_sdk::listing::{self as sdk_listing, Plan as ListingPlan};
use radroots_signing::{Actor, actor::ActorSource};
use radroots_sql_core::SqlxSqliteExecutor;
use radroots_trade::operational_listing::{
    RadrootsOperationalListingEditDocumentV1, validate_operational_listing_model,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::global::{
    ListingAppRecordExportArgs, ListingCreateArgs, ListingFileArgs, ListingMutationArgs,
    ListingRebindArgs, RecordLookupArgs,
};
use crate::runtime::RuntimeError;
use crate::runtime::account;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::farm_config;
use crate::runtime::runtime_store::{
    append_local_work, get_shared_record, list_shared_records_before, list_shared_records_latest,
    shared_runtime_store_db_path,
};
use crate::runtime::sdk::{CliSdkAdapterError, validate_configured_signer_for_actor};
use crate::runtime::sync::{
    RelayIngestScope, freshness_for_scope_from_executor, market_refresh, missing_freshness,
};
use crate::view::runtime::{
    FindPriceView, FindQuantityView, FindResultProvenanceView, ListingAppRecordExportView,
    ListingAppRecordListView, ListingAppRecordSummaryView, ListingGetView, ListingListView,
    ListingMutationEventView, ListingMutationView, ListingNewView, ListingRebindView,
    ListingSummaryView, ListingValidateView, ListingValidationIssueView, MarketReadinessView,
};

const DRAFT_KIND: &str = "listing_draft_v1";
const LISTING_SOURCE: &str = "local draft · local first";
const LISTING_READ_SOURCE: &str = "local replica · local first";
const LISTING_APP_RECORD_SOURCE: &str = "shared runtime store · app";
const SDK_LISTING_WRITE_SOURCE: &str = "SDK listing publish · configured signer";
const LISTING_DRAFTS_DIR: &str = "listings/drafts";
const LISTING_SELLER_ACTOR_SOURCE_FARM_CONFIG: &str = "farm_config";
const LISTING_SELLER_ACTOR_SOURCE_RESOLVED_ACCOUNT: &str = "resolved_account";
const LISTING_SELLER_ACTOR_SOURCE_REBIND: &str = "listing_rebind";
const CANONICAL_OWNER_PUBKEY_REQUIRED_REASON: &str = "canonical hex pubkey required before export";
const APP_RECORD_LIST_LIMIT: u32 = 500;

static D_TAG_COUNTER: AtomicU64 = AtomicU64::new(0);

fn protocol_d_tag(value: &str, field: &str) -> Result<DTag, RuntimeError> {
    value
        .parse()
        .map_err(|error| RuntimeError::Config(format!("{field} is not a valid d tag: {error}")))
}

fn protocol_inventory_bin_id(value: &str, field: &str) -> Result<InventoryBinId, RuntimeError> {
    value.parse().map_err(|error| {
        RuntimeError::Config(format!("{field} is not a valid inventory bin id: {error}"))
    })
}

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
    geohash: String,
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
    listing: OperationalListing,
}

#[derive(Debug, Clone)]
struct SdkListingPublishInput {
    canonical: CanonicalListingDraft,
    actor: Actor,
    document: RadrootsOperationalListingEditDocumentV1,
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
    Pause,
    Withdraw,
}

impl ListingMutationOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Update => "update",
            Self::Pause => "pause",
            Self::Withdraw => "withdraw",
        }
    }

    fn listing_status(self) -> Option<&'static str> {
        match self {
            Self::Pause => Some("paused"),
            Self::Withdraw => Some("withdrawn"),
            Self::Publish | Self::Update => None,
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
        "radroots listing publish {}",
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
        "radroots listing publish {}",
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
            geohash: String::new(),
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
    if let Some(parent) = output_path.parent()
        && parent.exists()
        && !parent.is_dir()
    {
        return Err(RuntimeError::Config(format!(
            "listing draft parent {} is not a directory",
            parent.display()
        )));
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
            match to_wire_parts_with_kind(&canonical.listing, KIND_CLASSIFIED_LISTING) {
                Ok(_) => {}
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
            }
            if let Some(issue) = listing_bound_account_issue(config, &canonical, &contents)? {
                return Ok(invalid_validation_view(
                    args.file.as_path(),
                    &parsed,
                    &context,
                    issue,
                ));
            }
            match validate_operational_listing_draft(&canonical) {
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

pub fn app_record_list(config: &RuntimeConfig) -> Result<ListingAppRecordListView, RuntimeError> {
    let database_path = shared_runtime_store_db_path(config)?;
    let mut entries = current_app_record_entries(app_local_records(config)?);
    let has_more = entries.len() > APP_RECORD_LIST_LIMIT as usize;
    if has_more {
        entries.truncate(APP_RECORD_LIST_LIMIT as usize);
    }
    let next_cursor = if has_more {
        entries
            .last()
            .map(|entry| (entry.record.change_seq, entry.record.seq))
    } else {
        None
    };
    let records = entries
        .iter()
        .map(|entry| app_record_summary(&entry.record, entry.superseded_count))
        .collect::<Vec<_>>();
    let state = if records.is_empty() { "empty" } else { "ready" };
    let actions = if records.is_empty() {
        vec!["create or save a farm listing in radroots_studio_app".to_owned()]
    } else {
        Vec::new()
    };

    Ok(ListingAppRecordListView {
        state: state.to_owned(),
        source: LISTING_APP_RECORD_SOURCE.to_owned(),
        count: records.len(),
        limit: APP_RECORD_LIST_LIMIT,
        has_more,
        next_before_change_seq: next_cursor.map(|(change_seq, _)| change_seq),
        next_before_seq: next_cursor.map(|(_, seq)| seq),
        runtime_store_db: database_path.display().to_string(),
        records,
        actions,
    })
}

pub fn app_record_export(
    config: &RuntimeConfig,
    args: &ListingAppRecordExportArgs,
) -> Result<ListingAppRecordExportView, RuntimeError> {
    let Some(record) = get_shared_record(config, args.record_id.as_str())? else {
        return Ok(ListingAppRecordExportView {
            state: "missing".to_owned(),
            source: LISTING_APP_RECORD_SOURCE.to_owned(),
            record_id: args.record_id.clone(),
            dry_run: config.output.dry_run,
            file: args
                .output
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            valid: false,
            listing_id: None,
            listing_addr: None,
            seller_account_id: None,
            seller_pubkey: None,
            seller_actor_source: None,
            farm_d_tag: None,
            issues: Vec::new(),
            reason: Some(format!(
                "app-authored local record `{}` was not found",
                args.record_id
            )),
            actions: vec!["radroots listing app list".to_owned()],
        });
    };

    if let Some(current_record) = current_app_record_for(config, &record)?
        && current_record.record_id != record.record_id
    {
        let (listing_id, title, farm_d_tag) = app_listing_display_parts(&record);
        let current_action = format!("radroots listing app export {}", current_record.record_id);
        return Ok(ListingAppRecordExportView {
            state: "stale".to_owned(),
            source: LISTING_APP_RECORD_SOURCE.to_owned(),
            record_id: args.record_id.clone(),
            dry_run: config.output.dry_run,
            file: args
                .output
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            valid: false,
            listing_id,
            listing_addr: record.listing_addr.clone(),
            seller_account_id: record.owner_account_id.clone(),
            seller_pubkey: record.owner_pubkey.clone(),
            seller_actor_source: None,
            farm_d_tag: farm_d_tag.or(record.farm_id.clone()),
            issues: vec![ListingValidationIssueView {
                field: "record_id".to_owned(),
                message: format!(
                    "app-authored local record `{}` was superseded by `{}`",
                    record.record_id, current_record.record_id
                ),
                line: None,
            }],
            reason: Some(format!(
                "app-authored local record `{}` was superseded by current record `{}`{}",
                record.record_id,
                current_record.record_id,
                title
                    .as_deref()
                    .map(|value| format!(" for `{value}`"))
                    .unwrap_or_default()
            )),
            actions: vec![current_action, "radroots listing app list".to_owned()],
        });
    }

    let draft = match app_listing_draft_from_record(&record) {
        Ok(draft) => draft,
        Err(reason) => {
            return Ok(ListingAppRecordExportView {
                state: "unsupported".to_owned(),
                source: LISTING_APP_RECORD_SOURCE.to_owned(),
                record_id: args.record_id.clone(),
                dry_run: config.output.dry_run,
                file: args
                    .output
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
                valid: false,
                listing_id: None,
                listing_addr: record.listing_addr.clone(),
                seller_account_id: record.owner_account_id.clone(),
                seller_pubkey: record.owner_pubkey.clone(),
                seller_actor_source: None,
                farm_d_tag: record.farm_id.clone(),
                issues: vec![ListingValidationIssueView {
                    field: "local_work_json".to_owned(),
                    message: reason.clone(),
                    line: None,
                }],
                reason: Some(reason),
                actions: vec!["radroots listing app list".to_owned()],
            });
        }
    };
    let output_path = listing_output_path(config, args.output.as_ref(), &draft.listing.d_tag)?;
    validate_listing_output_target(output_path.as_path())?;
    let contents = scaffold_contents(&draft)?;
    let context = validation_context(config)?;
    let issues = app_listing_export_issues(config, &draft, contents.as_str(), &context)?;
    let listing_addr_value = app_record_listing_addr(&draft);

    if !issues.is_empty() {
        return Ok(ListingAppRecordExportView {
            state: "invalid".to_owned(),
            source: LISTING_APP_RECORD_SOURCE.to_owned(),
            record_id: args.record_id.clone(),
            dry_run: config.output.dry_run,
            file: output_path.display().to_string(),
            valid: false,
            listing_id: non_empty(draft.listing.d_tag.clone()),
            listing_addr: listing_addr_value,
            seller_account_id: non_empty(draft.seller_actor.account_id.clone()),
            seller_pubkey: non_empty(draft.seller_actor.pubkey.clone()),
            seller_actor_source: non_empty(draft.seller_actor.source.clone()),
            farm_d_tag: non_empty(draft.listing.farm_d_tag.clone()),
            issues,
            reason: Some(format!(
                "app-authored local record `{}` does not validate as a CLI listing draft",
                args.record_id
            )),
            actions: vec!["radroots listing app list".to_owned()],
        });
    }

    if !config.output.dry_run {
        write_listing_draft(output_path.as_path(), &draft, false)?;
    }

    Ok(ListingAppRecordExportView {
        state: if config.output.dry_run {
            "dry_run"
        } else {
            "exported"
        }
        .to_owned(),
        source: LISTING_APP_RECORD_SOURCE.to_owned(),
        record_id: args.record_id.clone(),
        dry_run: config.output.dry_run,
        file: output_path.display().to_string(),
        valid: true,
        listing_id: Some(draft.listing.d_tag.clone()),
        listing_addr: app_record_listing_addr(&draft),
        seller_account_id: Some(draft.seller_actor.account_id.clone()),
        seller_pubkey: Some(draft.seller_actor.pubkey.clone()),
        seller_actor_source: Some(draft.seller_actor.source.clone()),
        farm_d_tag: Some(draft.listing.farm_d_tag.clone()),
        issues: Vec::new(),
        reason: Some(if config.output.dry_run {
            "dry run requested; listing draft was not written".to_owned()
        } else {
            "app-authored listing record exported as a CLI listing draft".to_owned()
        }),
        actions: vec![
            format!("radroots listing publish {}", output_path.display()),
            format!("radroots listing publish {}", output_path.display()),
        ],
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

    let target_account = account::resolve_account_selector(config, args.selector.as_str())
        .map_err(|error| listing_rebind_selector_error(args.selector.as_str(), error))?;
    let from_seller_account_id = non_empty(draft.seller_actor.account_id.clone());
    let from_seller_pubkey = non_empty(draft.seller_actor.pubkey.clone());
    let from_seller_actor_source = non_empty(draft.seller_actor.source.clone());
    let from_farm_d_tag = non_empty(draft.listing.farm_d_tag.clone());
    let target_account_id = target_account.record.id().to_string();
    let target_pubkey = target_account
        .record
        .public_identity()
        .public_key()
        .to_hex();
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
            vec![format!("radroots account select {}", args.selector)]
        } else {
            vec![format!("radroots listing publish {}", args.file.display())]
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
        RuntimeError::Account(account::AccountRuntimeFailure::Unresolved(issue)) => {
            account::AccountRuntimeFailure::unresolved_with_detail(
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
    format!("{KIND_CLASSIFIED_LISTING}:{seller_pubkey}:{listing_id}")
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
    match to_wire_parts_with_kind(&canonical.listing, KIND_CLASSIFIED_LISTING) {
        Ok(_) => {}
        Err(error) => {
            return vec![ListingValidationIssueView {
                field: "listing".to_owned(),
                message: format!("invalid listing contract: {error}"),
                line: None,
            }];
        }
    }
    match validate_operational_listing_draft(canonical) {
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

#[derive(Debug, Clone)]
struct AppRecordListEntry {
    record: RuntimeStoreRecord,
    superseded_count: usize,
}

fn app_local_records(config: &RuntimeConfig) -> Result<Vec<RuntimeStoreRecord>, RuntimeError> {
    let mut app_records = Vec::new();
    let mut before_cursor = None::<(i64, i64)>;
    loop {
        let shared_records = if let Some((before_change_seq, before_seq)) = before_cursor {
            list_shared_records_before(
                config,
                before_change_seq,
                before_seq,
                APP_RECORD_LIST_LIMIT,
            )?
        } else {
            list_shared_records_latest(config, APP_RECORD_LIST_LIMIT)?
        };
        let Some(next_cursor) = shared_records
            .last()
            .map(|record| (record.change_seq, record.seq))
        else {
            break;
        };
        let has_more = shared_records.len() == APP_RECORD_LIST_LIMIT as usize;
        app_records.extend(
            shared_records
                .into_iter()
                .filter(is_supported_app_local_record),
        );
        if !has_more {
            break;
        }
        before_cursor = Some(next_cursor);
    }
    Ok(app_records)
}

fn is_supported_app_local_record(record: &RuntimeStoreRecord) -> bool {
    record.source_runtime == SourceRuntime::App
        && record.family == RuntimeStoreRecordFamily::LocalWork
        && matches!(
            local_record_kind(record).as_deref(),
            Some("farm_config_v1" | DRAFT_KIND)
        )
}

fn current_app_record_entries(mut records: Vec<RuntimeStoreRecord>) -> Vec<AppRecordListEntry> {
    records.sort_by(|left, right| {
        right
            .change_seq
            .cmp(&left.change_seq)
            .then_with(|| right.seq.cmp(&left.seq))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });

    let mut entries: Vec<AppRecordListEntry> = Vec::new();
    let mut seen = HashMap::<String, usize>::new();
    for record in records {
        let key = app_record_current_key(&record);
        if let Some(index) = seen.get(&key).copied() {
            entries[index].superseded_count += 1;
        } else {
            seen.insert(key, entries.len());
            entries.push(AppRecordListEntry {
                record,
                superseded_count: 0,
            });
        }
    }
    entries
}

fn current_app_record_for(
    config: &RuntimeConfig,
    record: &RuntimeStoreRecord,
) -> Result<Option<RuntimeStoreRecord>, RuntimeError> {
    let key = app_record_current_key(record);
    Ok(app_local_records(config)?
        .into_iter()
        .filter(|candidate| app_record_current_key(candidate) == key)
        .max_by(|left, right| {
            left.change_seq
                .cmp(&right.change_seq)
                .then_with(|| left.seq.cmp(&right.seq))
        }))
}

fn app_record_summary(
    record: &RuntimeStoreRecord,
    superseded_count: usize,
) -> ListingAppRecordSummaryView {
    let record_kind = local_record_kind(record).unwrap_or_else(|| "unknown".to_owned());
    let (listing_id, title, exportable, reason) = match record_kind.as_str() {
        DRAFT_KIND => {
            if let Some(reason) = app_record_exportability_reason(record) {
                let (listing_id, title, _) = app_listing_display_parts(record);
                (listing_id, title, false, Some(reason))
            } else {
                match app_listing_draft_from_record(record) {
                    Ok(draft) => (
                        non_empty(draft.listing.d_tag),
                        non_empty(draft.product.title),
                        true,
                        None,
                    ),
                    Err(reason) => {
                        let (listing_id, title, _) = app_listing_display_parts(record);
                        (listing_id, title, false, Some(reason))
                    }
                }
            }
        }
        "farm_config_v1" => (
            None,
            record
                .local_work_json
                .as_ref()
                .and_then(|payload| payload["document"]["farm"]["name"].as_str())
                .map(str::to_owned),
            false,
            Some("farm records provide defaults; export selects listing records".to_owned()),
        ),
        _ => (
            None,
            None,
            false,
            Some(format!("unsupported app record kind `{record_kind}`")),
        ),
    };
    let actions = if exportable {
        vec![format!("radroots listing app export {}", record.record_id)]
    } else {
        Vec::new()
    };

    ListingAppRecordSummaryView {
        record_id: record.record_id.clone(),
        seq: record.seq,
        change_seq: record.change_seq,
        superseded_count,
        record_kind,
        status: record.status.as_str().to_owned(),
        source_runtime: record.source_runtime.as_str().to_owned(),
        owner_account_id: record.owner_account_id.clone(),
        owner_pubkey: record.owner_pubkey.clone(),
        farm_id: record.farm_id.clone(),
        listing_addr: record.listing_addr.clone(),
        listing_id,
        title,
        exportable,
        reason,
        actions,
    }
}

fn app_record_current_key(record: &RuntimeStoreRecord) -> String {
    match local_record_kind(record).as_deref() {
        Some(DRAFT_KIND) => {
            if let Some(listing_addr) = record
                .listing_addr
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return format!("listing_addr:{listing_addr}");
            }
            let (listing_id, _, _) = app_listing_display_parts(record);
            if let (Some(owner_pubkey), Some(listing_id)) = (
                app_record_canonical_owner_pubkey(record),
                listing_id.filter(|value| is_d_tag_base64url(value)),
            ) {
                return format!("listing_owner:{owner_pubkey}:{listing_id}");
            }
        }
        Some("farm_config_v1") => {
            if let Some(farm_id) = record
                .farm_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return format!("farm:{farm_id}");
            }
            if let Some(farm_id) = record
                .local_work_json
                .as_ref()
                .and_then(|payload| payload["document"]["farm"]["d_tag"].as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return format!("farm:{farm_id}");
            }
        }
        _ => {}
    }
    format!("record:{}", record.record_id)
}

fn canonical_hex_pubkey(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|char| char.is_ascii_hexdigit()) {
        Some(trimmed.to_ascii_lowercase())
    } else {
        None
    }
}

fn app_record_canonical_owner_pubkey(record: &RuntimeStoreRecord) -> Option<String> {
    record
        .owner_pubkey
        .as_deref()
        .and_then(canonical_hex_pubkey)
}

fn app_listing_display_parts(
    record: &RuntimeStoreRecord,
) -> (Option<String>, Option<String>, Option<String>) {
    let document = record
        .local_work_json
        .as_ref()
        .and_then(|payload| payload.get("document"));
    let listing_id = document
        .and_then(|document| document["listing"]["d_tag"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let title = document
        .and_then(|document| document["product"]["title"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let farm_d_tag = document
        .and_then(|document| document["listing"]["farm_d_tag"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    (listing_id, title, farm_d_tag)
}

fn app_record_exportability_reason(record: &RuntimeStoreRecord) -> Option<String> {
    if local_record_kind(record).as_deref() == Some(DRAFT_KIND)
        && app_record_canonical_owner_pubkey(record).is_none()
    {
        return Some(CANONICAL_OWNER_PUBKEY_REQUIRED_REASON.to_owned());
    }
    let exportability = record
        .local_work_json
        .as_ref()
        .and_then(|payload| payload.get("exportability"))?;
    let state = exportability
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if state.is_empty() || state == "exportable" {
        return None;
    }
    let reason = exportability
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(match (state, reason) {
        ("identity_unresolved", "canonical_hex_pubkey_required") => {
            CANONICAL_OWNER_PUBKEY_REQUIRED_REASON.to_owned()
        }
        ("identity_unresolved", _) => "app record identity is unresolved".to_owned(),
        (_, "") => format!("app record exportability state `{state}` is not exportable"),
        (_, reason) => format!("app record exportability state `{state}`: {reason}"),
    })
}

fn app_listing_draft_from_record(
    record: &RuntimeStoreRecord,
) -> Result<ListingDraftDocument, String> {
    if record.source_runtime != SourceRuntime::App {
        return Err(format!(
            "record source_runtime `{}` is not app",
            record.source_runtime.as_str()
        ));
    }
    if record.family != RuntimeStoreRecordFamily::LocalWork {
        return Err(format!(
            "record family `{}` is not local_work",
            record.family.as_str()
        ));
    }
    let payload = record
        .local_work_json
        .as_ref()
        .ok_or_else(|| "record has no local_work_json payload".to_owned())?;
    let record_kind = local_record_kind(record).unwrap_or_else(|| "unknown".to_owned());
    if record_kind != DRAFT_KIND {
        return Err(format!("record kind `{record_kind}` is not {DRAFT_KIND}"));
    }
    if let Some(reason) = app_record_exportability_reason(record) {
        return Err(reason);
    }
    let owner_pubkey = app_record_canonical_owner_pubkey(record)
        .ok_or_else(|| CANONICAL_OWNER_PUBKEY_REQUIRED_REASON.to_owned())?;
    let document = payload
        .get("document")
        .cloned()
        .ok_or_else(|| "record local_work_json.document is missing".to_owned())?;
    let mut draft = serde_json::from_value::<ListingDraftDocument>(document)
        .map_err(|error| format!("record listing document is invalid: {error}"))?;
    if let Some(account_id) = record
        .owner_account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        draft.seller_actor.account_id = account_id.to_owned();
    }
    draft.seller_actor.pubkey = owner_pubkey;
    if draft.listing.farm_d_tag.trim().is_empty()
        && let Some(farm_id) = record.farm_id.as_ref()
    {
        draft.listing.farm_d_tag = farm_id.clone();
    }
    normalize_app_listing_availability(&mut draft)?;
    normalize_app_listing_units(&mut draft);
    Ok(draft)
}

fn normalize_app_listing_availability(draft: &mut ListingDraftDocument) -> Result<(), String> {
    let kind = draft.availability.kind.trim();
    if kind.is_empty() || kind == "local" {
        draft.availability.kind = "status".to_owned();
    } else if !matches!(kind, "status" | "window") {
        return Err(format!(
            "unsupported app listing availability kind `{kind}`"
        ));
    }
    if draft.availability.kind == "window" {
        return Ok(());
    }

    let status = draft.availability.status.trim();
    draft.availability.status = match status {
        "" | "active" | "draft" | "published" => "active".to_owned(),
        "archived" | "paused" | "sold" => {
            return Err(format!(
                "app listing status `{status}` is not exportable as a publishable CLI draft"
            ));
        }
        other => return Err(format!("unsupported app listing status `{other}`")),
    };
    Ok(())
}

fn normalize_app_listing_units(draft: &mut ListingDraftDocument) {
    let quantity_unit = draft.primary_bin.quantity_unit.trim().to_owned();
    let price_per_unit = draft.primary_bin.price_per_unit.trim().to_owned();
    let quantity_unit_supported = quantity_unit.parse::<Unit>().is_ok();
    let price_per_unit_supported = price_per_unit.parse::<Unit>().is_ok();
    if quantity_unit_supported && price_per_unit_supported {
        return;
    }

    if draft.primary_bin.label.trim().is_empty() {
        draft.primary_bin.label = if !quantity_unit_supported && !quantity_unit.is_empty() {
            quantity_unit.clone()
        } else {
            price_per_unit.clone()
        };
    }
    if !quantity_unit_supported {
        draft.primary_bin.quantity_unit = "each".to_owned();
    }
    if !price_per_unit_supported {
        draft.primary_bin.price_per_unit = "each".to_owned();
    }
}

fn app_listing_export_issues(
    config: &RuntimeConfig,
    draft: &ListingDraftDocument,
    contents: &str,
    context: &ListingValidationContext,
) -> Result<Vec<ListingValidationIssueView>, RuntimeError> {
    let canonical = match canonicalize_draft(draft, contents, context) {
        Ok(canonical) => canonical,
        Err(error) => return Ok(vec![error.into_issue()]),
    };
    let mut issues = listing_ready_issues(&canonical, contents);
    if let Some(issue) = listing_bound_account_issue(config, &canonical, contents)? {
        issues.push(issue);
    }
    Ok(issues)
}

fn app_record_listing_addr(draft: &ListingDraftDocument) -> Option<String> {
    let seller_pubkey = draft.seller_actor.pubkey.trim();
    let listing_id = draft.listing.d_tag.trim();
    if seller_pubkey.is_empty() || listing_id.is_empty() {
        None
    } else {
        Some(listing_addr(seller_pubkey, listing_id))
    }
}

fn local_record_kind(record: &RuntimeStoreRecord) -> Option<String> {
    record
        .local_work_json
        .as_ref()
        .and_then(|payload| payload.get("record_kind"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn get(
    config: &RuntimeConfig,
    args: &RecordLookupArgs,
) -> Result<ListingGetView, RuntimeError> {
    refresh_market_listing_if_needed(config)?;
    let freshness = if config.local.replica_store_path.exists() {
        let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
        freshness_for_scope_from_executor(config, &executor, RelayIngestScope::MarketPull)?
    } else {
        missing_freshness()
    };
    let provenance = FindResultProvenanceView {
        origin: "local_replica.trade_product".to_owned(),
        freshness: freshness.display.clone(),
        relay_count: config.transport.nostr_relay_urls.len(),
    };

    if !config.local.replica_store_path.exists() {
        return Ok(ListingGetView {
            state: "unconfigured".to_owned(),
            source: LISTING_READ_SOURCE.to_owned(),
            lookup: args.key.clone(),
            readiness: MarketReadinessView::unavailable("local_replica_not_initialized"),
            listing_id: None,
            product_key: None,
            listing_addr: None,
            primary_bin_id: None,
            title: None,
            category: None,
            description: None,
            location_primary: None,
            available: None,
            price: None,
            provenance,
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store inspect".to_owned()],
        });
    }

    let db = ReplicaSql::new(SqlxSqliteExecutor::open(&config.local.replica_store_path)?);
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
            primary_bin_id: None,
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
                format!("radroots market search {}", args.key),
            ],
        });
    };

    let listing_addr = row.listing_addr.and_then(non_empty);
    let primary_bin_id = row.primary_bin_id.and_then(non_empty);
    let verified_primary_bin_id = row.verified_primary_bin_id.and_then(non_empty);
    let available_amount = row.qty_avail;
    let price_amount = row.price_amt;
    let price_currency = row.price_currency;
    let price_per_amount = row.price_qty_amt;
    let readiness = MarketReadinessView::from_market_projection(
        listing_addr.as_deref(),
        primary_bin_id.as_deref(),
        verified_primary_bin_id.as_deref(),
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
        primary_bin_id,
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
    if !config.local.replica_store_path.exists()
        || config.output.dry_run
        || config.transport.nostr_relay_urls.is_empty()
    {
        return Ok(());
    }
    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path)?;
    let freshness =
        freshness_for_scope_from_executor(config, &executor, RelayIngestScope::MarketPull)?;
    if crate::runtime::sync::freshness_requires_refresh(&freshness) {
        let _ = market_refresh(config)?;
    }
    Ok(())
}

pub fn publish_via_sdk(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, CliSdkAdapterError> {
    let input = sdk_listing_publish_input(config, args)?;
    validate_configured_listing_signer(config, &input.canonical)?;
    let plan = sdk_listing::prepare(sdk_listing::PrepareRequest::publish(
        input.actor,
        input.document,
        sdk_created_at_unix()?,
    ))
    .map_err(|error| RuntimeError::Config(format!("invalid SDK listing plan: {error}")))?;
    if !config.output.dry_run {
        return Err(RuntimeError::Config(
            "listing commit is unavailable until the shared sync engine is configured".to_owned(),
        )
        .into());
    }
    Ok(sdk_prepared_publish_view(
        config,
        args,
        ListingMutationOperation::Publish,
        &input.canonical,
        plan,
    ))
}

fn sdk_listing_publish_input(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<SdkListingPublishInput, RuntimeError> {
    let contents = fs::read_to_string(&args.file)?;
    let parsed = toml::from_str::<ListingDraftDocument>(&contents).map_err(|error| {
        RuntimeError::Config(format!(
            "invalid listing draft {}: {error}",
            args.file.display()
        ))
    })?;
    let context = mutation_validation_context(config)?;
    let canonical = canonicalize_draft(&parsed, &contents, &context).map_err(|error| {
        let issue = match error {
            ListingDraftValidationError::MissingSellerAccount(issue) => {
                return account::AccountRuntimeFailure::unresolved_with_detail(
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
    let actor = Actor::from_public_key_hex(
        canonical.seller_pubkey.as_str(),
        ActorSource::ExplicitPublicKey,
        [AuthorRole::Seller],
    )
    .map_err(|error| RuntimeError::Config(format!("invalid listing SDK actor: {error}")))?;
    let document = RadrootsOperationalListingEditDocumentV1::new(canonical.listing.clone());
    Ok(SdkListingPublishInput {
        canonical,
        actor,
        document,
    })
}

fn sdk_prepared_publish_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    plan: ListingPlan,
) -> ListingMutationView {
    let listing_addr = plan.address().as_str().to_owned();
    let event = sdk_plan_event_view(&plan);
    ListingMutationView {
        state: "dry_run".to_owned(),
        operation: operation.as_str().to_owned(),
        source: SDK_LISTING_WRITE_SOURCE.to_owned(),
        file: args.file.display().to_string(),
        listing_id: canonical.listing_id.clone(),
        listing_addr: listing_addr.clone(),
        seller_account_id: canonical.seller_account_id.clone(),
        seller_pubkey: canonical.seller_pubkey.clone(),
        seller_actor_source: canonical.seller_actor_source.clone(),
        event_kind: KIND_CLASSIFIED_LISTING,
        dry_run: true,
        deduplicated: false,
        target_transport_endpoints: Vec::new(),
        attempted_transport_endpoints: Vec::new(),
        accepted_transport_endpoints: Vec::new(),
        failed_transport_targets: Vec::new(),
        job_id: None,
        job_status: None,
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        event_id: Some(plan.draft().expected_event_id().to_string()),
        event_addr: Some(listing_addr),
        idempotency_key: args.idempotency_key.clone(),
        local_replica: None,
        reason: Some("dry run requested; SDK enqueue and transport push skipped".to_owned()),
        job: None,
        event: args.print_event.then_some(event),
        actions: vec![format!("radroots listing publish {}", args.file.display())],
    }
}

fn sdk_plan_event_view(plan: &ListingPlan) -> ListingMutationEventView {
    ListingMutationEventView {
        kind: plan.draft().kind_u32(),
        author: plan.draft().expected_pubkey().to_hex(),
        created_at: Some(plan.draft().created_at_u64()),
        content: plan.draft().content().to_owned(),
        tags: plan.draft().tags_as_vec(),
        event_id: Some(plan.draft().expected_event_id().to_string()),
        signature: None,
        event_addr: plan.address().as_str().to_owned(),
    }
}

pub fn update(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, CliSdkAdapterError> {
    mutate(config, args, ListingMutationOperation::Update)
}

pub fn pause(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, CliSdkAdapterError> {
    mutate(config, args, ListingMutationOperation::Pause)
}

pub fn withdraw(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<ListingMutationView, CliSdkAdapterError> {
    mutate(config, args, ListingMutationOperation::Withdraw)
}

fn mutate(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
) -> Result<ListingMutationView, CliSdkAdapterError> {
    let contents = fs::read_to_string(&args.file).map_err(RuntimeError::from)?;
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
                return account::AccountRuntimeFailure::unresolved_with_detail(
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

    if let Some(status) = operation.listing_status() {
        canonical.listing.availability = Some(OperationalListingAvailability::Status {
            status: OperationalListingStatus::Other {
                value: status.to_owned(),
            },
        });
    }

    if config.output.dry_run {
        validate_configured_listing_signer(config, &canonical)?;
    }

    mutate_via_sdk_from_canonical(config, args, operation, canonical)
}

fn mutate_via_sdk_from_canonical(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: CanonicalListingDraft,
) -> Result<ListingMutationView, CliSdkAdapterError> {
    let actor = Actor::from_public_key_hex(
        canonical.seller_pubkey.as_str(),
        ActorSource::ExplicitPublicKey,
        [AuthorRole::Seller],
    )
    .map_err(|error| RuntimeError::Config(format!("invalid listing SDK actor: {error}")))?;
    let document = RadrootsOperationalListingEditDocumentV1::new(canonical.listing.clone());
    let request = if matches!(operation, ListingMutationOperation::Publish) {
        sdk_listing::PrepareRequest::publish(actor, document, sdk_created_at_unix()?)
    } else {
        sdk_listing::PrepareRequest::update(actor, document, sdk_created_at_unix()?)
    };
    let plan = sdk_listing::prepare(request)
        .map_err(|error| RuntimeError::Config(format!("invalid SDK listing plan: {error}")))?;
    if !config.output.dry_run {
        return Err(RuntimeError::Config(
            "listing commit is unavailable until the shared sync engine is configured".to_owned(),
        )
        .into());
    }
    Ok(sdk_prepared_publish_view(
        config, args, operation, &canonical, plan,
    ))
}

fn sdk_created_at_unix() -> Result<u64, RuntimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| RuntimeError::Config(format!("system clock error: {error}")))
}

fn scaffold_contents(draft: &ListingDraftDocument) -> Result<String, RuntimeError> {
    let toml = toml::to_string_pretty(draft).map_err(|error| {
        RuntimeError::Config(format!("failed to render listing draft: {error}"))
    })?;
    Ok(format!(
        "# radroots listing draft v1\n# this scaffold applies selected farm defaults and provided product inputs when available\n# review any remaining empty fields, then run `radroots listing publish <file>`\n\n{toml}"
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
    validation_context(config)
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
    let quantity = Quantity::try_new(quantity_amount, quantity_unit)
        .map_err(|error| {
            issue_for_field(
                contents,
                "primary_bin.quantity_amount",
                format!("invalid primary_bin quantity: {error}"),
            )
        })?
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
    let price_money = Money::try_new(price_amount, price_currency).map_err(|error| {
        issue_for_field(contents, "primary_bin.price_amount", error.to_string())
    })?;
    let price_quantity = Quantity::try_new(price_per_amount, price_per_unit).map_err(|error| {
        issue_for_field(contents, "primary_bin.price_per_amount", error.to_string())
    })?;
    let price = QuantityPrice::try_new(price_money, price_quantity)
        .map_err(|error| issue_for_field(contents, "primary_bin.price_amount", error.to_string()))?
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
    let primary_bin_id =
        protocol_inventory_bin_id(draft.primary_bin.bin_id.trim(), "primary_bin.bin_id").map_err(
            |error| {
                issue_for_field(
                    contents,
                    "primary_bin.bin_id",
                    format!("invalid primary_bin bin id: {error}"),
                )
            },
        )?;

    let listing = OperationalListing {
        d_tag: protocol_d_tag(listing_id.as_str(), "listing d_tag").map_err(|error| {
            issue_for_field(
                contents,
                "listing.d_tag",
                format!("invalid listing d_tag: {error}"),
            )
        })?,
        published_at: None,
        farm: FarmRef {
            pubkey: seller_pubkey.clone(),
            d_tag: farm_d_tag.clone(),
        },
        product: OperationalListingProduct {
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
        primary_bin_id: primary_bin_id.clone(),
        bins: vec![OperationalListingBin {
            bin_id: primary_bin_id,
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
) -> Result<OperationalListingAvailability, ListingValidationIssueView> {
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
            Ok(OperationalListingAvailability::Status {
                status: match status {
                    "active" => OperationalListingStatus::Active,
                    "sold" => OperationalListingStatus::Sold,
                    other => OperationalListingStatus::Other {
                        value: other.to_owned(),
                    },
                },
            })
        }
        "window" => Ok(OperationalListingAvailability::Window {
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
) -> Result<OperationalListingDeliveryMethod, ListingValidationIssueView> {
    let method = draft.delivery.method.trim();
    if method.is_empty() {
        return Err(issue_for_field(
            contents,
            "delivery.method",
            "missing delivery method",
        ));
    }

    Ok(match method {
        "pickup" => OperationalListingDeliveryMethod::Pickup,
        "local_delivery" => OperationalListingDeliveryMethod::LocalDelivery,
        "shipping" => OperationalListingDeliveryMethod::Shipping,
        other => OperationalListingDeliveryMethod::Other {
            method: other.to_owned(),
        },
    })
}

fn build_location(draft: &ListingDraftDocument) -> OperationalListingPublicLocation {
    OperationalListingPublicLocation {
        primary: draft.location.primary.trim().to_owned(),
        city: draft.location.city.clone().and_then(non_empty),
        region: draft.location.region.clone().and_then(non_empty),
        country: draft.location.country.clone().and_then(non_empty),
        geohash: draft.location.geohash.trim().to_owned(),
    }
}

fn build_listing_discounts(
    draft: &ListingDraftDocument,
    contents: &str,
    primary_bin_id: &str,
    price_currency: Currency,
) -> Result<Option<Vec<Discount>>, ListingValidationIssueView> {
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
                let percent = raw.parse::<Percent>().map_err(|error| {
                    issue_for_field(
                        contents,
                        field_prefix.as_str(),
                        format!("percent discount value is invalid: {error}"),
                    )
                })?;
                DiscountValue::Percent(percent)
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
                DiscountValue::MoneyPerBin(Money::try_new(amount, currency).map_err(|error| {
                    issue_for_field(contents, field_prefix.as_str(), error.to_string())
                })?)
            }
            other => {
                return Err(issue_for_field(
                    contents,
                    field_prefix.as_str(),
                    format!("unsupported discount kind `{other}`"),
                ));
            }
        };
        let discount = Discount::try_new(
            DiscountScope::Bin,
            DiscountThreshold::BinCount { bin_id, min },
            value,
        )
        .map_err(|error| issue_for_field(contents, field_prefix.as_str(), error.to_string()))?;
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
    let account_pubkey = account.record.public_identity().public_key().to_hex();
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
        return Err(account::AccountRuntimeFailure::unresolved_with_detail(
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
    let account_pubkey = account.record.public_identity().public_key().to_hex();
    if !account_pubkey.eq_ignore_ascii_case(canonical.seller_pubkey.as_str()) {
        return Err(account::AccountRuntimeFailure::mismatch_with_detail(
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
    let attempted = account::resolve_account_selector(config, selector)?;
    if attempted.record.id().to_string() == canonical.seller_account_id {
        return Ok(());
    }
    Err(account::AccountRuntimeFailure::mismatch_with_detail(
        format!(
            "account mismatch: listing draft is bound to seller account `{}`; invocation selected `{}`",
            canonical.seller_account_id, attempted.record.id()
        ),
        json!({
            "seller_actor_source": canonical.seller_actor_source,
            "listing_seller_account_id": canonical.seller_account_id,
            "attempted_seller_account_id": attempted.record.id().to_string(),
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

fn validate_configured_listing_signer(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
) -> Result<(), RuntimeError> {
    validate_configured_signer_for_actor(
        config,
        Some(canonical.seller_account_id.as_str()),
        canonical.seller_pubkey.as_str(),
        "listing seller",
    )
}

fn validate_operational_listing_draft(
    canonical: &CanonicalListingDraft,
) -> Result<(), OperationalListingValidationError> {
    let seller_pubkey = PublicKey::from_hex(canonical.seller_pubkey.as_str())
        .map_err(|_| OperationalListingValidationError::InvalidSeller)?;
    validate_operational_listing_model(canonical.listing.clone(), &seller_pubkey).map(|_| ())
}

fn issue_from_trade_validation(
    error: OperationalListingValidationError,
    contents: &str,
) -> ListingValidationIssueView {
    match error {
        OperationalListingValidationError::InvalidSeller => issue_for_field(
            contents,
            "seller_actor.pubkey",
            "listing author does not match the farm pubkey",
        ),
        OperationalListingValidationError::MissingTitle => {
            issue_for_field(contents, "product.title", "missing listing title")
        }
        OperationalListingValidationError::MissingDescription => {
            issue_for_field(contents, "product.summary", "missing listing description")
        }
        OperationalListingValidationError::MissingProductType => {
            issue_for_field(contents, "product.category", "missing listing product type")
        }
        OperationalListingValidationError::MissingBins
        | OperationalListingValidationError::MissingPrimaryBin
        | OperationalListingValidationError::InvalidBin => {
            issue_for_field(contents, "primary_bin.bin_id", error.to_string())
        }
        OperationalListingValidationError::MissingPrice
        | OperationalListingValidationError::InvalidPrice => issue_for_field(
            contents,
            "primary_bin.price_amount",
            "invalid listing price",
        ),
        OperationalListingValidationError::MissingInventory
        | OperationalListingValidationError::InvalidInventory => {
            issue_for_field(contents, "inventory.available", error.to_string())
        }
        OperationalListingValidationError::MissingAvailability => issue_for_field(
            contents,
            "availability.status",
            "missing listing availability",
        ),
        OperationalListingValidationError::MissingLocation
        | OperationalListingValidationError::MissingLocationLocality => {
            issue_for_field(contents, "location.primary", error.to_string())
        }
        OperationalListingValidationError::MissingLocationGeohash
        | OperationalListingValidationError::InvalidLocationGeohash => {
            issue_for_field(contents, "location.geohash", error.to_string())
        }
        OperationalListingValidationError::MissingDeliveryMethod => issue_for_field(
            contents,
            "delivery.method",
            "missing listing delivery method",
        ),
        other => issue_for_field(contents, "listing", other.to_string()),
    }
}

fn authoring_defaults(config: &RuntimeConfig) -> Result<ListingAuthoringDefaults, RuntimeError> {
    let account_resolution = account::resolve_account_resolution(config)?;
    let Some(selected_account) = account_resolution.resolved_account.clone() else {
        return Err(account::AccountRuntimeFailure::unresolved_with_detail(
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
        seller_account_id: selected_account.record.id().to_string(),
        seller_pubkey: selected_account
            .record
            .public_identity()
            .public_key()
            .to_hex(),
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
        return Err(account::AccountRuntimeFailure::unresolved_with_detail(
            format!(
                "farm-bound seller account `{account_id}` is not present in the local account store"
            ),
            json!({
                "seller_actor_source": "farm_config",
                "farm_bound_seller_account_id": account_id,
                "actions": crate::runtime::farm::farm_bound_seller_recovery_actions("<selector>"),
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
    defaults.seller_pubkey = account.record.public_identity().public_key().to_hex();
    defaults.seller_actor_source = LISTING_SELLER_ACTOR_SOURCE_FARM_CONFIG.to_owned();
    defaults.selected_farm_d_tag = Some(resolved.document.selection.farm_d_tag.clone());
    let draft_missing = farm_config::missing_fields(&resolved.document);
    defaults.farm_defaults_ready = !draft_missing.iter().any(|field| {
        matches!(
            field,
            farm_config::FarmMissingField::Location
                | farm_config::FarmMissingField::City
                | farm_config::FarmMissingField::Delivery
                | farm_config::FarmMissingField::Geohash
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
        defaults.farm_next_action = Some("radroots farm get".to_owned());
        defaults.farm_reason = Some(
            "selected farm draft is missing delivery or location defaults; those fields were left blank"
                .to_owned(),
        );
    }
    Ok(defaults)
}

fn draft_location_from_model(location: &OperationalListingPublicLocation) -> ListingDraftLocation {
    ListingDraftLocation {
        primary: location.primary.clone(),
        city: location.city.clone(),
        region: location.region.clone(),
        country: location.country.clone(),
        geohash: location.geohash.clone(),
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
) -> Result<Option<account::AccountRecordView>, RuntimeError> {
    let snapshot = account::snapshot(config)?;
    Ok(snapshot
        .accounts
        .into_iter()
        .find(|account| account.record.id().to_hex() == account_id))
}

fn parse_decimal_field(
    value: &str,
    contents: &str,
    field: &str,
) -> Result<Decimal, ListingValidationIssueView> {
    value.trim().parse::<Decimal>().map_err(|_| {
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
) -> Result<Unit, ListingValidationIssueView> {
    value.parse::<Unit>().map_err(|_| {
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
) -> Result<Currency, ListingValidationIssueView> {
    let upper = value.trim().to_ascii_uppercase();
    Currency::from_str_upper(&upper).map_err(|_| {
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
    use super::{DRAFT_KIND, ListingDraftDocument, encode_base64url_no_pad, generate_d_tag};
    use radroots_event_codec::d_tag::is_d_tag_base64url;

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
                geohash: "dnqwy".to_owned(),
            },
            discounts: Vec::new(),
        };
        let rendered = toml::to_string_pretty(&document).expect("render draft");
        assert!(rendered.contains("kind = \"listing_draft_v1\""));
    }

    #[test]
    fn listing_draft_canonicalization_preserves_discounts_and_validates_semantics() {
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
                primary: "Farm stand".to_owned(),
                city: Some("Asheville".to_owned()),
                region: None,
                country: None,
                geohash: "dnqwy".to_owned(),
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
        super::validate_operational_listing_draft(&canonical)
            .expect("canonical listing passes operational validation");

        let mut missing_description = canonical;
        missing_description.listing.product.summary = Some(" ".to_owned());
        assert_eq!(
            super::validate_operational_listing_draft(&missing_description),
            Err(super::OperationalListingValidationError::MissingDescription)
        );
    }
}
