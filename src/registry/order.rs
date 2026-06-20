use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const ORDER_SUBMIT: OperationSpec = operation!(
    "order.submit",
    "radroots order submit",
    "order",
    "order_submit",
    "OrderSubmitRequest",
    "OrderSubmitResult",
    "Submit quoted basket as an order.",
    Buyer,
    true,
    Required,
    Critical,
    false,
    true
);

pub const ORDER_GET: OperationSpec = operation!(
    "order.get",
    "radroots order get",
    "order",
    "order_get",
    "OrderGetRequest",
    "OrderGetResult",
    "Get order details.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);

pub const ORDER_LIST: OperationSpec = operation!(
    "order.list",
    "radroots order list",
    "order",
    "order_list",
    "OrderListRequest",
    "OrderListResult",
    "List orders.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const ORDER_APP_LIST: OperationSpec = operation!(
    "order.app.list",
    "radroots order app list",
    "order",
    "order_app_list",
    "OrderAppListRequest",
    "OrderAppListResult",
    "List app-authored shared local order records.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);

pub const ORDER_APP_EXPORT: OperationSpec = operation!(
    "order.app.export",
    "radroots order app export",
    "order",
    "order_app_export",
    "OrderAppExportRequest",
    "OrderAppExportResult",
    "Export an app-authored shared order record as a CLI draft.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const ORDER_REBIND: OperationSpec = operation!(
    "order.rebind",
    "radroots order rebind",
    "order",
    "order_rebind",
    "OrderRebindRequest",
    "OrderRebindResult",
    "Rebind a local order draft to an explicit buyer actor.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const ORDER_ACCEPT: OperationSpec = operation!(
    "order.accept",
    "radroots order accept",
    "order",
    "order_accept",
    "OrderAcceptRequest",
    "OrderAcceptResult",
    "Accept a buyer order request.",
    Seller,
    true,
    Required,
    Critical,
    false,
    true
);

pub const ORDER_DECLINE: OperationSpec = operation!(
    "order.decline",
    "radroots order decline",
    "order",
    "order_decline",
    "OrderDeclineRequest",
    "OrderDeclineResult",
    "Decline a buyer order request.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const ORDER_CANCEL: OperationSpec = operation!(
    "order.cancel",
    "radroots order cancel",
    "order",
    "order_cancel",
    "OrderCancelRequest",
    "OrderCancelResult",
    "Cancel a buyer order before fulfillment.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const ORDER_REVISION_PROPOSE: OperationSpec = operation!(
    "order.revision.propose",
    "radroots order revision propose",
    "order",
    "order_revision_propose",
    "OrderRevisionProposeRequest",
    "OrderRevisionProposeResult",
    "Propose seller-authored order revision.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const ORDER_REVISION_ACCEPT: OperationSpec = operation!(
    "order.revision.accept",
    "radroots order revision accept",
    "order",
    "order_revision_accept",
    "OrderRevisionAcceptRequest",
    "OrderRevisionAcceptResult",
    "Accept a seller-authored order revision.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const ORDER_REVISION_DECLINE: OperationSpec = operation!(
    "order.revision.decline",
    "radroots order revision decline",
    "order",
    "order_revision_decline",
    "OrderRevisionDeclineRequest",
    "OrderRevisionDeclineResult",
    "Decline a seller-authored order revision.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const ORDER_STATUS_GET: OperationSpec = operation!(
    "order.status.get",
    "radroots order status get",
    "order",
    "order_status_get",
    "OrderStatusGetRequest",
    "OrderStatusGetResult",
    "Get reducer-derived order status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const ORDER_EVENT_LIST: OperationSpec = operation!(
    "order.event.list",
    "radroots order event list",
    "order",
    "order_event_list",
    "OrderEventListRequest",
    "OrderEventListResult",
    "List order events.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const ORDER_EVENT_WATCH: OperationSpec = operation!(
    "order.event.watch",
    "radroots order event watch",
    "order",
    "order_event_watch",
    "OrderEventWatchRequest",
    "OrderEventWatchResult",
    "Report deferred order event watch status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);
