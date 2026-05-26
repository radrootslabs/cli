use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const WORKSPACE_INIT: OperationSpec = operation!(
    "workspace.init",
    "radroots workspace init",
    "workspace",
    "workspace_init",
    "WorkspaceInitRequest",
    "WorkspaceInitResult",
    "Initialize a local workspace/profile.",
    Any,
    true,
    None,
    Medium,
    false,
    true
);

pub const WORKSPACE_GET: OperationSpec = operation!(
    "workspace.get",
    "radroots workspace get",
    "workspace",
    "workspace_get",
    "WorkspaceGetRequest",
    "WorkspaceGetResult",
    "Read workspace/profile configuration and current context.",
    Any,
    false,
    None,
    Low,
    false,
    false
);
