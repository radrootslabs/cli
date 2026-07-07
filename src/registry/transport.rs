use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const TRANSPORT_PROFILE_GET: OperationSpec = operation!(
    "transport.profile.get",
    "radroots transport profile get",
    "transport",
    "transport_profile_get",
    "TransportProfileGetRequest",
    "TransportProfileGetResult",
    "Read the active transport profile.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const TRANSPORT_PROFILE_SET: OperationSpec = operation!(
    "transport.profile.set",
    "radroots transport profile set --kind local-only",
    "transport",
    "transport_profile_set",
    "TransportProfileSetRequest",
    "TransportProfileSetResult",
    "Write the active transport profile.",
    Any,
    true,
    Required,
    High,
    false,
    true
);

pub const TRANSPORT_STATUS: OperationSpec = operation!(
    "transport.status",
    "radroots transport status",
    "transport",
    "transport_status",
    "TransportStatusRequest",
    "TransportStatusResult",
    "Read transport implementation readiness.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const TRANSPORT_OUTBOX_STATUS: OperationSpec = operation!(
    "transport.outbox.status",
    "radroots transport outbox status",
    "transport",
    "transport_outbox_status",
    "TransportOutboxStatusRequest",
    "TransportOutboxStatusResult",
    "Read SDK transport outbox status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const TRANSPORT_OUTBOX_PUSH: OperationSpec = operation!(
    "transport.outbox.push",
    "radroots transport outbox push",
    "transport",
    "transport_outbox_push",
    "TransportOutboxPushRequest",
    "TransportOutboxPushResult",
    "Push ready SDK outbox events through the active transport profile.",
    Any,
    true,
    Required,
    High,
    true,
    true
);
