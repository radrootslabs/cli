use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const MARKET_REFRESH: OperationSpec = operation!(
    "market.refresh",
    "radroots market refresh",
    "market",
    "market_refresh",
    "MarketRefreshRequest",
    "MarketRefreshResult",
    "Refresh local market projection.",
    Buyer,
    true,
    None,
    Medium,
    true,
    true
);

pub const MARKET_PRODUCT_SEARCH: OperationSpec = operation!(
    "market.product.search",
    "radroots market product search",
    "market",
    "market_product_search",
    "MarketProductSearchRequest",
    "MarketProductSearchResult",
    "Search market products/listings.",
    Buyer,
    false,
    None,
    Low,
    true,
    false
);

pub const MARKET_LISTING_GET: OperationSpec = operation!(
    "market.listing.get",
    "radroots market listing get",
    "market",
    "market_listing_get",
    "MarketListingGetRequest",
    "MarketListingGetResult",
    "Get market listing details.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);
