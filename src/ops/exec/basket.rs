use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_events::order::RadrootsOrderEconomics;
use radroots_replica_db::{ReplicaSql, trade_product};
use radroots_replica_db_schema::trade_product::{ITradeProductFieldsFilter, ITradeProductFindMany};
use radroots_sql_core::SqliteExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::global::{OrderDraftAdjustmentArgs, OrderDraftCreateArgs};
use crate::ops::{
    BasketAdjustmentAddRequest, BasketAdjustmentAddResult, BasketAdjustmentRemoveRequest,
    BasketAdjustmentRemoveResult, BasketCreateRequest, BasketCreateResult, BasketGetRequest,
    BasketGetResult, BasketItemAddRequest, BasketItemAddResult, BasketItemRemoveRequest,
    BasketItemRemoveResult, BasketItemUpdateRequest, BasketItemUpdateResult, BasketListRequest,
    BasketListResult, BasketQuoteCreateRequest, BasketQuoteCreateResult, BasketValidateRequest,
    BasketValidateResult, OperationAdapterError, OperationRequest, OperationRequestData,
    OperationRequestPayload, OperationResult, OperationResultData, OperationService,
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

impl OperationService<BasketAdjustmentAddRequest> for BasketOperationService<'_> {
    type Result = BasketAdjustmentAddResult;

    fn execute(
        &self,
        request: OperationRequest<BasketAdjustmentAddRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let adjustment = required_adjustment_from_request(&request)?;
        if loaded
            .document
            .basket
            .adjustments
            .iter()
            .any(|existing| existing.id == adjustment.id)
        {
            return Err(invalid_input(
                request.operation_id(),
                format!("basket adjustment `{}` already exists", adjustment.id),
            ));
        }
        if request.context.dry_run {
            return json_operation_result::<BasketAdjustmentAddResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "adjustment": adjustment,
                "actions": ["radroots basket adjustment add"],
            }));
        }

        loaded.document.basket.adjustments.push(adjustment);
        touch_basket(&mut loaded.document);
        loaded.document.quote = None;
        save_basket(loaded.file.as_path(), &loaded.document)?;
        json_operation_result::<BasketAdjustmentAddResult>(basket_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        )?)
    }
}

impl OperationService<BasketAdjustmentRemoveRequest> for BasketOperationService<'_> {
    type Result = BasketAdjustmentRemoveResult;

    fn execute(
        &self,
        request: OperationRequest<BasketAdjustmentRemoveRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let adjustment_id = required_string(&request, "id")?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let Some(index) = loaded
            .document
            .basket
            .adjustments
            .iter()
            .position(|adjustment| adjustment.id == adjustment_id)
        else {
            return Err(invalid_input(
                request.operation_id(),
                format!("basket adjustment `{adjustment_id}` was not found"),
            ));
        };
        if request.context.dry_run {
            return json_operation_result::<BasketAdjustmentRemoveResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "adjustment_id": adjustment_id,
                "actions": ["radroots basket adjustment remove"],
            }));
        }

        loaded.document.basket.adjustments.remove(index);
        touch_basket(&mut loaded.document);
        loaded.document.quote = None;
        save_basket(loaded.file.as_path(), &loaded.document)?;
        json_operation_result::<BasketAdjustmentRemoveResult>(basket_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        )?)
    }
}

impl OperationService<BasketValidateRequest> for BasketOperationService<'_> {
    type Result = BasketValidateResult;

    fn execute(
        &self,
        request: OperationRequest<BasketValidateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let Some(loaded) = load_basket_optional(self.config, basket_id.as_str())? else {
            return json_operation_result::<BasketValidateResult>(missing_basket_view(
                self.config,
                basket_id.as_str(),
            ));
        };
        json_operation_result::<BasketValidateResult>(basket_validation_view(
            self.config,
            &loaded.document,
            loaded.file.as_path(),
        )?)
    }
}

