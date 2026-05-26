use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const BASKET_CREATE: OperationSpec = operation!(
    "basket.create",
    "radroots basket create",
    "basket",
    "basket_create",
    "BasketCreateRequest",
    "BasketCreateResult",
    "Create local basket.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const BASKET_GET: OperationSpec = operation!(
    "basket.get",
    "radroots basket get",
    "basket",
    "basket_get",
    "BasketGetRequest",
    "BasketGetResult",
    "Get local basket.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);

pub const BASKET_LIST: OperationSpec = operation!(
    "basket.list",
    "radroots basket list",
    "basket",
    "basket_list",
    "BasketListRequest",
    "BasketListResult",
    "List local baskets.",
    Buyer,
    false,
    None,
    Low,
    true,
    false
);

pub const BASKET_ITEM_ADD: OperationSpec = operation!(
    "basket.item.add",
    "radroots basket item add",
    "basket",
    "basket_item_add",
    "BasketItemAddRequest",
    "BasketItemAddResult",
    "Add item to local basket.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const BASKET_ITEM_UPDATE: OperationSpec = operation!(
    "basket.item.update",
    "radroots basket item update",
    "basket",
    "basket_item_update",
    "BasketItemUpdateRequest",
    "BasketItemUpdateResult",
    "Update local basket item.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const BASKET_ITEM_REMOVE: OperationSpec = operation!(
    "basket.item.remove",
    "radroots basket item remove",
    "basket",
    "basket_item_remove",
    "BasketItemRemoveRequest",
    "BasketItemRemoveResult",
    "Remove item from local basket.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const BASKET_ADJUSTMENT_ADD: OperationSpec = operation!(
    "basket.adjustment.add",
    "radroots basket adjustment add",
    "basket",
    "basket_adjustment_add",
    "BasketAdjustmentAddRequest",
    "BasketAdjustmentAddResult",
    "Add buyer basket adjustment.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const BASKET_ADJUSTMENT_REMOVE: OperationSpec = operation!(
    "basket.adjustment.remove",
    "radroots basket adjustment remove",
    "basket",
    "basket_adjustment_remove",
    "BasketAdjustmentRemoveRequest",
    "BasketAdjustmentRemoveResult",
    "Remove buyer basket adjustment.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const BASKET_VALIDATE: OperationSpec = operation!(
    "basket.validate",
    "radroots basket validate",
    "basket",
    "basket_validate",
    "BasketValidateRequest",
    "BasketValidateResult",
    "Validate basket orderability.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);

pub const BASKET_QUOTE_CREATE: OperationSpec = operation!(
    "basket.quote.create",
    "radroots basket quote create",
    "basket",
    "basket_quote_create",
    "BasketQuoteCreateRequest",
    "BasketQuoteCreateResult",
    "Create deterministic basket quote.",
    Buyer,
    true,
    Conditional,
    Medium,
    false,
    true
);
