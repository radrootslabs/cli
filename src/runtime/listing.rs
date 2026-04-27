use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_core::{
    RadrootsCoreCurrency, RadrootsCoreDecimal, RadrootsCoreMoney, RadrootsCoreQuantity,
    RadrootsCoreQuantityPrice, RadrootsCoreUnit,
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
use radroots_nostr::prelude::radroots_nostr_build_event;
use radroots_replica_db::ReplicaSql;
use radroots_sql_core::SqliteExecutor;
use radroots_trade::listing::publish::validate_listing_for_seller;
use radroots_trade::listing::validation::validate_listing_event;
use serde::{Deserialize, Serialize};

use crate::cli::{
    ListingFileArgs, ListingMutationArgs, ListingNewArgs, RecordKeyArgs, SellAddArgs,
    SellRepriceArgs, SellRestockArgs, SellShowArgs,
};
use crate::domain::runtime::{
    FindPriceView, FindQuantityView, FindResultProvenanceView, ListingGetView,
    ListingMutationEventView, ListingMutationJobView, ListingMutationView, ListingNewView,
    ListingValidateView, ListingValidationIssueView, SellAddView, SellCheckView,
    SellDraftMutationView, SellMutationView, SellShowView, SyncFreshnessView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts;
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use crate::runtime::daemon;
use crate::runtime::daemon::DaemonRpcError;
use crate::runtime::farm_config;
use crate::runtime::signer::{ActorWriteBindingError, resolve_actor_write_authority};
use crate::runtime::sync::freshness_from_executor;

const DRAFT_KIND: &str = "listing_draft_v1";
const LISTING_SOURCE: &str = "local draft · local first";
const LISTING_READ_SOURCE: &str = "local replica · local first";
const LISTING_WRITE_SOURCE: &str = "daemon bridge · durable write plane";
const LISTING_LOCAL_SIGNED_SOURCE: &str = "local account signer · signed event artifact";

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
    farm_setup_action: String,
}

#[derive(Debug, Clone)]
struct ListingAuthoringDefaults {
    farm_config_present: bool,
    farm_defaults_ready: bool,
    farm_next_action: Option<String>,
    farm_reason: Option<String>,
    farm_name: Option<String>,
    selected_account_id: Option<String>,
    selected_account_pubkey: Option<String>,
    selected_farm_d_tag: Option<String>,
    delivery_method: Option<String>,
    location: Option<ListingDraftLocation>,
}

#[derive(Debug, Clone)]
struct DraftSummary {
    product_key: Option<String>,
    title: Option<String>,
    category: Option<String>,
    offer: Option<String>,
    price: Option<String>,
    stock: Option<String>,
    delivery_method: Option<String>,
    location_primary: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedQuantityExpr {
    amount: String,
    unit: String,
    label: String,
}

#[derive(Debug, Clone)]
struct ParsedPriceExpr {
    amount: String,
    currency: String,
    per_amount: String,
    per_unit: String,
}

#[derive(Debug, Clone)]
struct CanonicalListingDraft {
    listing_id: String,
    seller_pubkey: String,
    farm_d_tag: String,
    listing: RadrootsListing,
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
    let (draft, defaults) = build_listing_draft(config, args)?;
    let output_path = default_listing_output_path(args.output.as_ref(), &draft.listing.d_tag)?;
    write_listing_draft(&output_path, &draft, false)?;

    let mut actions = vec![format!(
        "radroots listing validate {}",
        output_path.display()
    )];
    if defaults.selected_account_pubkey.is_none() {
        actions.push("radroots account new".to_owned());
    }
    if let Some(action) = &defaults.farm_next_action {
        actions.push(action.clone());
    }

    Ok(ListingNewView {
        state: "draft created".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: output_path.display().to_string(),
        listing_id: draft.listing.d_tag,
        selected_account_id: defaults.selected_account_id,
        seller_pubkey: defaults.selected_account_pubkey,
        farm_d_tag: defaults.selected_farm_d_tag,
        delivery_method: non_empty(draft.delivery.method.clone()),
        location_primary: non_empty(draft.location.primary.clone()),
        reason: defaults.farm_reason,
        actions,
    })
}

pub fn sell_add(config: &RuntimeConfig, args: &SellAddArgs) -> Result<SellAddView, RuntimeError> {
    let listing_args = listing_args_from_sell_add(args)?;
    let (draft, defaults) = build_listing_draft(config, &listing_args)?;
    let output_path = listing_args
        .output
        .clone()
        .expect("sell add always sets an explicit output path");
    write_listing_draft(&output_path, &draft, false)?;

    let summary = summarize_draft(&draft);
    let mut actions = vec![format!("radroots sell check {}", output_path.display())];
    if defaults.selected_account_pubkey.is_some() && defaults.selected_farm_d_tag.is_some() {
        actions.push(format!("radroots sell publish {}", output_path.display()));
    }
    if defaults.selected_account_pubkey.is_none() {
        actions.push("radroots account create".to_owned());
    }
    if let Some(action) = &defaults.farm_next_action {
        actions.push(action.clone());
    }

    Ok(SellAddView {
        state: "draft_saved".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: output_path.display().to_string(),
        product_key: summary.product_key,
        title: summary.title,
        offer: summary.offer,
        price: summary.price,
        stock: summary.stock,
        farm_name: defaults.farm_name,
        delivery_method: summary.delivery_method,
        location_primary: summary.location_primary,
        reason: defaults.farm_reason,
        actions,
    })
}

pub fn sell_show(
    _config: &RuntimeConfig,
    args: &SellShowArgs,
) -> Result<SellShowView, RuntimeError> {
    let draft = read_listing_draft(&args.file)?;
    let summary = summarize_draft(&draft);
    Ok(SellShowView {
        state: "ready".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: args.file.display().to_string(),
        product_key: summary.product_key,
        title: summary.title,
        category: summary.category,
        offer: summary.offer,
        price: summary.price,
        stock: summary.stock,
        delivery_method: summary.delivery_method,
        location_primary: summary.location_primary,
        reason: None,
        actions: vec![
            format!("radroots sell check {}", args.file.display()),
            format!("radroots sell publish {}", args.file.display()),
        ],
    })
}

pub fn sell_reprice(
    _config: &RuntimeConfig,
    args: &SellRepriceArgs,
) -> Result<SellDraftMutationView, RuntimeError> {
    let mut draft = read_listing_draft(&args.file)?;
    let parsed = parse_price_expr(args.price_expr.as_str())?;
    draft.primary_bin.price_amount = parsed.amount;
    draft.primary_bin.price_currency = parsed.currency;
    draft.primary_bin.price_per_amount = parsed.per_amount;
    draft.primary_bin.price_per_unit = parsed.per_unit;
    write_listing_draft(&args.file, &draft, true)?;

    let summary = summarize_draft(&draft);
    Ok(SellDraftMutationView {
        state: "updated".to_owned(),
        operation: "reprice".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: args.file.display().to_string(),
        product_key: summary.product_key,
        changed_label: "Price".to_owned(),
        changed_value: summary
            .price
            .unwrap_or_else(|| args.price_expr.trim().to_owned()),
        actions: vec![
            format!("radroots sell check {}", args.file.display()),
            format!("radroots sell update {}", args.file.display()),
        ],
    })
}

pub fn sell_restock(
    _config: &RuntimeConfig,
    args: &SellRestockArgs,
) -> Result<SellDraftMutationView, RuntimeError> {
    let mut draft = read_listing_draft(&args.file)?;
    parse_decimal_string(args.available.as_str(), "`sell restock <available>`")?;
    draft.inventory.available = args.available.trim().to_owned();
    write_listing_draft(&args.file, &draft, true)?;

    let summary = summarize_draft(&draft);
    Ok(SellDraftMutationView {
        state: "updated".to_owned(),
        operation: "restock".to_owned(),
        source: LISTING_SOURCE.to_owned(),
        file: args.file.display().to_string(),
        product_key: summary.product_key,
        changed_label: "Stock".to_owned(),
        changed_value: summary
            .stock
            .unwrap_or_else(|| format!("{} available", args.available.trim())),
        actions: vec![
            format!("radroots sell check {}", args.file.display()),
            format!("radroots sell update {}", args.file.display()),
        ],
    })
}

pub fn sell_check(
    config: &RuntimeConfig,
    args: &ListingFileArgs,
) -> Result<SellCheckView, RuntimeError> {
    let view = validate(config, args)?;
    let summary = read_listing_draft(&args.file)
        .ok()
        .map(|draft| summarize_draft(&draft));
    let actions = if view.valid {
        vec![format!("radroots sell publish {}", args.file.display())]
    } else {
        vec![
            format!("radroots sell show {}", args.file.display()),
            "Edit the draft file and run the command again".to_owned(),
        ]
    };

    Ok(SellCheckView {
        state: if view.valid {
            "ready".to_owned()
        } else {
            "invalid".to_owned()
        },
        source: view.source,
        file: view.file,
        valid: view.valid,
        product_key: summary
            .as_ref()
            .and_then(|summary| summary.product_key.clone()),
        seller_pubkey: view.seller_pubkey,
        farm_ref: view.farm_d_tag,
        issues: view.issues,
        actions,
    })
}

pub fn sell_publish(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<SellMutationView, RuntimeError> {
    let view = publish(config, args)?;
    Ok(sell_mutation_from_listing(
        view,
        args.file.as_path(),
        "publish",
    ))
}

pub fn sell_update(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<SellMutationView, RuntimeError> {
    let view = update(config, args)?;
    Ok(sell_mutation_from_listing(
        view,
        args.file.as_path(),
        "update",
    ))
}

pub fn sell_pause(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
) -> Result<SellMutationView, RuntimeError> {
    let view = archive(config, args)?;
    Ok(sell_mutation_from_listing(
        view,
        args.file.as_path(),
        "pause",
    ))
}

fn build_listing_draft(
    config: &RuntimeConfig,
    args: &ListingNewArgs,
) -> Result<(ListingDraftDocument, ListingAuthoringDefaults), RuntimeError> {
    let defaults = authoring_defaults(config)?;
    let quantity_unit = args.quantity_unit.clone().unwrap_or_else(|| "g".to_owned());
    let draft = ListingDraftDocument {
        version: 1,
        kind: DRAFT_KIND.to_owned(),
        listing: ListingDraftMeta {
            d_tag: generate_d_tag(),
            farm_d_tag: defaults.selected_farm_d_tag.clone().unwrap_or_default(),
            seller_pubkey: defaults.selected_account_pubkey.clone().unwrap_or_default(),
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
    };
    Ok((draft, defaults))
}

fn listing_args_from_sell_add(args: &SellAddArgs) -> Result<ListingNewArgs, RuntimeError> {
    let product_key = slugify_ascii(args.product.as_str());
    if product_key.is_empty() {
        return Err(RuntimeError::Config(
            "`sell add <product>` requires at least one ASCII letter or digit".to_owned(),
        ));
    }

    let title = args
        .title
        .clone()
        .unwrap_or_else(|| title_case_ascii(args.product.as_str()));
    let category = args.category.clone().unwrap_or_else(|| title.clone());
    let pack = args.pack.as_deref().map(parse_quantity_expr).transpose()?;
    let price = args
        .price_expr
        .as_deref()
        .map(parse_price_expr)
        .transpose()?;
    let output = Some(match &args.file {
        Some(path) => path.clone(),
        None => std::path::PathBuf::from(format!("listing-{product_key}.toml")),
    });

    Ok(ListingNewArgs {
        output,
        key: Some(product_key),
        title: Some(title.clone()),
        category: Some(category),
        summary: Some(
            args.summary
                .clone()
                .unwrap_or_else(|| format!("Listing for {title}")),
        ),
        bin_id: None,
        quantity_amount: pack.as_ref().map(|pack| pack.amount.clone()),
        quantity_unit: pack.as_ref().map(|pack| pack.unit.clone()),
        price_amount: price.as_ref().map(|price| price.amount.clone()),
        price_currency: price.as_ref().map(|price| price.currency.clone()),
        price_per_amount: price.as_ref().map(|price| price.per_amount.clone()),
        price_per_unit: price.as_ref().map(|price| price.per_unit.clone()),
        available: args.stock.clone(),
        label: pack.as_ref().map(|pack| pack.label.clone()),
    })
}

fn default_listing_output_path(
    explicit: Option<&std::path::PathBuf>,
    listing_id: &str,
) -> Result<std::path::PathBuf, RuntimeError> {
    match explicit {
        Some(path) => Ok(path.clone()),
        None => Ok(std::env::current_dir()?.join(format!("listing-{listing_id}.toml"))),
    }
}

fn write_listing_draft(
    output_path: &Path,
    draft: &ListingDraftDocument,
    overwrite: bool,
) -> Result<(), RuntimeError> {
    if output_path.exists() && !overwrite {
        return Err(RuntimeError::Config(format!(
            "listing draft output {} already exists",
            output_path.display()
        )));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, scaffold_contents(draft)?)?;
    Ok(())
}

fn read_listing_draft(path: &Path) -> Result<ListingDraftDocument, RuntimeError> {
    let contents = fs::read_to_string(path)?;
    toml::from_str::<ListingDraftDocument>(&contents).map_err(|error| {
        RuntimeError::Config(format!(
            "failed to parse listing draft {}: {error}",
            path.display()
        ))
    })
}

fn translate_sell_actions(actions: &[String]) -> Vec<String> {
    actions
        .iter()
        .map(|action| {
            action
                .replace("radroots listing validate ", "radroots sell check ")
                .replace("radroots listing publish ", "radroots sell publish ")
                .replace("radroots listing update ", "radroots sell update ")
                .replace("radroots listing archive ", "radroots sell pause ")
                .replace("radroots account new", "radroots account create")
        })
        .collect()
}

fn successful_sell_mutation_actions(operation: &str, product_key: Option<&str>) -> Vec<String> {
    match operation {
        "publish" => {
            let mut actions = Vec::new();
            if let Some(product_key) = product_key {
                actions.push(format!("radroots market view {product_key}"));
                actions.push(format!("radroots sell add {product_key}"));
            }
            actions
        }
        "update" => product_key
            .map(|product_key| vec![format!("radroots market view {product_key}")])
            .unwrap_or_default(),
        "pause" => product_key
            .map(|product_key| vec![format!("radroots sell add {product_key}")])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn summarize_draft(draft: &ListingDraftDocument) -> DraftSummary {
    DraftSummary {
        product_key: non_empty(draft.product.key.clone()),
        title: non_empty(draft.product.title.clone()),
        category: non_empty(draft.product.category.clone()),
        offer: draft_offer_text(draft),
        price: draft_price_text(draft),
        stock: draft_stock_text(draft),
        delivery_method: non_empty(draft.delivery.method.clone()),
        location_primary: non_empty(draft.location.primary.clone()),
    }
}

fn sell_mutation_from_listing(
    view: ListingMutationView,
    file: &Path,
    operation: &str,
) -> SellMutationView {
    let summary = read_listing_draft(file)
        .ok()
        .map(|draft| summarize_draft(&draft));
    let product_key = summary
        .as_ref()
        .and_then(|summary| summary.product_key.clone());
    let actions = match view.state.as_str() {
        "published" | "deduplicated" => {
            successful_sell_mutation_actions(operation, product_key.as_deref())
        }
        _ => translate_sell_actions(view.actions.as_slice()),
    };

    SellMutationView {
        state: view.state,
        operation: operation.to_owned(),
        source: view.source,
        file: view.file,
        product_key,
        listing_addr: view.listing_addr,
        dry_run: view.dry_run,
        deduplicated: view.deduplicated,
        publish_mode: Some("runtime_bridge".to_owned()),
        job_id: view.job_id,
        job_status: view.job_status,
        event_id: view.event_id,
        reason: view.reason,
        actions,
    }
}

fn draft_offer_text(draft: &ListingDraftDocument) -> Option<String> {
    non_empty(draft.primary_bin.label.clone()).or_else(|| {
        let amount = draft.primary_bin.quantity_amount.trim();
        let unit = draft.primary_bin.quantity_unit.trim();
        if amount.is_empty() || unit.is_empty() {
            None
        } else {
            Some(format!("{} {}", trim_decimal_string(amount), unit))
        }
    })
}

fn draft_price_text(draft: &ListingDraftDocument) -> Option<String> {
    let amount = non_empty(draft.primary_bin.price_amount.clone())?;
    let currency = non_empty(draft.primary_bin.price_currency.clone())?;
    let per_amount = non_empty(draft.primary_bin.price_per_amount.clone())?;
    let per_unit = non_empty(draft.primary_bin.price_per_unit.clone())?;
    let denominator = if per_unit == "each"
        && numeric_strings_equal(
            per_amount.as_str(),
            draft.primary_bin.quantity_amount.trim(),
        )
        && !draft.primary_bin.label.trim().is_empty()
    {
        draft.primary_bin.label.trim().to_owned()
    } else if per_amount == "1" {
        per_unit.to_owned()
    } else {
        format!("{} {}", trim_decimal_string(&per_amount), per_unit)
    };
    Some(format!(
        "{} {}/{}",
        trim_decimal_string(&amount),
        currency.to_ascii_uppercase(),
        denominator
    ))
}

fn draft_stock_text(draft: &ListingDraftDocument) -> Option<String> {
    non_empty(draft.inventory.available.clone())
        .map(|available| format!("{} available", trim_decimal_string(&available)))
}

fn parse_quantity_expr(expr: &str) -> Result<ParsedQuantityExpr, RuntimeError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(
            "quantity expression must not be empty".to_owned(),
        ));
    }
    if trimmed.eq_ignore_ascii_case("dozen") {
        return Ok(ParsedQuantityExpr {
            amount: "12".to_owned(),
            unit: "each".to_owned(),
            label: "dozen".to_owned(),
        });
    }

    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(RuntimeError::Config(
            "quantity expression must not be empty".to_owned(),
        ));
    }

    let (amount, unit) = if parse_decimal_string(parts[0], "quantity amount").is_ok() {
        let Some(unit) = parts.get(1) else {
            return Err(RuntimeError::Config(
                "quantity expression must include a unit, for example `1 kg`".to_owned(),
            ));
        };
        (parts[0].trim().to_owned(), unit.trim().to_ascii_lowercase())
    } else {
        ("1".to_owned(), parts[0].trim().to_ascii_lowercase())
    };

    unit.parse::<RadrootsCoreUnit>().map_err(|_| {
        RuntimeError::Config(format!(
            "quantity expression uses unsupported unit `{unit}`"
        ))
    })?;

    Ok(ParsedQuantityExpr {
        amount,
        unit,
        label: trimmed.to_owned(),
    })
}

fn parse_price_expr(expr: &str) -> Result<ParsedPriceExpr, RuntimeError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(
            "price expression must not be empty".to_owned(),
        ));
    }

    let segments = trimmed.split_whitespace().collect::<Vec<_>>();
    if segments.len() < 2 {
        return Err(RuntimeError::Config(
            "price expression must look like `10 USD/kg`".to_owned(),
        ));
    }

    parse_decimal_string(segments[0], "price amount")?;
    let remainder = segments[1..].join(" ");
    let Some((currency, per_expr)) = remainder.split_once('/') else {
        return Err(RuntimeError::Config(
            "price expression must include a `/`, for example `10 USD/kg`".to_owned(),
        ));
    };
    let per = parse_quantity_expr(per_expr)?;
    RadrootsCoreCurrency::from_str_upper(currency.trim().to_ascii_uppercase().as_str()).map_err(
        |_| {
            RuntimeError::Config(format!(
                "price expression uses unsupported currency `{}`",
                currency.trim()
            ))
        },
    )?;

    Ok(ParsedPriceExpr {
        amount: segments[0].trim().to_owned(),
        currency: currency.trim().to_ascii_uppercase(),
        per_amount: per.amount,
        per_unit: per.unit,
    })
}