impl OperationService<BasketQuoteCreateRequest> for BasketOperationService<'_> {
    type Result = BasketQuoteCreateResult;

    fn execute(
        &self,
        request: OperationRequest<BasketQuoteCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let basket_id = required_basket_id(&request)?;
        let mut loaded =
            load_required_basket(self.config, basket_id.as_str(), request.operation_id())?;
        let issues = basket_issues(self.config, &loaded.document)?;
        if !issues.is_empty() {
            let actions = basket_actions(&loaded.document, issues.as_slice());
            return json_operation_result::<BasketQuoteCreateResult>(json!({
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
            return json_operation_result::<BasketQuoteCreateResult>(json!({
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

        json_operation_result::<BasketQuoteCreateResult>(json!({
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

fn required_adjustment_from_request<P>(
    request: &OperationRequest<P>,
) -> Result<BasketAdjustment, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    let id = required_string(request, "id")?.trim().to_owned();
    if id.is_empty() {
        return Err(invalid_input(
            request.operation_id(),
            "`id` must not be empty".to_owned(),
        ));
    }
    let effect = required_string(request, "effect")?.trim().to_owned();
    if effect != "increase" && effect != "decrease" {
        return Err(invalid_input(
            request.operation_id(),
            "`effect` must be increase or decrease".to_owned(),
        ));
    }
    let amount = required_string(request, "amount")?.trim().to_owned();
    let parsed_amount = amount
        .parse::<radroots_core::RadrootsCoreDecimal>()
        .map_err(|_| {
            invalid_input(
                request.operation_id(),
                "`amount` must be a valid decimal value".to_owned(),
            )
        })?;
    if parsed_amount.is_sign_negative() || parsed_amount.is_zero() {
        return Err(invalid_input(
            request.operation_id(),
            "`amount` must be greater than zero".to_owned(),
        ));
    }
    let currency = required_string(request, "currency")?
        .trim()
        .to_ascii_uppercase();
    if radroots_core::RadrootsCoreCurrency::from_str_upper(currency.as_str()).is_err() {
        return Err(invalid_input(
            request.operation_id(),
            "`currency` must be a valid ISO currency code".to_owned(),
        ));
    }
    let reason = required_string(request, "reason")?.trim().to_owned();
    if reason.is_empty() {
        return Err(invalid_input(
            request.operation_id(),
            "`reason` must not be empty".to_owned(),
        ));
    }
    Ok(BasketAdjustment {
        id,
        effect,
        amount,
        currency,
        reason,
    })
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

fn basket_validation_view(
    config: &RuntimeConfig,
    document: &BasketDocument,
    file: &Path,
) -> Result<Value, OperationAdapterError> {
    let issues = basket_issues(config, document)?;
    let ready_for_quote = issues.is_empty();
    let actions = basket_actions(document, issues.as_slice());
    Ok(json!({
        "state": if ready_for_quote { "ready" } else { "unconfigured" },
        "source": BASKET_SOURCE,
        "basket_id": document.basket.basket_id,
        "file": file.display().to_string(),
        "ready_for_quote": ready_for_quote,
        "item_count": document.basket.items.len(),
        "adjustment_count": document.basket.adjustments.len(),
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
    if !config.local.replica_db_path.exists() {
        return Ok(vec![basket_issue(
            "basket_market_replica_missing",
            "local.replica_db",
            "current local replica data is required before quote creation; run `radroots store init` and `radroots market refresh`",
        )]);
    }
    let executor = SqliteExecutor::open(&config.local.replica_db_path).map_err(|error| {
        OperationAdapterError::Runtime(format!(
            "open local replica {}: {error}",
            config.local.replica_db_path.display()
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
                    "basket item listing is not active in the current local replica; run `radroots market refresh` before quote creation",
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
    executor: &SqliteExecutor,
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
    let lookup_executor = SqliteExecutor::open(&config.local.replica_db_path).map_err(|error| {
        OperationAdapterError::Runtime(format!(
            "open local replica {}: {error}",
            config.local.replica_db_path.display()
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
        vec![format!("radroots trade submit {}", order.order_id)]
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_events::RadrootsNostrEvent;
    use radroots_events::ids::RadrootsListingAddress;
    use radroots_events::kinds::{KIND_FARM, KIND_LISTING};
    use radroots_replica_sync::{RadrootsReplicaIngestOutcome, radroots_replica_ingest_event};
    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use radroots_sql_core::{SqlExecutor, SqliteExecutor};
    use serde_json::{Map, Value, json};
    use tempfile::tempdir;

    use super::BasketOperationService;
    use crate::ops::{
        BasketAdjustmentAddRequest, BasketAdjustmentRemoveRequest, BasketCreateRequest,
        BasketGetRequest, BasketItemAddRequest, BasketItemRemoveRequest, BasketItemUpdateRequest,
        BasketListRequest, BasketQuoteCreateRequest, BasketValidateRequest, OperationAdapter,
        OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::account;
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, PublishConfig, PublishTransport, PublishTransportSource, RelayConfig,
        RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig, SignerBackend,
        SignerConfig, Verbosity,
    };

    const LISTING_ADDR: &str = "30402:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAg";

    #[test]
    fn basket_service_creates_gets_and_lists_local_baskets() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        let create = OperationRequest::new(
            OperationContext::default(),
            BasketCreateRequest::from_data(data(&[("basket_id", "basket_test")])),
        )
        .expect("basket create request");
        let create_envelope = service
            .execute(create)
            .expect("basket create result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_create"))
            .expect("basket create envelope");
        assert_eq!(create_envelope.operation_id, "basket.create");
        assert_eq!(create_envelope.result["basket_id"], "basket_test");
        assert_eq!(create_envelope.result["item_count"], 0);

        let get = OperationRequest::new(
            OperationContext::default(),
            BasketGetRequest::from_data(data(&[("basket_id", "basket_test")])),
        )
        .expect("basket get request");
        let get_envelope = service
            .execute(get)
            .expect("basket get result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_get"))
            .expect("basket get envelope");
        assert_eq!(get_envelope.operation_id, "basket.get");
        assert_eq!(get_envelope.result["state"], "ready");

        let list = OperationRequest::new(OperationContext::default(), BasketListRequest::default())
            .expect("basket list request");
        let list_envelope = service
            .execute(list)
            .expect("basket list result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_list"))
            .expect("basket list envelope");
        assert_eq!(list_envelope.operation_id, "basket.list");
        assert_eq!(list_envelope.result["count"], 1);
    }

    #[test]
    fn basket_service_mutates_items_and_validates_readiness() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_items");

        let add = OperationRequest::new(
            OperationContext::default(),
            BasketItemAddRequest::from_data(data(&[
                ("basket_id", "basket_items"),
                ("listing_addr", LISTING_ADDR),
                ("bin_id", "bin-1"),
                ("quantity", "2"),
            ])),
        )
        .expect("basket item add request");
        let add_envelope = service
            .execute(add)
            .expect("basket item add result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_add"))
            .expect("basket item add envelope");
        assert_eq!(add_envelope.operation_id, "basket.item.add");
        assert_eq!(add_envelope.result["item_count"], 1);

        let update = OperationRequest::new(
            OperationContext::default(),
            BasketItemUpdateRequest::from_data(data(&[
                ("basket_id", "basket_items"),
                ("item_id", "item_1"),
                ("quantity", "3"),
            ])),
        )
        .expect("basket item update request");
        let update_envelope = service
            .execute(update)
            .expect("basket item update result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_update"))
            .expect("basket item update envelope");
        assert_eq!(update_envelope.operation_id, "basket.item.update");
        assert_eq!(update_envelope.result["items"][0]["quantity"], 3);

        let validate = OperationRequest::new(
            OperationContext::default(),
            BasketValidateRequest::from_data(data(&[("basket_id", "basket_items")])),
        )
        .expect("basket validate request");
        let validate_envelope = service
            .execute(validate)
            .expect("basket validate result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_validate"))
            .expect("basket validate envelope");
        assert_eq!(validate_envelope.operation_id, "basket.validate");
        assert_eq!(validate_envelope.result["ready_for_quote"], false);
        assert_eq!(
            validate_envelope.result["issues"][0]["code"],
            "basket_market_replica_missing"
        );

        let adjustment_add = OperationRequest::new(
            OperationContext::default(),
            BasketAdjustmentAddRequest::from_data(data(&[
                ("basket_id", "basket_items"),
                ("id", "adj_pickup"),
                ("effect", "decrease"),
                ("amount", "1.00"),
                ("currency", "USD"),
                ("reason", "pickup"),
            ])),
        )
        .expect("basket adjustment add request");
        let adjustment_add_envelope = service
            .execute(adjustment_add)
            .expect("basket adjustment add result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_adjust_add"))
            .expect("basket adjustment add envelope");
        assert_eq!(
            adjustment_add_envelope.operation_id,
            "basket.adjustment.add"
        );
        assert_eq!(adjustment_add_envelope.result["adjustment_count"], 1);

        let adjustment_remove = OperationRequest::new(
            OperationContext::default(),
            BasketAdjustmentRemoveRequest::from_data(data(&[
                ("basket_id", "basket_items"),
                ("id", "adj_pickup"),
            ])),
        )
        .expect("basket adjustment remove request");
        let adjustment_remove_envelope = service
            .execute(adjustment_remove)
            .expect("basket adjustment remove result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_adjust_remove"))
            .expect("basket adjustment remove envelope");
        assert_eq!(
            adjustment_remove_envelope.operation_id,
            "basket.adjustment.remove"
        );
        assert_eq!(adjustment_remove_envelope.result["adjustment_count"], 0);

        let remove = OperationRequest::new(
            OperationContext::default(),
            BasketItemRemoveRequest::from_data(data(&[
                ("basket_id", "basket_items"),
                ("item_id", "item_1"),
            ])),
        )
        .expect("basket item remove request");
        let remove_envelope = service
            .execute(remove)
            .expect("basket item remove result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_remove"))
            .expect("basket item remove envelope");
        assert_eq!(remove_envelope.operation_id, "basket.item.remove");
        assert_eq!(remove_envelope.result["item_count"], 0);
        assert_eq!(remove_envelope.result["ready_for_quote"], false);
    }

    #[test]
    fn basket_quote_create_materializes_order_draft() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        seed_current_listing(&config);
        account::create_or_migrate_default_account(&config).expect("create buyer account");
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_quote");
        add_listing_item(&service, "basket_quote");

        let quote = OperationRequest::new(
            OperationContext::default(),
            BasketQuoteCreateRequest::from_data(data(&[("basket_id", "basket_quote")])),
        )
        .expect("basket quote request");
        let envelope = service
            .execute(quote)
            .expect("basket quote result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_quote"))
            .expect("basket quote envelope");

        assert_eq!(envelope.operation_id, "basket.quote.create");
        assert_eq!(envelope.result["state"], "quoted");
        assert!(
            envelope.result["quote"]["trade_id"]
                .as_str()
                .unwrap()
                .starts_with("ord_")
        );
        assert!(
            envelope.result["trade"]["buyer_account_id"]
                .as_str()
                .expect("buyer account id")
                .len()
                > 8
        );
        assert!(
            envelope.result["trade"]["buyer_pubkey"]
                .as_str()
                .expect("buyer pubkey")
                .len()
                == 64
        );
        assert_eq!(
            envelope.result["trade"]["buyer_actor_source"],
            "resolved_account"
        );
        let order_file = PathBuf::from(envelope.result["quote"]["trade_file"].as_str().unwrap());
        assert!(order_file.exists());
        let draft = std::fs::read_to_string(order_file).expect("read order draft");
        assert!(draft.contains("[buyer_actor]"));
        assert!(draft.contains("source = \"resolved_account\""));
    }

    #[test]
    fn basket_quote_create_dry_run_skips_order_draft() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        seed_current_listing(&config);
        account::create_or_migrate_default_account(&config).expect("create buyer account");
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_dry_run");
        add_listing_item(&service, "basket_dry_run");

        let mut context = OperationContext::default();
        context.dry_run = true;
        let quote = OperationRequest::new(
            context.clone(),
            BasketQuoteCreateRequest::from_data(data(&[("basket_id", "basket_dry_run")])),
        )
        .expect("basket quote request");
        let envelope = service
            .execute(quote)
            .expect("basket quote dry run")
            .to_envelope(context.envelope_context("req_basket_quote"))
            .expect("basket quote envelope");

        assert_eq!(envelope.operation_id, "basket.quote.create");
        assert_eq!(envelope.dry_run, true);
        assert_eq!(envelope.result["state"], "dry_run");
        assert_eq!(envelope.result["trade"]["state"], "dry_run");
        assert_eq!(
            envelope.result["trade"]["buyer_actor_source"],
            "resolved_account"
        );
        assert!(!PathBuf::from(envelope.result["trade"]["file"].as_str().unwrap()).exists());
    }

    #[test]
    fn basket_quote_create_requires_resolved_buyer_account() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        seed_current_listing(&config);
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_no_buyer");
        add_listing_item(&service, "basket_no_buyer");

        let quote = OperationRequest::new(
            OperationContext::default(),
            BasketQuoteCreateRequest::from_data(data(&[("basket_id", "basket_no_buyer")])),
        )
        .expect("basket quote request");
        let error = service.execute(quote).expect_err("missing buyer account");

        let output_error = error.to_output_error();
        assert_eq!(output_error.code, "account_unresolved");
        let detail = output_error.detail.expect("account detail");
        assert_eq!(detail["buyer_actor_source"], "resolved_account");
        assert_eq!(detail["actions"][0], "radroots account create");
    }

    #[test]
    fn basket_readiness_fails_closed_without_replica_data() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_missing_replica");
        let add = add_listing_item(&service, "basket_missing_replica");
        assert_eq!(add.result["ready_for_quote"], false);
        assert_eq!(
            add.result["issues"][0]["code"],
            "basket_market_replica_missing"
        );

        let list = OperationRequest::new(OperationContext::default(), BasketListRequest::default())
            .expect("basket list request");
        let list_envelope = service
            .execute(list)
            .expect("basket list result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_list"))
            .expect("basket list envelope");
        assert_eq!(
            list_envelope.result["baskets"][0]["issues"][0]["code"],
            "basket_market_replica_missing"
        );

        let validate = OperationRequest::new(
            OperationContext::default(),
            BasketValidateRequest::from_data(data(&[("basket_id", "basket_missing_replica")])),
        )
        .expect("basket validate request");
        let validate_envelope = service
            .execute(validate)
            .expect("basket validate result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_validate"))
            .expect("basket validate envelope");
        assert_eq!(validate_envelope.result["state"], "unconfigured");
        assert_eq!(
            validate_envelope.result["issues"][0]["code"],
            "basket_market_replica_missing"
        );

        let quote = OperationRequest::new(
            OperationContext::default(),
            BasketQuoteCreateRequest::from_data(data(&[("basket_id", "basket_missing_replica")])),
        )
        .expect("basket quote request");
        let quote_envelope = service
            .execute(quote)
            .expect("basket quote result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_quote"))
            .expect("basket quote envelope");
        assert_eq!(quote_envelope.result["state"], "unconfigured");
        assert_eq!(
            quote_envelope.result["issues"][0]["code"],
            "basket_market_replica_missing"
        );
        assert!(!config.paths.app_data_root.join("orders/drafts").exists());
    }

    #[test]
    fn basket_readiness_fails_closed_for_unresolved_listing() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        crate::runtime::store::init(&config).expect("store init");
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_unresolved");
        let add = add_listing_item(&service, "basket_unresolved");
        assert_eq!(add.result["ready_for_quote"], false);
        assert_eq!(
            add.result["issues"][0]["code"],
            "basket_item_listing_unresolved"
        );

        let quote = OperationRequest::new(
            OperationContext::default(),
            BasketQuoteCreateRequest::from_data(data(&[("basket_id", "basket_unresolved")])),
        )
        .expect("basket quote request");
        let quote_envelope = service
            .execute(quote)
            .expect("basket quote result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_quote"))
            .expect("basket quote envelope");
        assert_eq!(quote_envelope.result["state"], "unconfigured");
        assert_eq!(
            quote_envelope.result["issues"][0]["code"],
            "basket_item_listing_unresolved"
        );
        assert!(!config.paths.app_data_root.join("orders/drafts").exists());
    }

    #[test]
    fn basket_readiness_fails_closed_for_ambiguous_listing() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        seed_current_listing(&config);
        duplicate_current_listing_row(&config);
        let service = OperationAdapter::new(BasketOperationService::new(&config));
        create_basket(&service, "basket_ambiguous");
        let add = add_listing_item(&service, "basket_ambiguous");
        assert_eq!(add.result["ready_for_quote"], false);
        assert_eq!(
            add.result["issues"][0]["code"],
            "basket_item_listing_ambiguous"
        );

        let quote = OperationRequest::new(
            OperationContext::default(),
            BasketQuoteCreateRequest::from_data(data(&[("basket_id", "basket_ambiguous")])),
        )
        .expect("basket quote request");
        let quote_envelope = service
            .execute(quote)
            .expect("basket quote result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_quote"))
            .expect("basket quote envelope");
        assert_eq!(quote_envelope.result["state"], "unconfigured");
        assert_eq!(
            quote_envelope.result["issues"][0]["code"],
            "basket_item_listing_ambiguous"
        );
        assert!(!config.paths.app_data_root.join("orders/drafts").exists());
    }

    fn create_basket(service: &OperationAdapter<BasketOperationService<'_>>, basket_id: &str) {
        let request = OperationRequest::new(
            OperationContext::default(),
            BasketCreateRequest::from_data(data(&[("basket_id", basket_id)])),
        )
        .expect("basket create request");
        service.execute(request).expect("basket create result");
    }

    fn add_listing_item(
        service: &OperationAdapter<BasketOperationService<'_>>,
        basket_id: &str,
    ) -> crate::out::envelope::OutputEnvelope {
        let request = OperationRequest::new(
            OperationContext::default(),
            BasketItemAddRequest::from_data(data(&[
                ("basket_id", basket_id),
                ("listing_addr", LISTING_ADDR),
                ("bin_id", "bin-1"),
                ("quantity", "1"),
            ])),
        )
        .expect("basket item add request");
        service
            .execute(request)
            .expect("basket item add result")
            .to_envelope(OperationContext::default().envelope_context("req_basket_add"))
            .expect("basket item add envelope")
    }

    fn seed_current_listing(config: &RuntimeConfig) {
        crate::runtime::store::init(config).expect("store init");
        let (seller_pubkey, listing_id) = listing_addr_parts(LISTING_ADDR);
        let event = RadrootsNostrEvent {
            id: "2".repeat(64),
            author: seller_pubkey.clone(),
            created_at: 1,
            kind: KIND_LISTING,
            tags: vec![
                vec!["d".to_owned(), listing_id],
                vec![
                    "a".to_owned(),
                    format!(
                        "{}:{}:{}",
                        KIND_FARM, seller_pubkey, "AAAAAAAAAAAAAAAAAAAAAA"
                    ),
                ],
                vec!["p".to_owned(), seller_pubkey],
                vec!["key".to_owned(), "pasture-eggs".to_owned()],
                vec!["title".to_owned(), "Market Eggs".to_owned()],
                vec!["category".to_owned(), "eggs".to_owned()],
                vec!["summary".to_owned(), "Pasture-raised eggs".to_owned()],
                vec!["process".to_owned(), "washed".to_owned()],
                vec!["lot".to_owned(), "lot-a".to_owned()],
                vec!["profile".to_owned(), "dozen".to_owned()],
                vec!["year".to_owned(), "2026".to_owned()],
                vec!["radroots:primary_bin".to_owned(), "bin-1".to_owned()],
                vec![
                    "radroots:bin".to_owned(),
                    "bin-1".to_owned(),
                    "12".to_owned(),
                    "each".to_owned(),
                    "12".to_owned(),
                    "each".to_owned(),
                    "dozen".to_owned(),
                ],
                vec![
                    "radroots:price".to_owned(),
                    "bin-1".to_owned(),
                    "6".to_owned(),
                    "USD".to_owned(),
                    "1".to_owned(),
                    "each".to_owned(),
                    "6".to_owned(),
                    "each".to_owned(),
                ],
                vec!["inventory".to_owned(), "5".to_owned()],
                vec!["status".to_owned(), "active".to_owned()],
            ],
            content: "# Market Eggs".to_owned(),
            sig: "f".repeat(128),
        };
        let executor = SqliteExecutor::open(&config.local.replica_db_path).expect("open replica");
        assert_eq!(
            radroots_replica_ingest_event(&executor, &event).expect("ingest listing"),
            RadrootsReplicaIngestOutcome::Applied
        );
    }

    fn listing_addr_parts(listing_addr: &str) -> (String, String) {
        let parsed = RadrootsListingAddress::parse(listing_addr).expect("listing addr");
        let (_, rest) = parsed.as_str().split_once(':').expect("listing addr kind");
        let (seller_pubkey, listing_id) = rest.split_once(':').expect("listing addr parts");
        (seller_pubkey.to_owned(), listing_id.to_owned())
    }

    fn duplicate_current_listing_row(config: &RuntimeConfig) {
        let executor = SqliteExecutor::open(&config.local.replica_db_path).expect("open replica");
        let params = json!(["33333333-3333-3333-3333-333333333333", LISTING_ADDR]).to_string();
        executor
            .exec(
                "INSERT INTO trade_product (id, created_at, updated_at, key, category, title, summary, process, lot, profile, year, qty_amt, qty_unit, qty_label, qty_avail, price_amt, price_currency, price_qty_amt, price_qty_unit, notes, listing_addr, primary_bin_id, qty_amt_exact, price_amt_exact, price_qty_amt_exact, verified_primary_bin_id) SELECT ?, created_at, updated_at, key, category, title, summary, process, lot, profile, year, qty_amt, qty_unit, qty_label, qty_avail, price_amt, price_currency, price_qty_amt, price_qty_unit, notes, listing_addr, primary_bin_id, qty_amt_exact, price_amt_exact, price_qty_amt_exact, verified_primary_bin_id FROM trade_product WHERE listing_addr = ?;",
                params.as_str(),
            )
            .expect("duplicate listing row");
    }

    fn sample_config(root: &Path) -> RuntimeConfig {
        let data = root.join("data");
        let cache = root.join("cache");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Normal,
                color: true,
                dry_run: false,
            },
            interaction: InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: false,
                stdout_tty: false,
                prompts_allowed: false,
                confirmations_allowed: false,
            },
            paths: PathsConfig {
                profile: "interactive_user".into(),
                profile_source: "test".into(),
                allowed_profiles: vec!["interactive_user".into(), "repo_local".into()],
                root_source: "test".into(),
                repo_local_root: None,
                repo_local_root_source: None,
                subordinate_path_override_source: "runtime_config".into(),
                app_namespace: "apps/cli".into(),
                shared_accounts_namespace: "shared/accounts".into(),
                shared_identities_namespace: "shared/identities".into(),
                app_config_path: root.join("config/apps/cli/config.toml"),
                workspace_config_path: None,
                app_data_root: data.join("apps/cli"),
                shared_cache_root: cache.clone(),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
            },
            migration: MigrationConfig {
                report: RadrootsMigrationReport::empty(),
            },
            logging: LoggingConfig {
                filter: "info".into(),
                directory: None,
                stdout: false,
            },
            account: AccountConfig {
                selector: None,
                store_path: data.join("shared/accounts/store.json"),
                secrets_dir: secrets.join("shared/accounts"),
                secret_backend: RadrootsSecretBackend::EncryptedFile,
                secret_fallback: None,
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
                default_fallback: Some("encrypted_file".into()),
                allowed_backends: vec!["host_vault".into(), "encrypted_file".into()],
                host_vault_policy: Some("desktop".into()),
                uses_protected_store: true,
            },
            identity: IdentityConfig {
                path: secrets.join("shared/identities/default.json"),
            },
            signer: SignerConfig {
                backend: SignerBackend::Local,
            },
            publish: PublishConfig {
                transport: PublishTransport::DirectNostrRelay,
                source: PublishTransportSource::Defaults,
                radrootsd_proxy: crate::runtime::config::RadrootsdProxyConfig::default(),
            },
            relay: RelayConfig {
                urls: Vec::new(),
                publish_policy: RelayPublishPolicy::Any,
                source: RelayConfigSource::Defaults,
            },
            local: LocalConfig {
                root: data.join("apps/cli/replica"),
                replica_db_path: data.join("apps/cli/replica/replica.sqlite"),
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
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
            },
            rhi: crate::runtime::config::RhiConfig {
                trusted_worker_pubkeys: Vec::new(),
            },
            capability_bindings: Vec::new(),
        }
    }

    fn data(entries: &[(&str, &str)]) -> OperationData {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
            .collect::<Map<String, Value>>()
    }
}
