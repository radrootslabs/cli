use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const RELAY_LIST: OperationSpec = operation!(
    "relay.list",
    "radroots relay list",
    "relay",
    "relay_list",
    "RelayListRequest",
    "RelayListResult",
    "List configured relays.",
    Any,
    false,
    None,
    Low,
    true,
    false
);
