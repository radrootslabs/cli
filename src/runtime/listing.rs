use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_core::{
    RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreMoney, RadrootsCoreQuantity,
    RadrootsCoreQuantityPrice, RadrootsCoreUnit,
};
use radroots_events::RadrootsNostrEvent;
use radroots_events::kinds::{KIND_LISTING, KIND_LISTING_DRAFT};
use radroots_events::listing::{
    RadrootsListing, RadrootsListingAvailability, RadrootsListingBin,
    RadrootsListingDeliveryMethod, RadrootsListingFarmRef, RadrootsListingLocation,
    RadrootsListingProduct, RadrootsListingStatus,
};
use radroots_events::trade::RadrootsTradeListingValidationError;
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::listing::encode::to_wire_parts_with_kind;
use radroots_sql_core::{SqlExecutor, SqliteExecutor, utils};
use radroots_trade::listing::validation::validate_listing_event;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::{ListingFileArgs, ListingMutationArgs, ListingNewArgs, RecordKeyArgs};
use crate::domain::runtime::{
    FindPriceView, FindQuantityView, FindResultProvenanceView, ListingGetView,
    ListingMutationEventView, ListingMutationJobView, ListingMutationView, ListingNewView,
    ListingValidateView, ListingValidationIssueView, SyncFreshnessView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::daemon;
use crate::runtime::daemon::DaemonRpcError;
use crate::runtime::sync::freshness_from_executor;

const DRAFT_KIND: &str = "listing_draft_v1";
const LISTING_SOURCE: &str = "local draft · local first";
const LISTING_READ_SOURCE: &str = "local replica · local first";
const LISTING_WRITE_SOURCE: &str = "daemon bridge · durable write plane";

static D_TAG_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftDocument {
    version: u32,
    kind: String,
    listing: ListingDraftMeta,
    product: ListingDraftProduct,
    primary_bin: ListingDraftPrimaryBin,
    inventory: ListingDraftInventory,
    availability: ListingDraftAvailability,
    delivery: ListingDraftDelivery,
    location: ListingDraftLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingDraftMeta {
    d_tag: String,
    farm_d_tag: String,
    seller_pubkey: String,
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

#[derive(Debug, Clone)]
struct ListingValidationContext {
    selected_account_id: Option<String>,
    selected_account_pubkey: Option<String>,
    selected_farm_d_tag: Option<String>,
}

#[derive(Debug, Clone)]
struct CanonicalListingDraft {
    listing_id: String,
    seller_pubkey: String,
    farm_d_tag: String,
    listing: RadrootsListing,
}

#[derive(Debug, Clone, Deserialize)]
struct ListingRow {
    id: String,
    key: String,
    category: String,
    title: String,
    summary: String,
    qty_amt: i64,
    qty_unit: String,
    qty_label: Option<String>,
    qty_avail: Option<i64>,
    price_amt: f64,
    price_currency: String,
    price_qty_amt: u32,
    price_qty_unit: String,
    location_primary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FarmRow {
    d_tag: String,
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
    args: &ListingNewArgs,
) -> Result<ListingNewView, RuntimeError> {
    let selected_account = accounts::resolve_account(config)?;
    let seller_pubkey = selected_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.clone());
    let farm_d_tag = match seller_pubkey.as_deref() {
        Some(pubkey) => resolve_selected_farm_d_tag(config, pubkey)?,
        None => None,
    };

    let draft = ListingDraftDocument {
        version: 1,
        kind: DRAFT_KIND.to_owned(),
        listing: ListingDraftMeta {
            d_tag: generate_d_tag(),
            farm_d_tag: farm_d_tag.clone().unwrap_or_default(),
            seller_pubkey: seller_pubkey.clone().unwrap_or_default(),
        },
        product: ListingDraftProduct {
            key: String::new(),
            title: String::new(),
            category: String::new(),
            summary: String::new(),
        },
        primary_bin: ListingDraftPrimaryBin {
            bin_id: "bin-1".to_owned(),
            quantity_amount: "1000".to_owned(),
            quantity_unit: "g".to_owned(),
            price_amount: "0.01".to_owned(),
            price_currency: "USD".to_owned(),
            price_per_amount: "1".to_owned(),
            price_per_unit: "g".to_owned(),
            label: String::new(),
        },
        inventory: ListingDraftInventory {
            available: "1".to_owned(),
        },
        availability: ListingDraftAvailability {
            kind: "status".to_owned(),
            status: "active".to_owned(),
            start: None,
            end: None,
        },
        delivery: ListingDraftDelivery {
            method: "pickup".to_owned(),
        },
        location: ListingDraftLocation {
            primary: String::new(),
            city: None,
            region: None,
            country: None,
        },
    };

    let output_path = match &args.output {
        Some(path) => path.clone(),
        None => std::env::current_dir()?.join(format!("listing-{}.toml", draft.listing.d_tag)),
    };
    if output_path.exists() {
        return Err(RuntimeError::Config(format!(
            "listing draft output {} already exists",
            output_path.display()
        )));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, scaffold_contents(&draft)?)?;

    let mut actions = vec![format!(
        "radroots listing validate {}",
        output_path.display()
    )];
    if seller_pubkey.is_none() {
        actions.push("radroots account new".to_owned());
    }
    if farm_d_tag.is_none() {
        actions.push("radroots sync status".to_owned());
    }

    Ok(ListingNewView {
        state: "draft created".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: output_path.display().to_string(),
        listing_id: draft.listing.d_tag,
        selected_account_id: selected_account.map(|account| account.record.account_id.to_string()),
        seller_pubkey,
        farm_d_tag,
        actions,
    })
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
                seller_pubkey: context.selected_account_pubkey.clone(),
                farm_d_tag: context.selected_farm_d_tag.clone(),
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
                        parsed.listing.d_tag.as_str(),
                        &context,
                        ListingValidationIssueView {
                            field: "listing".to_owned(),
                            message: format!("invalid listing contract: {error}"),
                            line: None,
                        },
                    ));
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
                Ok(_) => Ok(ListingValidateView {
                    state: "valid".to_owned(),
                    source: LISTING_SOURCE.to_owned(),
                    file: args.file.display().to_string(),
                    valid: true,
                    listing_id: Some(canonical.listing_id),
                    seller_pubkey: Some(canonical.seller_pubkey),
                    farm_d_tag: Some(canonical.farm_d_tag),
                    issues: Vec::new(),
                    actions: vec![format!("radroots listing publish {}", args.file.display())],
                }),
                Err(error) => Ok(invalid_validation_view(
                    args.file.as_path(),
                    parsed.listing.d_tag.as_str(),
                    &context,
                    issue_from_trade_validation(error, &contents),
                )),
            }
        }
        Err(issue) => Ok(invalid_validation_view(
            args.file.as_path(),
            parsed.listing.d_tag.as_str(),
            &context,
            issue,
        )),
    }
}

