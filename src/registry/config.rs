use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const CONFIG_GET: OperationSpec = operation!(
    "config.get",
    "radroots config get",
    "config",
    "config_get",
    "ConfigGetRequest",
    "ConfigGetResult",
    "Read effective configuration.",
    Any,
    false,
    None,
    Low,
    true,
    false
);
