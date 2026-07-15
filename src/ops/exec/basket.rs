use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_event::order::RadrootsOrderEconomics;
use radroots_replica_schema::trade_product::{ITradeProductFieldsFilter, ITradeProductFindMany};
use radroots_replica_store::{ReplicaSql, trade_product};
use radroots_sql_core::SqlxSqliteExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::global::{OrderDraftAdjustmentArgs, OrderDraftCreateArgs};
use crate::ops::{
    BasketCreateRequest, BasketCreateResult, BasketGetRequest, BasketGetResult,
    BasketItemAddRequest, BasketItemAddResult, BasketItemRemoveRequest, BasketItemRemoveResult,
    BasketItemUpdateRequest, BasketItemUpdateResult, BasketListRequest, BasketListResult,
    BasketQuoteRequest, BasketQuoteResult, OperationAdapterError, OperationRequest,
    OperationRequestData, OperationRequestPayload, OperationResult, OperationResultData,
    OperationService,
};
use crate::runtime::config::RuntimeConfig;
use crate::view::runtime::OrderNewView;

const BASKET_KIND: &str = "basket_v1";
const BASKET_SOURCE: &str = "local baskets - local first";
const BASKET_QUOTE_SOURCE: &str = "local baskets - deterministic quote";
const BASKETS_DIR: &str = "baskets";

static BASKET_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketDocument {
    version: u32,
    kind: String,
    basket: BasketState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quote: Option<BasketQuote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketState {
    basket_id: String,
    created_at_unix: u64,
    updated_at_unix: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<BasketItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    adjustments: Vec<BasketAdjustment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketItem {
    item_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    listing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    listing_addr: Option<String>,
    bin_id: String,
    quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketAdjustment {
    id: String,
    effect: String,
    amount: String,
    currency: String,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketQuote {
    quote_id: String,
    quote_version: u32,
    trade_id: String,
    trade_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    economics: Option<RadrootsOrderEconomics>,
    ready_for_submit: bool,
    created_at_unix: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    issues: Vec<BasketIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketIssue {
    code: String,
    field: String,
    message: String,
}

#[derive(Debug, Clone)]
struct LoadedBasket {
    file: PathBuf,
    document: BasketDocument,
}

#[derive(Debug, Clone)]
struct BasketProductBinState {
    primary_bin_id: Option<String>,
    verified_primary_bin_id: Option<String>,
}

#[derive(Debug, Clone)]
enum BasketProductResolution {
    Resolved(BasketProductBinState),
    Unresolved,
    Ambiguous(usize),
}

pub struct BasketOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> BasketOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<BasketCreateRequest> for BasketOperationService<'_> {
    type Result = BasketCreateResult;

    fn execute(
        &self,
        request: OperationRequest<BasketCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = string_input(&request, "basket_id").unwrap_or_else(next_basket_id);
        let initial_item = optional_item_from_request(&request, None)?;
        let file = basket_lookup_path(self.config, basket_id.as_str());
        if file.exists() {
            return Err(invalid_input(
                request.operation_id(),
                format!("basket `{basket_id}` already exists"),
            ));
        }
        if request.context.dry_run {
            return json_operation_result::<BasketCreateResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "file": file.display().to_string(),
                "item_count": initial_item.as_ref().map(|_| 1).unwrap_or(0),
                "actions": ["radroots basket create"],
            }));
        }

        let now = now_unix();
        let document = BasketDocument {
            version: 1,
            kind: BASKET_KIND.to_owned(),
            basket: BasketState {
                basket_id,
                created_at_unix: now,
                updated_at_unix: now,
                items: initial_item.into_iter().collect(),
                adjustments: Vec::new(),
            },
            quote: None,
        };
        save_basket(file.as_path(), &document)?;
        json_operation_result::<BasketCreateResult>(basket_view(
            self.config,
            &document,
            file.as_path(),
            None,
        )?)
    }
}

impl OperationService<BasketGetRequest> for BasketOperationService<'_> {
    type Result = BasketGetResult;

    fn execute(
        &self,
        request: OperationRequest<BasketGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let lookup = required_basket_id(&request)?;
        let Some(loaded) = load_basket_optional(self.config, lookup.as_str())? else {
            return json_operation_result::<BasketGetResult>(missing_basket_view(
                self.config,
                lookup.as_str(),
            ));
        };
        json_operation_result::<BasketGetResult>(basket_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
            None,
        )?)
    }
}

