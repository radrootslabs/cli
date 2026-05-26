use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const SIGNER_STATUS_GET: OperationSpec = operation!(
    "signer.status.get",
    "radroots signer status get",
    "signer",
    "signer_status_get",
    "SignerStatusGetRequest",
    "SignerStatusGetResult",
    "Get signer availability and permission status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);