pub fn get(config: &RuntimeConfig, args: &RecordKeyArgs) -> Result<ListingGetView, RuntimeError> {
    let freshness = if config.local.replica_db_path.exists() {
        let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
        freshness_from_executor(&executor)?
    } else {
        SyncFreshnessView {
            state: "never".to_owned(),
            display: "never synced".to_owned(),
            age_seconds: None,
            last_event_at: None,
        }
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
            listing_id: None,
            product_key: None,
            title: None,
            category: None,
            description: None,
            location_primary: None,
            available: None,
            price: None,
            provenance,
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots local init".to_owned()],
        });
    }

    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let rows = query_listing_rows(&executor, args.key.as_str())?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(ListingGetView {
            state: "missing".to_owned(),
            source: LISTING_READ_SOURCE.to_owned(),
            lookup: args.key.clone(),
            listing_id: None,
            product_key: None,
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
                format!("radroots find {}", args.key),
            ],
        });
    };

    Ok(ListingGetView {
        state: "ready".to_owned(),
        source: LISTING_READ_SOURCE.to_owned(),
        lookup: args.key.clone(),
        listing_id: Some(row.id),
        product_key: Some(row.key),
        title: Some(row.title),
        category: Some(row.category),
        description: non_empty(row.summary),
        location_primary: row.location_primary.and_then(non_empty),
        available: Some(FindQuantityView {
            total_amount: row.qty_amt,
            total_unit: row.qty_unit,
            label: row.qty_label.and_then(non_empty),
            available_amount: row.qty_avail,
        }),
        price: Some(FindPriceView {
            amount: row.price_amt,
            currency: row.price_currency,
            per_amount: row.price_qty_amt,
            per_unit: row.price_qty_unit,
        }),
        provenance,
        reason: None,
        actions: Vec::new(),
    })
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
    let context = validation_context(config)?;
    let mut canonical = canonicalize_draft(&parsed, &contents, &context).map_err(|issue| {
        RuntimeError::Config(format!(
            "invalid listing draft {}: {} ({})",
            args.file.display(),
            issue.message,
            issue.field
        ))
    })?;

    if matches!(operation, ListingMutationOperation::Archive) {
        canonical.listing.availability = Some(RadrootsListingAvailability::Status {
            status: RadrootsListingStatus::Other {
                value: "archived".to_owned(),
            },
        });
    }

    let (event_preview, listing_addr) = build_listing_event_preview(&canonical)?;

    if config.output.dry_run {
        return Ok(ListingMutationView {
            state: "dry_run".to_owned(),
            operation: operation.as_str().to_owned(),
            source: LISTING_WRITE_SOURCE.to_owned(),
            file: args.file.display().to_string(),
            listing_id: canonical.listing_id.clone(),
            listing_addr: listing_addr.clone(),
            seller_pubkey: canonical.seller_pubkey.clone(),
            event_kind: KIND_LISTING,
            dry_run: true,
            deduplicated: false,
            job_id: None,
            job_status: None,
            signer_mode: None,
            event_id: None,
            event_addr: Some(listing_addr.clone()),
            idempotency_key: args.idempotency_key.clone(),
            signer_session_id: None,
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some("dry run requested; daemon publish skipped".to_owned()),
            job: args.print_job.then(|| ListingMutationJobView {
                rpc_method: "bridge.listing.publish".to_owned(),
                state: "not_submitted".to_owned(),
                job_id: None,
                idempotency_key: args.idempotency_key.clone(),
                requested_signer_session_id: args.signer_session_id.clone(),
                signer_mode: Some(config.signer.backend.as_str().to_owned()),
                signer_session_id: None,
            }),
            event: args.print_event.then_some(event_preview),
            actions: vec![format!(
                "radroots listing {} {}",
                operation.as_str(),
                args.file.display()
            )],
        });
    }

    let signer_session_id = match daemon::resolve_signer_session_id(
        config,
        "seller",
        canonical.seller_pubkey.as_str(),
        KIND_LISTING,
        args.signer_session_id.as_deref(),
    ) {
        Ok(session_id) => session_id,
        Err(error) => {
            return Ok(daemon_error_view(
                config,
                args,
                operation,
                &canonical,
                listing_addr,
                event_preview,
                error,
            ));
        }
    };

    match daemon::bridge_listing_publish(
        config,
        &canonical.listing,
        KIND_LISTING,
        args.idempotency_key.as_deref(),
        Some(signer_session_id.as_str()),
    ) {
        Ok(result) => {
            let failed = result.status == "failed";
            let mut actions = Vec::new();
            if failed {
                if let Some(job_id) = &Some(result.job_id.clone()) {
                    actions.push(format!("radroots job get {job_id}"));
                }
                actions.push("radroots rpc status".to_owned());
            } else {
                actions.push(format!("radroots job get {}", result.job_id));
                actions.push(format!("radroots job watch {}", result.job_id));
            }

            Ok(ListingMutationView {
                state: if failed {
                    "unavailable".to_owned()
                } else if result.deduplicated {
                    "deduplicated".to_owned()
                } else {
                    result.status.clone()
                },
                operation: operation.as_str().to_owned(),
                source: LISTING_WRITE_SOURCE.to_owned(),
                file: args.file.display().to_string(),
                listing_id: canonical.listing_id,
                listing_addr: listing_addr.clone(),
                seller_pubkey: canonical.seller_pubkey.clone(),
                event_kind: result.event_kind.unwrap_or(KIND_LISTING),
                dry_run: false,
                deduplicated: result.deduplicated,
                job_id: Some(result.job_id.clone()),
                job_status: Some(result.status.clone()),
                signer_mode: Some(result.signer_mode.clone()),
                signer_session_id: result.signer_session_id.clone(),
                event_id: result.event_id.clone(),
                event_addr: result
                    .event_addr
                    .clone()
                    .or_else(|| Some(listing_addr.clone())),
                idempotency_key: result.idempotency_key.clone(),
                requested_signer_session_id: args.signer_session_id.clone(),
                reason: failed.then(|| {
                    "daemon publish job failed before relay delivery completed".to_owned()
                }),
                job: args.print_job.then(|| ListingMutationJobView {
                    rpc_method: "bridge.listing.publish".to_owned(),
                    state: result.status,
                    job_id: Some(result.job_id),
                    idempotency_key: result.idempotency_key,
                    requested_signer_session_id: args.signer_session_id.clone(),
                    signer_mode: Some(result.signer_mode),
                    signer_session_id: result.signer_session_id,
                }),
                event: args.print_event.then(|| ListingMutationEventView {
                    event_id: result.event_id,
                    event_addr: result.event_addr.unwrap_or(listing_addr),
                    ..event_preview
                }),
                actions,
            })
        }
        Err(error) => Ok(daemon_error_view(
            config,
            args,
            operation,
            &canonical,
            listing_addr,
            event_preview,
            error,
        )),
    }
}

