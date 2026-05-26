use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const HEALTH_STATUS_GET: OperationSpec = operation!(
    "health.status.get",
    "radroots health status get",
    "health",
    "health_status_get",
    "HealthStatusGetRequest",
    "HealthStatusGetResult",
    "Get concise health and readiness status.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const HEALTH_CHECK_RUN: OperationSpec = operation!(
    "health.check.run",
    "radroots health check run",
    "health",
    "health_check_run",
    "HealthCheckRunRequest",
    "HealthCheckRunResult",
    "Run comprehensive diagnostics.",
    Any,
    false,
    None,
    Low,
    true,
    false
);