impl OperationService<BasketListRequest> for BasketOperationService<'_> {
    type Result = BasketListResult;

    fn execute(
        &self,
        _request: OperationRequest<BasketListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let baskets = list_basket_summaries(self.config)?;
        json_operation_result::<BasketListResult>(json!({
            "state": if baskets.is_empty() { "empty" } else { "ready" },
            "source": BASKET_SOURCE,
            "count": baskets.len(),
            "baskets": baskets,
            "actions": if baskets.is_empty() {
                vec!["radroots basket create".to_owned()]
            } else {
                Vec::new()
            },
        }))
    }
}

impl OperationService<BasketItemAddRequest> for BasketOperationService<'_> {
    type Result = BasketItemAddResult;

    fn execute(
        &self,
        request: OperationRequest<BasketItemAddRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let item = required_item_from_request(&request, Some(next_item_id(&loaded.document)))?;
        if request.context.dry_run {
            return json_operation_result::<BasketItemAddResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "item": item,
                "actions": ["radroots basket item add"],
            }));
        }

        loaded.document.basket.items.push(item);
        touch_basket(&mut loaded.document);
        loaded.document.quote = None;
        save_basket(loaded.file.as_path(), &loaded.document)?;
        json_operation_result::<BasketItemAddResult>(basket_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        )?)
    }
}

impl OperationService<BasketItemUpdateRequest> for BasketOperationService<'_> {
    type Result = BasketItemUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<BasketItemUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let item_id = required_string(&request, "item_id")?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let Some(index) = loaded
            .document
            .basket
            .items
            .iter()
            .position(|item| item.item_id == item_id)
        else {
            return Err(invalid_input(
                request.operation_id(),
                format!("basket item `{item_id}` was not found"),
            ));
        };

        let updated =
            update_item_from_request(&request, loaded.document.basket.items[index].clone())?;
        if request.context.dry_run {
            return json_operation_result::<BasketItemUpdateResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "item": updated,
                "actions": ["radroots basket item update"],
            }));
        }

        loaded.document.basket.items[index] = updated;
        touch_basket(&mut loaded.document);
        loaded.document.quote = None;
        save_basket(loaded.file.as_path(), &loaded.document)?;
        json_operation_result::<BasketItemUpdateResult>(basket_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        )?)
    }
}

impl OperationService<BasketItemRemoveRequest> for BasketOperationService<'_> {
    type Result = BasketItemRemoveResult;

    fn execute(
        &self,
        request: OperationRequest<BasketItemRemoveRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let item_id = required_string(&request, "item_id")?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let Some(index) = loaded
            .document
            .basket
            .items
            .iter()
            .position(|item| item.item_id == item_id)
        else {
            return Err(invalid_input(
                request.operation_id(),
                format!("basket item `{item_id}` was not found"),
            ));
        };

        if request.context.dry_run {
            return json_operation_result::<BasketItemRemoveResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "item_id": item_id,
                "actions": ["radroots basket item remove"],
            }));
        }

        loaded.document.basket.items.remove(index);
        touch_basket(&mut loaded.document);
        loaded.document.quote = None;
        save_basket(loaded.file.as_path(), &loaded.document)?;
        json_operation_result::<BasketItemRemoveResult>(basket_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        )?)
    }
}

