use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const TRADE_SUBMIT: OperationSpec = operation!(
    "trade.submit",
    "radroots trade submit",
    "trade",
    "trade_submit",
    "TradeSubmitRequest",
    "TradeSubmitResult",
    "Submit quoted basket as a trade.",
    Buyer,
    true,
    Required,
    Critical,
    false,
    true
);

pub const TRADE_GET: OperationSpec = operation!(
    "trade.get",
    "radroots trade get",
    "trade",
    "trade_get",
    "TradeGetRequest",
    "TradeGetResult",
    "Get trade details.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);

pub const TRADE_LIST: OperationSpec = operation!(
    "trade.list",
    "radroots trade list",
    "trade",
    "trade_list",
    "TradeListRequest",
    "TradeListResult",
    "List trades.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const TRADE_APP_LIST: OperationSpec = operation!(
    "trade.app.list",
    "radroots trade app list",
    "trade",
    "trade_app_list",
    "TradeAppListRequest",
    "TradeAppListResult",
    "List app-authored shared local trade records.",
    Buyer,
    false,
    None,
    Low,
    false,
    false
);

pub const TRADE_APP_EXPORT: OperationSpec = operation!(
    "trade.app.export",
    "radroots trade app export",
    "trade",
    "trade_app_export",
    "TradeAppExportRequest",
    "TradeAppExportResult",
    "Export an app-authored shared trade record as a CLI draft.",
    Buyer,
    true,
    None,
    Medium,
    false,
    true
);

pub const TRADE_REBIND: OperationSpec = operation!(
    "trade.rebind",
    "radroots trade rebind",
    "trade",
    "trade_rebind",
    "TradeRebindRequest",
    "TradeRebindResult",
    "Rebind a local trade draft to an explicit buyer actor.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const TRADE_ACCEPT: OperationSpec = operation!(
    "trade.accept",
    "radroots trade accept",
    "trade",
    "trade_accept",
    "TradeAcceptRequest",
    "TradeAcceptResult",
    "Accept a buyer trade request.",
    Seller,
    true,
    Required,
    Critical,
    false,
    true
);

pub const TRADE_DECLINE: OperationSpec = operation!(
    "trade.decline",
    "radroots trade decline",
    "trade",
    "trade_decline",
    "TradeDeclineRequest",
    "TradeDeclineResult",
    "Decline a buyer trade request.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const TRADE_CANCEL: OperationSpec = operation!(
    "trade.cancel",
    "radroots trade cancel",
    "trade",
    "trade_cancel",
    "TradeCancelRequest",
    "TradeCancelResult",
    "Withdraw a buyer trade before agreement finalization.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const TRADE_REVISION_PROPOSE: OperationSpec = operation!(
    "trade.revision.propose",
    "radroots trade revision propose",
    "trade",
    "trade_revision_propose",
    "TradeRevisionProposeRequest",
    "TradeRevisionProposeResult",
    "Propose seller-authored trade revision.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const TRADE_REVISION_ACCEPT: OperationSpec = operation!(
    "trade.revision.accept",
    "radroots trade revision accept",
    "trade",
    "trade_revision_accept",
    "TradeRevisionAcceptRequest",
    "TradeRevisionAcceptResult",
    "Accept a seller-authored trade revision.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const TRADE_REVISION_DECLINE: OperationSpec = operation!(
    "trade.revision.decline",
    "radroots trade revision decline",
    "trade",
    "trade_revision_decline",
    "TradeRevisionDeclineRequest",
    "TradeRevisionDeclineResult",
    "Decline a seller-authored trade revision.",
    Buyer,
    true,
    Required,
    High,
    false,
    true
);

pub const TRADE_STATUS_GET: OperationSpec = operation!(
    "trade.status.get",
    "radroots trade status get",
    "trade",
    "trade_status_get",
    "TradeStatusGetRequest",
    "TradeStatusGetResult",
    "Get reducer-derived trade status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const TRADE_EVENT_LIST: OperationSpec = operation!(
    "trade.event.list",
    "radroots trade event list",
    "trade",
    "trade_event_list",
    "TradeEventListRequest",
    "TradeEventListResult",
    "List trade events.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const TRADE_EVENT_WATCH: OperationSpec = operation!(
    "trade.event.watch",
    "radroots trade event watch",
    "trade",
    "trade_event_watch",
    "TradeEventWatchRequest",
    "TradeEventWatchResult",
    "Report deferred trade event watch status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);