fn scaffold_contents(draft: &ListingDraftDocument) -> Result<String, RuntimeError> {
    let toml = toml::to_string_pretty(draft).map_err(|error| {
        RuntimeError::Config(format!("failed to render listing draft: {error}"))
    })?;
    Ok(format!(
        "# radroots listing draft v1\n# fill the empty fields, then run `radroots listing validate <file>`\n\n{toml}"
    ))
}

fn validation_context(config: &RuntimeConfig) -> Result<ListingValidationContext, RuntimeError> {
    let selected_account = accounts::resolve_account(config)?;
    let selected_account_pubkey = selected_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.clone());
    let selected_farm_d_tag = match selected_account_pubkey.as_deref() {
        Some(pubkey) => resolve_selected_farm_d_tag(config, pubkey)?,
        None => None,
    };
    Ok(ListingValidationContext {
        selected_account_id: selected_account.map(|account| account.record.account_id.to_string()),
        selected_account_pubkey,
        selected_farm_d_tag,
    })
}

fn canonicalize_draft(
    draft: &ListingDraftDocument,
    contents: &str,
    context: &ListingValidationContext,
) -> Result<CanonicalListingDraft, ListingValidationIssueView> {
    if draft.version != 1 {
        return Err(issue_for_field(
            contents,
            "version",
            format!("unsupported listing draft version `{}`", draft.version),
        ));
    }
    if draft.kind.trim() != DRAFT_KIND {
        return Err(issue_for_field(
            contents,
            "kind",
            format!("unsupported listing draft kind `{}`", draft.kind),
        ));
    }

    let listing_id = draft.listing.d_tag.trim().to_owned();
    if !is_d_tag_base64url(&listing_id) {
        return Err(issue_for_field(
            contents,
            "listing.d_tag",
            "listing d_tag must be a 22-character base64url identifier",
        ));
    }

    let seller_pubkey = if let Some(pubkey) = non_empty(draft.listing.seller_pubkey.clone()) {
        pubkey
    } else if let Some(pubkey) = context.selected_account_pubkey.clone() {
        pubkey
    } else {
        return Err(issue_for_field(
            contents,
            "listing.seller_pubkey",
            "missing seller_pubkey and no local account is selected",
        ));
    };

    let farm_d_tag = if let Some(d_tag) = non_empty(draft.listing.farm_d_tag.clone()) {
        d_tag
    } else if let Some(d_tag) = context.selected_farm_d_tag.clone() {
        d_tag
    } else {
        return Err(issue_for_field(
            contents,
            "listing.farm_d_tag",
            "missing farm_d_tag and no matching local farm was found for the selected account",
        ));
    };
    if !is_d_tag_base64url(&farm_d_tag) {
        return Err(issue_for_field(
            contents,
            "listing.farm_d_tag",
            "farm_d_tag must be a 22-character base64url identifier",
        ));
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

    let listing = RadrootsListing {
        d_tag: listing_id.clone(),
        farm: RadrootsListingFarmRef {
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
        discounts: None,
        inventory_available: Some(inventory_available),
        availability: Some(availability),
        delivery_method: Some(delivery_method),
        location: Some(location),
        images: None,
    };

    Ok(CanonicalListingDraft {
        listing_id,
        seller_pubkey,
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

fn invalid_validation_view(
    file: &Path,
    listing_id: &str,
    context: &ListingValidationContext,
    issue: ListingValidationIssueView,
) -> ListingValidateView {
    let mut actions = vec![format!("edit {}", file.display())];
    if context.selected_account_id.is_none() {
        actions.push("radroots account new".to_owned());
    }
    if context.selected_farm_d_tag.is_none() {
        actions.push("radroots sync status".to_owned());
    }

    ListingValidateView {
        state: "invalid".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: file.display().to_string(),
        valid: false,
        listing_id: non_empty(listing_id.to_owned()),
        seller_pubkey: context.selected_account_pubkey.clone(),
        farm_d_tag: context.selected_farm_d_tag.clone(),
        issues: vec![issue],
        actions,
    }
}

fn build_listing_event_preview(
    canonical: &CanonicalListingDraft,
) -> Result<(ListingMutationEventView, String), RuntimeError> {
    let parts = to_wire_parts_with_kind(&canonical.listing, KIND_LISTING)
        .map_err(|error| RuntimeError::Config(format!("invalid listing contract: {error}")))?;
    let event = RadrootsNostrEvent {
        id: String::new(),
        author: canonical.seller_pubkey.clone(),
        created_at: 0,
        kind: KIND_LISTING,
        tags: parts.tags.clone(),
        content: parts.content.clone(),
        sig: String::new(),
    };
    let validated = validate_listing_event(&event)
        .map_err(|error| RuntimeError::Config(format!("invalid listing contract: {error}")))?;
    Ok((
        ListingMutationEventView {
            kind: KIND_LISTING,
            author: canonical.seller_pubkey.clone(),
            content: parts.content,
            tags: parts.tags,
            event_id: None,
            event_addr: validated.listing_addr.clone(),
        },
        validated.listing_addr,
    ))
}

fn daemon_error_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    event_preview: ListingMutationEventView,
    error: DaemonRpcError,
) -> ListingMutationView {
    match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => ListingMutationView {
            state: "unconfigured".to_owned(),
            operation: operation.as_str().to_owned(),
            source: LISTING_WRITE_SOURCE.to_owned(),
            file: args.file.display().to_string(),
            listing_id: canonical.listing_id.clone(),
            listing_addr,
            seller_pubkey: canonical.seller_pubkey.clone(),
            event_kind: KIND_LISTING,
            dry_run: false,
            deduplicated: false,
            job_id: None,
            job_status: None,
            signer_mode: None,
            signer_session_id: None,
            event_id: None,
            event_addr: None,
            idempotency_key: args.idempotency_key.clone(),
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some(reason),
            job: args.print_job.then(|| ListingMutationJobView {
                rpc_method: "bridge.listing.publish".to_owned(),
                state: "unconfigured".to_owned(),
                job_id: None,
                idempotency_key: args.idempotency_key.clone(),
                requested_signer_session_id: args.signer_session_id.clone(),
                signer_mode: Some(config.signer.backend.as_str().to_owned()),
                signer_session_id: None,
            }),
            event: args.print_event.then_some(event_preview),
            actions: vec![
                "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                "start radrootsd with bridge ingress enabled".to_owned(),
            ],
        },
        DaemonRpcError::External(reason) => ListingMutationView {
            state: "unavailable".to_owned(),
            operation: operation.as_str().to_owned(),
            source: LISTING_WRITE_SOURCE.to_owned(),
            file: args.file.display().to_string(),
            listing_id: canonical.listing_id.clone(),
            listing_addr,
            seller_pubkey: canonical.seller_pubkey.clone(),
            event_kind: KIND_LISTING,
            dry_run: false,
            deduplicated: false,
            job_id: None,
            job_status: None,
            signer_mode: None,
            signer_session_id: None,
            event_id: None,
            event_addr: None,
            idempotency_key: args.idempotency_key.clone(),
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some(reason),
            job: args.print_job.then(|| ListingMutationJobView {
                rpc_method: "bridge.listing.publish".to_owned(),
                state: "unavailable".to_owned(),
                job_id: None,
                idempotency_key: args.idempotency_key.clone(),
                requested_signer_session_id: args.signer_session_id.clone(),
                signer_mode: Some(config.signer.backend.as_str().to_owned()),
                signer_session_id: None,
            }),
            event: args.print_event.then_some(event_preview),
            actions: vec!["start radrootsd and verify the rpc url".to_owned()],
        },
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => ListingMutationView {
            state: "error".to_owned(),
            operation: operation.as_str().to_owned(),
            source: LISTING_WRITE_SOURCE.to_owned(),
            file: args.file.display().to_string(),
            listing_id: canonical.listing_id.clone(),
            listing_addr,
            seller_pubkey: canonical.seller_pubkey.clone(),
            event_kind: KIND_LISTING,
            dry_run: false,
            deduplicated: false,
            job_id: None,
            job_status: None,
            signer_mode: None,
            signer_session_id: None,
            event_id: None,
            event_addr: None,
            idempotency_key: args.idempotency_key.clone(),
            requested_signer_session_id: args.signer_session_id.clone(),
            reason: Some(reason),
            job: args.print_job.then(|| ListingMutationJobView {
                rpc_method: "bridge.listing.publish".to_owned(),
                state: "error".to_owned(),
                job_id: None,
                idempotency_key: args.idempotency_key.clone(),
                requested_signer_session_id: args.signer_session_id.clone(),
                signer_mode: Some(config.signer.backend.as_str().to_owned()),
                signer_session_id: None,
            }),
            event: args.print_event.then_some(event_preview),
            actions: vec!["inspect the daemon rpc response contract".to_owned()],
        },
    }
}

fn issue_from_trade_validation(
    error: RadrootsTradeListingValidationError,
    contents: &str,
) -> ListingValidationIssueView {
    match error {
        RadrootsTradeListingValidationError::InvalidSeller => issue_for_field(
            contents,
            "listing.seller_pubkey",
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

fn query_listing_rows(
    executor: &SqliteExecutor,
    lookup: &str,
) -> Result<Vec<ListingRow>, RuntimeError> {
    let sql = "SELECT tp.id, tp.key, tp.category, tp.title, tp.summary, tp.qty_amt, tp.qty_unit, tp.qty_label, tp.qty_avail, tp.price_amt, tp.price_currency, tp.price_qty_amt, tp.price_qty_unit, loc.location_primary \
         FROM trade_product tp \
         LEFT JOIN (\
             SELECT tpl.tb_tp AS trade_product_id, MIN(COALESCE(gl.label, gl.gc_name, gl.gc_admin1_name, gl.gc_country_name, gl.d_tag)) AS location_primary \
             FROM trade_product_location tpl \
             JOIN gcs_location gl ON gl.id = tpl.tb_gl \
             GROUP BY tpl.tb_tp\
         ) loc ON loc.trade_product_id = tp.id \
         WHERE tp.id = ? OR tp.key = ? \
         ORDER BY lower(tp.title) ASC, tp.id ASC;";
    let params = utils::to_params_json(vec![
        Value::from(lookup.to_owned()),
        Value::from(lookup.to_owned()),
    ])?;
    let raw = executor.query_raw(sql, &params)?;
    serde_json::from_str(&raw).map_err(RuntimeError::from)
}

fn resolve_selected_farm_d_tag(
    config: &RuntimeConfig,
    seller_pubkey: &str,
) -> Result<Option<String>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(None);
    }
    let executor = SqliteExecutor::open(&config.local.replica_db_path)?;
    let sql = "SELECT d_tag FROM farm WHERE pubkey = ? ORDER BY d_tag ASC;";
    let params = utils::to_params_json(vec![Value::from(seller_pubkey.to_owned())])?;
    let raw = executor.query_raw(sql, &params)?;
    let rows: Vec<FarmRow> = serde_json::from_str(&raw).map_err(RuntimeError::from)?;
    if rows.len() == 1 {
        Ok(Some(rows[0].d_tag.clone()))
    } else {
        Ok(None)
    }
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
        "listing.seller_pubkey" => &["seller_pubkey ="],
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
    use radroots_events_codec::d_tag::is_d_tag_base64url;

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
                seller_pubkey: "a".repeat(64),
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
        };
        let rendered = toml::to_string_pretty(&document).expect("render draft");
        assert!(rendered.contains("kind = \"listing_draft_v1\""));
    }
}