impl OperationService<BasketQuoteRequest> for BasketOperationService<'_> {
    type Result = BasketQuoteResult;

    fn execute(
        &self,
        request: OperationRequest<BasketQuoteRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let issues = basket_issues(self.config, &loaded.document)?;
        if !issues.is_empty() {
            let actions = basket_actions(&loaded.document, issues.as_slice());
            return json_operation_result::<BasketQuoteResult>(json!({
                "state": "unconfigured",
                "source": BASKET_QUOTE_SOURCE,
                "basket_id": basket_id,
                "file": loaded.file.display().to_string(),
                "ready_for_quote": false,
                "issues": issues,
                "actions": actions,
            }));
        }

        let item = loaded
            .document
            .basket
            .items
            .first()
            .expect("validated basket has one item")
            .clone();
        if request.context.dry_run {
            let order = crate::runtime::order::scaffold_preflight(
                self.config,
                &OrderDraftCreateArgs {
                    listing: item.listing.clone(),
                    listing_addr: item.listing_addr.clone(),
                    bin_id: Some(item.bin_id.clone()),
                    bin_count: Some(item.quantity),
                    adjustments: order_adjustments_from_basket(&loaded.document),
                },
            )
            .map_err(|error| {
                OperationAdapterError::runtime_failure(request.operation_id(), error)
            })?;
            return json_operation_result::<BasketQuoteResult>(json!({
                "state": "dry_run",
                "source": BASKET_QUOTE_SOURCE,
                "basket_id": basket_id,
                "file": loaded.file.display().to_string(),
                "item": item,
                "trade": order,
                "actions": ["radroots basket quote create"],
            }));
        }

        let order = crate::runtime::order::scaffold(
            self.config,
            &OrderDraftCreateArgs {
                listing: item.listing.clone(),
                listing_addr: item.listing_addr.clone(),
                bin_id: Some(item.bin_id.clone()),
                bin_count: Some(item.quantity),
                adjustments: order_adjustments_from_basket(&loaded.document),
            },
        )
        .map_err(|error| OperationAdapterError::runtime_failure(request.operation_id(), error))?;
        let quote_economics = order.economics.clone();
        let quote = BasketQuote {
            quote_id: quote_economics
                .as_ref()
                .map(|economics| economics.quote_id.to_string())
                .unwrap_or_else(|| format!("quote_{}", loaded.document.basket.basket_id)),
            quote_version: quote_economics
                .as_ref()
                .map(|economics| economics.quote_version)
                .unwrap_or(1),
            trade_id: order.order_id.clone(),
            trade_file: order.file.clone(),
            economics: quote_economics,
            ready_for_submit: order.ready_for_submit,
            created_at_unix: now_unix(),
            issues: quote_issues_from_order(&order),
        };
        loaded.document.quote = Some(quote.clone());
        touch_basket(&mut loaded.document);
        save_basket(loaded.file.as_path(), &loaded.document)?;

        json_operation_result::<BasketQuoteResult>(json!({
            "state": "quoted",
            "source": BASKET_QUOTE_SOURCE,
            "basket_id": loaded.document.basket.basket_id,
            "file": loaded.file.display().to_string(),
            "quote": quote,
            "trade": order,
            "actions": quote_actions(&order),
        }))
    }
}

fn optional_item_from_request<P>(
    request: &OperationRequest<P>,
    item_id: Option<String>,
) -> Result<Option<BasketItem>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    if string_input(request, "listing").is_none()
        && string_input(request, "listing_addr").is_none()
        && string_input(request, "bin_id").is_none()
    {
        return Ok(None);
    }
    required_item_from_request(request, item_id).map(Some)
}

fn required_item_from_request<P>(
    request: &OperationRequest<P>,
    item_id: Option<String>,
) -> Result<BasketItem, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    let listing = string_input(request, "listing");
    let listing_addr = string_input(request, "listing_addr");
    if listing.is_none() && listing_addr.is_none() {
        return Err(invalid_input(
            request.operation_id(),
            "missing required `listing` or `listing_addr` input".to_owned(),
        ));
    }
    let bin_id = required_string(request, "bin_id")?;
    let quantity = quantity_input(request)?.unwrap_or(1);
    if quantity == 0 {
        return Err(invalid_input(
            request.operation_id(),
            "`quantity` must be greater than 0".to_owned(),
        ));
    }

    Ok(BasketItem {
        item_id: item_id
            .or_else(|| string_input(request, "item_id"))
            .unwrap_or_else(|| "item_1".to_owned()),
        listing,
        listing_addr,
        bin_id,
        quantity,
    })
}

