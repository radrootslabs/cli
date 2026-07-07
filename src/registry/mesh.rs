use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const MESH_SCOPE_GET: OperationSpec = operation!(
    "mesh.scope.get",
    "radroots mesh scope get",
    "mesh",
    "mesh_scope_get",
    "MeshScopeGetRequest",
    "MeshScopeGetResult",
    "Read the configured mesh scope.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const MESH_SCOPE_SET: OperationSpec = operation!(
    "mesh.scope.set",
    "radroots mesh scope set --scope disabled",
    "mesh",
    "mesh_scope_set",
    "MeshScopeSetRequest",
    "MeshScopeSetResult",
    "Write the configured mesh scope.",
    Any,
    true,
    Required,
    High,
    false,
    true
);

pub const MESH_STATUS: OperationSpec = operation!(
    "mesh.status",
    "radroots mesh status",
    "mesh",
    "mesh_status",
    "MeshStatusRequest",
    "MeshStatusResult",
    "Read mesh implementation status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const MESH_POLICY_CHECK: OperationSpec = operation!(
    "mesh.policy.check",
    "radroots mesh policy check",
    "mesh",
    "mesh_policy_check",
    "MeshPolicyCheckRequest",
    "MeshPolicyCheckResult",
    "Evaluate mesh delivery policy for the active preview state.",
    Any,
    false,
    None,
    Low,
    false,
    false
);
