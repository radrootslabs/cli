use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::runtime::OrderNewView;
use crate::operation_adapter::{
    BasketCreateRequest, BasketCreateResult, BasketGetRequest, BasketGetResult,
    BasketItemAddRequest, BasketItemAddResult, BasketItemRemoveRequest, BasketItemRemoveResult,
    BasketItemUpdateRequest, BasketItemUpdateResult, BasketListRequest, BasketListResult,
    BasketQuoteCreateRequest, BasketQuoteCreateResult, BasketValidateRequest, BasketValidateResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime_args::OrderDraftCreateArgs;

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
struct BasketQuote {
    quote_id: String,
    order_id: String,
    order_file: String,
    ready_for_submit: bool,
    created_at_unix: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    issues: Vec<BasketIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BasketIssue {
    field: String,
    message: String,
}

#[derive(Debug, Clone)]
struct LoadedBasket {
    file: PathBuf,
    document: BasketDocument,
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
        if request.context.dry_run {
            return json_operation_result::<BasketCreateResult>(json!({
                "state": "dry_run",
                "source": BASKET_SOURCE,
                "basket_id": basket_id,
                "item_count": initial_item.as_ref().map(|_| 1).unwrap_or(0),
                "actions": ["radroots basket create"],
            }));
        }

        let file = basket_lookup_path(self.config, basket_id.as_str());
        if file.exists() {
            return Err(invalid_input(
                request.operation_id(),
                format!("basket `{basket_id}` already exists"),
            ));
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
            },
            quote: None,
        };
        save_basket(file.as_path(), &document)?;
        json_operation_result::<BasketCreateResult>(basket_view(&document, file.as_path(), None))
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
            &loaded.document,
            loaded.file.as_path(),
            None,
        ))
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
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        ))
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
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        ))
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
            &loaded.document,
            loaded.file.as_path(),
            Some("updated"),
        ))
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
            &loaded.document,
            loaded.file.as_path(),
        ))
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
        let issues = basket_issues(&loaded.document);
        if !issues.is_empty() {
            return json_operation_result::<BasketQuoteCreateResult>(json!({
                "state": "unconfigured",
                "source": BASKET_QUOTE_SOURCE,
                "basket_id": basket_id,
                "file": loaded.file.display().to_string(),
                "ready_for_quote": false,
                "issues": issues,
                "actions": basket_actions(&loaded.document),
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
            return json_operation_result::<BasketQuoteCreateResult>(json!({
                "state": "dry_run",
                "source": BASKET_QUOTE_SOURCE,
                "basket_id": basket_id,
                "item": item,
                "actions": ["radroots basket quote create"],
            }));
        }

        let order = map_runtime(crate::runtime::order::scaffold(
            self.config,
            &OrderDraftCreateArgs {
                listing: item.listing.clone(),
                listing_addr: item.listing_addr.clone(),
                bin_id: Some(item.bin_id.clone()),
                bin_count: Some(item.quantity),
            },
        ))?;
        let quote = BasketQuote {
            quote_id: format!("quote_{}", loaded.document.basket.basket_id),
            order_id: order.order_id.clone(),
            order_file: order.file.clone(),
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
            "order": order,
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

fn basket_view(document: &BasketDocument, file: &Path, state: Option<&str>) -> Value {
    json!({
        "state": state.unwrap_or("ready"),
        "source": BASKET_SOURCE,
        "basket_id": document.basket.basket_id,
        "file": file.display().to_string(),
        "item_count": document.basket.items.len(),
        "items": document.basket.items,
        "quote": document.quote,
        "ready_for_quote": basket_issues(document).is_empty(),
        "issues": basket_issues(document),
        "actions": basket_actions(document),
    })
}

fn basket_validation_view(document: &BasketDocument, file: &Path) -> Value {
    let issues = basket_issues(document);
    json!({
        "state": if issues.is_empty() { "ready" } else { "unconfigured" },
        "source": BASKET_SOURCE,
        "basket_id": document.basket.basket_id,
        "file": file.display().to_string(),
        "ready_for_quote": issues.is_empty(),
        "item_count": document.basket.items.len(),
        "issues": issues,
        "actions": basket_actions(document),
    })
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
        baskets.push(json!({
            "basket_id": loaded.document.basket.basket_id,
            "state": if basket_issues(&loaded.document).is_empty() { "ready" } else { "unconfigured" },
            "file": loaded.file.display().to_string(),
            "item_count": loaded.document.basket.items.len(),
            "ready_for_quote": basket_issues(&loaded.document).is_empty(),
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

fn basket_issues(document: &BasketDocument) -> Vec<BasketIssue> {
    let mut issues = Vec::new();
    if document.basket.items.is_empty() {
        issues.push(BasketIssue {
            field: "basket.items".to_owned(),
            message: "basket must contain one item before quote creation".to_owned(),
        });
    }
    if document.basket.items.len() > 1 {
        issues.push(BasketIssue {
            field: "basket.items".to_owned(),
            message: "MVP basket quotes support exactly one item".to_owned(),
        });
    }
    for item in &document.basket.items {
        if item.listing.is_none() && item.listing_addr.is_none() {
            issues.push(BasketIssue {
                field: format!("basket.items.{}.listing", item.item_id),
                message: "item must include listing or listing_addr".to_owned(),
            });
        }
        if item.bin_id.trim().is_empty() {
            issues.push(BasketIssue {
                field: format!("basket.items.{}.bin_id", item.item_id),
                message: "item must include bin_id".to_owned(),
            });
        }
        if item.quantity == 0 {
            issues.push(BasketIssue {
                field: format!("basket.items.{}.quantity", item.item_id),
                message: "item quantity must be greater than 0".to_owned(),
            });
        }
    }
    issues
}

fn basket_actions(document: &BasketDocument) -> Vec<String> {
    let basket_id = document.basket.basket_id.as_str();
    if document.basket.items.is_empty() {
        return vec![format!("radroots basket item add {basket_id}")];
    }
    if basket_issues(document).is_empty() {
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
        vec![format!("radroots order submit {}", order.order_id)]
    } else {
        let mut actions = vec![format!("radroots order get {}", order.order_id)];
        actions.extend(order.actions.iter().cloned());
        actions
    }
}

fn quote_issues_from_order(order: &OrderNewView) -> Vec<BasketIssue> {
    order
        .issues
        .iter()
        .map(|issue| BasketIssue {
            field: issue.field.clone(),
            message: issue.message.clone(),
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

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
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

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value};
    use tempfile::tempdir;

    use super::BasketOperationService;
    use crate::operation_adapter::{
        BasketCreateRequest, BasketGetRequest, BasketItemAddRequest, BasketItemRemoveRequest,
        BasketItemUpdateRequest, BasketListRequest, BasketQuoteCreateRequest,
        BasketValidateRequest, OperationAdapter, OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
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
        assert_eq!(validate_envelope.result["ready_for_quote"], true);

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
            envelope.result["quote"]["order_id"]
                .as_str()
                .unwrap()
                .starts_with("ord_")
        );
        assert!(PathBuf::from(envelope.result["quote"]["order_file"].as_str().unwrap()).exists());
    }

    #[test]
    fn basket_quote_create_dry_run_skips_order_draft() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
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
        assert!(envelope.result.get("order").is_none());
    }

    fn create_basket(service: &OperationAdapter<BasketOperationService<'_>>, basket_id: &str) {
        let request = OperationRequest::new(
            OperationContext::default(),
            BasketCreateRequest::from_data(data(&[("basket_id", basket_id)])),
        )
        .expect("basket create request");
        service.execute(request).expect("basket create result");
    }

    fn add_listing_item(service: &OperationAdapter<BasketOperationService<'_>>, basket_id: &str) {
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
        service.execute(request).expect("basket item add result");
    }

    fn sample_config(root: &Path) -> RuntimeConfig {
        let data = root.join("data");
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
                bridge_bearer_token: None,
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