fn update_item_from_request<P>(
    request: &OperationRequest<P>,
    mut item: BasketItem,
) -> Result<BasketItem, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    let mut changed = false;
    if let Some(listing) = string_input(request, "listing") {
        item.listing = Some(listing);
        changed = true;
    }
    if let Some(listing_addr) = string_input(request, "listing_addr") {
        item.listing_addr = Some(listing_addr);
        changed = true;
    }
    if let Some(bin_id) = string_input(request, "bin_id") {
        item.bin_id = bin_id;
        changed = true;
    }
    if let Some(quantity) = quantity_input(request)? {
        if quantity == 0 {
            return Err(invalid_input(
                request.operation_id(),
                "`quantity` must be greater than 0".to_owned(),
            ));
        }
        item.quantity = quantity;
        changed = true;
    }
    if !changed {
        return Err(invalid_input(
            request.operation_id(),
            "no item update input was provided".to_owned(),
        ));
    }
    Ok(item)
}

fn basket_view(
    config: &RuntimeConfig,
    document: &BasketDocument,
    file: &Path,
    state: Option<&str>,
) -> Result<Value, OperationAdapterError> {
    let issues = basket_issues(config, document)?;
    let ready_for_quote = issues.is_empty();
    let actions = basket_actions(document, issues.as_slice());
    Ok(json!({
        "state": state.unwrap_or("ready"),
        "source": BASKET_SOURCE,
        "basket_id": document.basket.basket_id,
        "file": file.display().to_string(),
        "item_count": document.basket.items.len(),
        "items": document.basket.items,
        "adjustment_count": document.basket.adjustments.len(),
        "adjustments": document.basket.adjustments,
        "quote": document.quote,
        "ready_for_quote": ready_for_quote,
        "issues": issues,
        "actions": actions,
    }))
}

fn missing_basket_view(config: &RuntimeConfig, lookup: &str) -> Value {
    json!({
        "state": "missing",
        "source": BASKET_SOURCE,
        "lookup": lookup,
        "file": basket_lookup_path(config, lookup).display().to_string(),
        "reason": format!("basket `{lookup}` was not found"),
        "actions": ["radroots basket list", "radroots basket create"],
    })
}