fn parse_decimal_string(value: &str, label: &str) -> Result<RadrootsCoreDecimal, RuntimeError> {
    value
        .trim()
        .parse::<RadrootsCoreDecimal>()
        .map_err(|_| RuntimeError::Config(format!("{label} must be a valid decimal value")))
}

fn slugify_ascii(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !slug.is_empty() && !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    slug.trim_matches('-').to_owned()
}

fn title_case_ascii(value: &str) -> String {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(capitalize_ascii_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_ascii_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut rendered = String::new();
    rendered.push(first.to_ascii_uppercase());
    rendered.push_str(chars.as_str());
    rendered
}

fn numeric_strings_equal(lhs: &str, rhs: &str) -> bool {
    trim_decimal_string(lhs) == trim_decimal_string(rhs)
}

fn trim_decimal_string(value: &str) -> String {
    if let Ok(parsed) = value.trim().parse::<RadrootsCoreDecimal>() {
        parsed.to_string()
    } else {
        value.trim().to_owned()
    }
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
            listing_addr: None,
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

    let db = ReplicaSql::new(SqliteExecutor::open(&config.local.replica_db_path)?);
    let rows = db.trade_product_lookup(args.key.as_str())?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(ListingGetView {
            state: "missing".to_owned(),
            source: LISTING_READ_SOURCE.to_owned(),
            lookup: args.key.clone(),
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
        listing_addr: row.listing_addr.and_then(non_empty),
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

    if config.output.dry_run
        && matches!(operation, ListingMutationOperation::Publish)
        && matches!(config.signer.backend, SignerBackend::Local)
    {
        validate_local_listing_signer(config, &canonical)?;
    }

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

    if matches!(operation, ListingMutationOperation::Publish)
        && matches!(config.signer.backend, SignerBackend::Local)
    {
        return local_signed_view(
            config,
            args,
            operation,
            &canonical,
            listing_addr,
            event_preview,
        );
    }

    let signer_authority =
        match resolve_actor_write_authority(config, "seller", canonical.seller_pubkey.as_str()) {
            Ok(authority) => authority,
            Err(error) => {
                return Ok(binding_error_view(
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

    let signer_session_id = match daemon::resolve_signer_session_id(
        config,
        "seller",
        canonical.seller_pubkey.as_str(),
        KIND_LISTING,
        args.signer_session_id.as_deref(),
        signer_authority.as_ref(),
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
        signer_authority.as_ref(),
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
        "# radroots listing draft v1\n# this scaffold applies selected farm defaults and provided product inputs when available\n# review any remaining empty fields, then run `radroots listing validate <file>`\n\n{toml}"
    ))
}

fn validation_context(config: &RuntimeConfig) -> Result<ListingValidationContext, RuntimeError> {
    let defaults = authoring_defaults(config)?;
    let selected_farm_d_tag = match (
        defaults.farm_config_present,
        defaults.selected_farm_d_tag,
        defaults.selected_account_pubkey.clone(),
    ) {
        (true, d_tag, _) => d_tag,
        (false, Some(d_tag), _) => Some(d_tag),
        (false, None, Some(pubkey)) => resolve_selected_farm_d_tag(config, pubkey.as_str())?,
        (false, None, None) => None,
    };
    Ok(ListingValidationContext {
        selected_account_id: defaults.selected_account_id,
        selected_account_pubkey: defaults.selected_account_pubkey,
        selected_farm_d_tag,
        farm_setup_action: farm_setup_action(config)?,
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
            "missing farm_d_tag and no selected farm config is available",
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
        actions.push(context.farm_setup_action.clone());
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
    let validated = validate_listing_for_seller(
        canonical.listing.clone(),
        canonical.seller_pubkey.as_str(),
        KIND_LISTING,
    )
    .map_err(|error| RuntimeError::Config(format!("invalid listing contract: {error}")))?;
    Ok((
        ListingMutationEventView {
            kind: KIND_LISTING,
            author: canonical.seller_pubkey.clone(),
            created_at: None,
            content: parts.content,
            tags: parts.tags,
            event_id: None,
            signature: None,
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

fn local_signed_view(
    config: &RuntimeConfig,
    args: &ListingMutationArgs,
    operation: ListingMutationOperation,
    canonical: &CanonicalListingDraft,
    listing_addr: String,
    event_preview: ListingMutationEventView,
) -> Result<ListingMutationView, RuntimeError> {
    let signed_event = match sign_listing_event(config, canonical) {
        Ok(event) => event,
        Err(error) => {
            return Ok(ListingMutationView {
                state: "unconfigured".to_owned(),
                operation: operation.as_str().to_owned(),
                source: LISTING_LOCAL_SIGNED_SOURCE.to_owned(),
                file: args.file.display().to_string(),
                listing_id: canonical.listing_id.clone(),
                listing_addr,
                seller_pubkey: canonical.seller_pubkey.clone(),
                event_kind: KIND_LISTING,
                dry_run: false,
                deduplicated: false,
                job_id: None,
                job_status: None,
                signer_mode: Some(config.signer.backend.as_str().to_owned()),
                event_id: None,
                event_addr: None,
                idempotency_key: args.idempotency_key.clone(),
                signer_session_id: None,
                requested_signer_session_id: args.signer_session_id.clone(),
                reason: Some(error.to_string()),
                job: args.print_job.then(|| ListingMutationJobView {
                    rpc_method: "local.listing.sign".to_owned(),
                    state: "unconfigured".to_owned(),
                    job_id: None,
                    idempotency_key: args.idempotency_key.clone(),
                    requested_signer_session_id: args.signer_session_id.clone(),
                    signer_mode: Some(config.signer.backend.as_str().to_owned()),
                    signer_session_id: None,
                }),
                event: args.print_event.then_some(event_preview),
                actions: vec!["radroots signer status get".to_owned()],
            });
        }
    };
    let event_view = signed_listing_event_view(&signed_event, listing_addr.as_str());
    Ok(ListingMutationView {
        state: "signed".to_owned(),
        operation: operation.as_str().to_owned(),
        source: LISTING_LOCAL_SIGNED_SOURCE.to_owned(),
        file: args.file.display().to_string(),
        listing_id: canonical.listing_id.clone(),
        listing_addr: listing_addr.clone(),
        seller_pubkey: canonical.seller_pubkey.clone(),
        event_kind: KIND_LISTING,
        dry_run: false,
        deduplicated: false,
        job_id: None,
        job_status: None,
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        event_id: event_view.event_id.clone(),
        event_addr: Some(listing_addr),
        idempotency_key: args.idempotency_key.clone(),
        signer_session_id: None,
        requested_signer_session_id: args.signer_session_id.clone(),
        reason: Some("signed locally; relay delivery was not attempted".to_owned()),
        job: args.print_job.then(|| ListingMutationJobView {
            rpc_method: "local.listing.sign".to_owned(),
            state: "not_submitted".to_owned(),
            job_id: None,
            idempotency_key: args.idempotency_key.clone(),
            requested_signer_session_id: args.signer_session_id.clone(),
            signer_mode: Some(config.signer.backend.as_str().to_owned()),
            signer_session_id: None,
        }),
        event: Some(event_view),
        actions: Vec::new(),
    })
}

fn sign_listing_event(
    config: &RuntimeConfig,
    canonical: &CanonicalListingDraft,
) -> Result<radroots_nostr::prelude::RadrootsNostrEvent, RuntimeError> {
    let signing = resolve_listing_signing_identity(config, canonical)?;
    let parts = to_wire_parts_with_kind(&canonical.listing, KIND_LISTING)
        .map_err(|error| RuntimeError::Config(format!("invalid listing contract: {error}")))?;
    let event = radroots_nostr_build_event(parts.kind, parts.content, parts.tags)
        .map_err(|error| RuntimeError::Config(format!("build local listing event: {error}")))?
        .sign_with_keys(signing.identity.keys())
        .map_err(|error| RuntimeError::Config(format!("sign local listing event: {error}")))?;
    Ok(event)
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
    let signing = accounts::resolve_local_signing_identity(config)?;
    let account_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !account_pubkey.eq_ignore_ascii_case(canonical.seller_pubkey.as_str()) {
        return Err(RuntimeError::Config(format!(
            "selected local account pubkey `{account_pubkey}` cannot sign listing seller_pubkey `{}`",
            canonical.seller_pubkey
        )));
    }
    Ok(signing)
}

fn signed_listing_event_view(
    event: &radroots_nostr::prelude::RadrootsNostrEvent,
    listing_addr: &str,
) -> ListingMutationEventView {
    ListingMutationEventView {
        kind: event.kind.as_u16() as u32,
        author: event.pubkey.to_string(),
        created_at: Some(u32::try_from(event.created_at.as_secs()).unwrap_or(u32::MAX)),
        content: event.content.clone(),
        tags: event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect(),
        event_id: Some(event.id.to_string()),
        signature: Some(event.sig.to_string()),
        event_addr: listing_addr.to_owned(),
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
    let (state, reason, actions) = match error {
        ActorWriteBindingError::Unconfigured(reason) => (
            "unconfigured".to_owned(),
            reason,
            vec!["run radroots signer status get".to_owned()],
        ),
        ActorWriteBindingError::Unavailable(reason) => (
            "unavailable".to_owned(),
            reason,
            vec!["run radroots signer status get".to_owned()],
        ),
    };

    ListingMutationView {
        state: state.clone(),
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
        signer_mode: Some(config.signer.backend.as_str().to_owned()),
        signer_session_id: None,
        event_id: None,
        event_addr: None,
        idempotency_key: args.idempotency_key.clone(),
        requested_signer_session_id: args.signer_session_id.clone(),
        reason: Some(reason),
        job: args.print_job.then(|| ListingMutationJobView {
            rpc_method: "bridge.listing.publish".to_owned(),
            state,
            job_id: None,
            idempotency_key: args.idempotency_key.clone(),
            requested_signer_session_id: args.signer_session_id.clone(),
            signer_mode: Some(config.signer.backend.as_str().to_owned()),
            signer_session_id: None,
        }),
        event: args.print_event.then_some(event_preview),
        actions,
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

fn authoring_defaults(config: &RuntimeConfig) -> Result<ListingAuthoringDefaults, RuntimeError> {
    let selected_account = accounts::resolve_account(config)?;
    let mut defaults = ListingAuthoringDefaults {
        farm_config_present: false,
        farm_defaults_ready: false,
        farm_next_action: Some(farm_setup_action(config)?),
        farm_reason: Some(
            "selected farm draft not found; delivery, location, and farm defaults were left blank"
                .to_owned(),
        ),
        farm_name: None,
        selected_account_id: selected_account
            .as_ref()
            .map(|account| account.record.account_id.to_string()),
        selected_account_pubkey: selected_account
            .as_ref()
            .map(|account| account.record.public_identity.public_key_hex.clone()),
        selected_farm_d_tag: None,
        delivery_method: None,
        location: None,
    };

    let Some(resolved) = farm_config::load(config, None)? else {
        return Ok(defaults);
    };
    let Some(account) = configured_account(config, &resolved.document.selection.account)? else {
        return Err(RuntimeError::Config(format!(
            "farm config account `{}` is not present in the local account store",
            resolved.document.selection.account
        )));
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
    defaults.selected_account_id = Some(resolved.document.selection.account.clone());
    defaults.selected_account_pubkey = Some(account.record.public_identity.public_key_hex.clone());
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
        defaults.farm_next_action = Some("radroots farm check".to_owned());
        defaults.farm_reason = Some(
            "selected farm draft is missing delivery or location defaults; those fields were left blank"
                .to_owned(),
        );
    }
    Ok(defaults)
}

fn resolve_selected_farm_d_tag(
    config: &RuntimeConfig,
    seller_pubkey: &str,
) -> Result<Option<String>, RuntimeError> {
    if !config.local.replica_db_path.exists() {
        return Ok(None);
    }
    let db = ReplicaSql::new(SqliteExecutor::open(&config.local.replica_db_path)?);
    db.farm_unique_d_tag_by_pubkey(seller_pubkey)
        .map_err(RuntimeError::from)
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
    Ok("radroots farm init".to_owned())
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