fn list_basket_summaries(config: &RuntimeConfig) -> Result<Vec<Value>, OperationAdapterError> {
    let dir = baskets_dir(config);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut baskets = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|error| {
        OperationAdapterError::Runtime(format!("read basket directory {}: {error}", dir.display()))
    })? {
        let entry = entry.map_err(|error| {
            OperationAdapterError::Runtime(format!(
                "read basket directory {}: {error}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let loaded = load_basket_path(path.as_path())?;
        let issues = basket_issues(config, &loaded.document)?;
        let ready_for_quote = issues.is_empty();
        baskets.push(json!({
            "basket_id": loaded.document.basket.basket_id,
            "state": if ready_for_quote { "ready" } else { "unconfigured" },
            "file": loaded.file.display().to_string(),
            "item_count": loaded.document.basket.items.len(),
            "adjustment_count": loaded.document.basket.adjustments.len(),
            "ready_for_quote": ready_for_quote,
            "issues": issues,
            "quote": loaded.document.quote,
            "updated_at_unix": loaded.document.basket.updated_at_unix,
        }));
    }
    baskets.sort_by(|left, right| {
        right["updated_at_unix"]
            .as_u64()
            .cmp(&left["updated_at_unix"].as_u64())
            .then_with(|| {
                left["basket_id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["basket_id"].as_str().unwrap_or_default())
            })
    });
    Ok(baskets)
}

fn basket_issues(
    config: &RuntimeConfig,
    document: &BasketDocument,
) -> Result<Vec<BasketIssue>, OperationAdapterError> {
    let mut issues = Vec::new();
    if document.basket.items.is_empty() {
        issues.push(basket_issue(
            "basket_items_missing",
            "basket.items",
            "basket must contain one item before quote creation",
        ));
    }
    if document.basket.items.len() > 1 {
        issues.push(basket_issue(
            "basket_items_unsupported",
            "basket.items",
            "basket quotes support exactly one item",
        ));
    }
    for item in &document.basket.items {
        if item.listing.is_none() && item.listing_addr.is_none() {
            issues.push(basket_issue(
                "basket_item_listing_missing",
                format!("basket.items.{}.listing", item.item_id),
                "item must include listing or listing_addr",
            ));
        }
        if item.bin_id.trim().is_empty() {
            issues.push(basket_issue(
                "basket_item_bin_missing",
                format!("basket.items.{}.bin_id", item.item_id),
                "item must include bin_id",
            ));
        }
        if item.quantity == 0 {
            issues.push(basket_issue(
                "basket_item_quantity_invalid",
                format!("basket.items.{}.quantity", item.item_id),
                "item quantity must be greater than 0",
            ));
        }
    }
    if issues.is_empty() {
        issues.extend(basket_market_issues(config, document)?);
    }
    Ok(issues)
}

fn basket_market_issues(
    config: &RuntimeConfig,
    document: &BasketDocument,
) -> Result<Vec<BasketIssue>, OperationAdapterError> {
    if !config.local.replica_store_path.exists() {
        return Ok(vec![basket_issue(
            "basket_market_replica_missing",
            "local.replica_store",
            "current local replica data is required before quote creation; run `radroots store inspect` and `radroots market pull`",
        )]);
    }
    let executor = SqlxSqliteExecutor::open(&config.local.replica_store_path).map_err(|error| {
        OperationAdapterError::Runtime(format!(
            "open local replica {}: {error}",
            config.local.replica_store_path.display()
        ))
    })?;
    let mut issues = Vec::new();
    for item in &document.basket.items {
        let product = match basket_product_bin_state(config, &executor, item)? {
            BasketProductResolution::Resolved(product) => product,
            BasketProductResolution::Unresolved => {
                issues.push(basket_issue(
                    "basket_item_listing_unresolved",
                    basket_item_listing_field(item),
                    "basket item listing is not active in the current local replica; run `radroots market pull` before quote creation",
                ));
                continue;
            }
            BasketProductResolution::Ambiguous(count) => {
                issues.push(basket_issue(
                    "basket_item_listing_ambiguous",
                    basket_item_listing_field(item),
                    format!(
                        "basket item listing matched {count} active local replica rows; choose a unique listing before quote creation"
                    ),
                ));
                continue;
            }
        };
        let Some(primary_bin_id) = product.primary_bin_id.as_deref().and_then(non_empty_ref) else {
            issues.push(basket_issue(
                "listing_primary_bin_missing",
                format!("basket.items.{}.bin_id", item.item_id),
                "current local replica listing primary bin is required before quote creation",
            ));
            continue;
        };
        let Some(verified_primary_bin_id) = product
            .verified_primary_bin_id
            .as_deref()
            .and_then(non_empty_ref)
        else {
            issues.push(basket_issue(
                "listing_primary_bin_invalid",
                format!("basket.items.{}.bin_id", item.item_id),
                format!("current local replica primary bin `{primary_bin_id}` is not verified"),
            ));
            continue;
        };
        if verified_primary_bin_id != primary_bin_id {
            issues.push(basket_issue(
                "listing_primary_bin_invalid",
                format!("basket.items.{}.bin_id", item.item_id),
                format!(
                    "current local replica primary bin `{primary_bin_id}` does not match verified primary bin `{verified_primary_bin_id}`"
                ),
            ));
            continue;
        }
        if item.bin_id != primary_bin_id {
            issues.push(basket_issue(
                "order_bin_unknown",
                format!("basket.items.{}.bin_id", item.item_id),
                format!(
                    "basket bin `{}` is not in the current local listing bin set; expected primary bin `{primary_bin_id}`",
                    item.bin_id
                ),
            ));
        }
    }
    Ok(issues)
}

fn basket_product_bin_state(
    config: &RuntimeConfig,
    executor: &SqlxSqliteExecutor,
    item: &BasketItem,
) -> Result<BasketProductResolution, OperationAdapterError> {
    if let Some(listing_addr) = item.listing_addr.as_deref().and_then(non_empty_ref) {
        let product_rows = trade_product::find_many(
            executor,
            &ITradeProductFindMany {
                filter: Some(trade_product_listing_addr_filter(listing_addr)),
            },
        )
        .map_err(|error| {
            OperationAdapterError::Runtime(format!("resolve listing product state: {error:?}"))
        })?
        .results;
        let product = match product_rows.as_slice() {
            [] => return Ok(BasketProductResolution::Unresolved),
            [product] => product,
            rows => return Ok(BasketProductResolution::Ambiguous(rows.len())),
        };
        return Ok(BasketProductResolution::Resolved(BasketProductBinState {
            primary_bin_id: product.primary_bin_id.clone(),
            verified_primary_bin_id: product.verified_primary_bin_id.clone(),
        }));
    }

    let Some(listing_lookup) = item.listing.as_deref().and_then(non_empty_ref) else {
        return Ok(BasketProductResolution::Unresolved);
    };
    let lookup_executor =
        SqlxSqliteExecutor::open(&config.local.replica_store_path).map_err(|error| {
            OperationAdapterError::Runtime(format!(
                "open local replica {}: {error}",
                config.local.replica_store_path.display()
            ))
        })?;
    let rows = ReplicaSql::new(lookup_executor)
        .trade_product_lookup(listing_lookup)
        .map_err(|error| {
            OperationAdapterError::Runtime(format!("resolve listing product state: {error:?}"))
        })?;
    let product = match rows.as_slice() {
        [] => return Ok(BasketProductResolution::Unresolved),
        [product] => product,
        rows => return Ok(BasketProductResolution::Ambiguous(rows.len())),
    };
    Ok(BasketProductResolution::Resolved(BasketProductBinState {
        primary_bin_id: product.primary_bin_id.clone(),
        verified_primary_bin_id: product.verified_primary_bin_id.clone(),
    }))
}

fn basket_item_listing_field(item: &BasketItem) -> String {
    if item
        .listing_addr
        .as_deref()
        .and_then(non_empty_ref)
        .is_some()
    {
        format!("basket.items.{}.listing_addr", item.item_id)
    } else {
        format!("basket.items.{}.listing", item.item_id)
    }
}

fn basket_issue(
    code: impl Into<String>,
    field: impl Into<String>,
    message: impl Into<String>,
) -> BasketIssue {
    BasketIssue {
        code: code.into(),
        field: field.into(),
        message: message.into(),
    }
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

fn non_empty_ref(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

fn basket_actions(document: &BasketDocument, issues: &[BasketIssue]) -> Vec<String> {
    let basket_id = document.basket.basket_id.as_str();
    if document.basket.items.is_empty() {
        return vec![format!("radroots basket item add {basket_id}")];
    }
    if issues.is_empty() {
        vec![
            format!("radroots basket validate {basket_id}"),
            format!("radroots basket quote create {basket_id}"),
        ]
    } else {
        vec![format!("radroots basket get {basket_id}")]
    }
}

fn quote_actions(order: &OrderNewView) -> Vec<String> {
    if order.ready_for_submit {
        vec![format!("radroots trade request {}", order.order_id)]
    } else {
        let mut actions = vec![format!("radroots trade get {}", order.order_id)];
        actions.extend(order.actions.iter().cloned());
        actions
    }
}

fn quote_issues_from_order(order: &OrderNewView) -> Vec<BasketIssue> {
    order
        .issues
        .iter()
        .map(|issue| BasketIssue {
            code: issue.code.clone(),
            field: issue.field.clone(),
            message: issue.message.clone(),
        })
        .collect()
}

fn order_adjustments_from_basket(document: &BasketDocument) -> Vec<OrderDraftAdjustmentArgs> {
    document
        .basket
        .adjustments
        .iter()
        .map(|adjustment| OrderDraftAdjustmentArgs {
            id: adjustment.id.clone(),
            effect: adjustment.effect.clone(),
            amount: adjustment.amount.clone(),
            currency: adjustment.currency.clone(),
            reason: adjustment.reason.clone(),
        })
        .collect()
}

fn load_required_basket(
    config: &RuntimeConfig,
    lookup: &str,
    operation_id: &str,
) -> Result<LoadedBasket, OperationAdapterError> {
    load_basket_optional(config, lookup)?.ok_or_else(|| {
        invalid_input(
            operation_id,
            format!("basket `{lookup}` was not found; run `radroots basket create` first"),
        )
    })
}

fn load_basket_optional(
    config: &RuntimeConfig,
    lookup: &str,
) -> Result<Option<LoadedBasket>, OperationAdapterError> {
    let path = basket_lookup_path(config, lookup);
    if !path.exists() {
        return Ok(None);
    }
    load_basket_path(path.as_path()).map(Some)
}

fn load_basket_path(path: &Path) -> Result<LoadedBasket, OperationAdapterError> {
    let contents = fs::read_to_string(path).map_err(|error| {
        OperationAdapterError::Runtime(format!("read basket {}: {error}", path.display()))
    })?;
    let document = serde_json::from_str::<BasketDocument>(contents.as_str()).map_err(|error| {
        OperationAdapterError::Runtime(format!("parse basket {}: {error}", path.display()))
    })?;
    Ok(LoadedBasket {
        file: path.to_path_buf(),
        document,
    })
}

fn save_basket(path: &Path, document: &BasketDocument) -> Result<(), OperationAdapterError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            OperationAdapterError::Runtime(format!(
                "create basket directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let contents = serde_json::to_string_pretty(document)
        .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?;
    fs::write(path, contents).map_err(|error| {
        OperationAdapterError::Runtime(format!("write basket {}: {error}", path.display()))
    })
}

fn baskets_dir(config: &RuntimeConfig) -> PathBuf {
    config.paths.app_data_root.join(BASKETS_DIR)
}

fn basket_lookup_path(config: &RuntimeConfig, lookup: &str) -> PathBuf {
    let candidate = PathBuf::from(lookup);
    if candidate.is_absolute() || lookup.contains(std::path::MAIN_SEPARATOR) {
        return candidate;
    }
    let file_name = if lookup.ends_with(".json") {
        lookup.to_owned()
    } else {
        format!("{lookup}.json")
    };
    baskets_dir(config).join(file_name)
}

fn touch_basket(document: &mut BasketDocument) {
    document.basket.updated_at_unix = now_unix();
}

fn next_item_id(document: &BasketDocument) -> String {
    for index in 1.. {
        let candidate = format!("item_{index}");
        if document
            .basket
            .items
            .iter()
            .all(|item| item.item_id != candidate)
        {
            return candidate;
        }
    }
    unreachable!("unbounded item id search should always return")
}

fn next_basket_id() -> String {
    let sequence = BASKET_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("basket_{}_{}", now_unix(), sequence)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn required_basket_id<P>(request: &OperationRequest<P>) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, "basket_id")
        .or_else(|| string_input(request, "key"))
        .ok_or_else(|| {
            invalid_input(
                request.operation_id(),
                "missing required `basket_id` input".to_owned(),
            )
        })
}

fn required_string<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key).ok_or_else(|| {
        invalid_input(
            request.operation_id(),
            format!("missing required `{key}` input"),
        )
    })
}

fn quantity_input<P>(request: &OperationRequest<P>) -> Result<Option<u32>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    let value = request
        .payload
        .input()
        .get("quantity")
        .or_else(|| request.payload.input().get("bin_count"));
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| {
                invalid_input(
                    request.operation_id(),
                    "`quantity` input must fit in u32".to_owned(),
                )
            }),
        Value::String(value) => value.parse::<u32>().map(Some).map_err(|error| {
            invalid_input(
                request.operation_id(),
                format!("`quantity` input must be a u32: {error}"),
            )
        }),
        _ => Err(invalid_input(
            request.operation_id(),
            "`quantity` input must be a number or string".to_owned(),
        )),
    }
}

fn json_operation_result<R>(value: Value) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    OperationResult::new(R::from_value(value))
}

fn string_input<P>(request: &OperationRequest<P>, key: &str) -> Option<String>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn invalid_input(operation_id: &str, message: String) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message,
    }
}
