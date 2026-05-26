use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const SYNC_STATUS_GET: OperationSpec = operation!(
    "sync.status.get",
    "radroots sync status get",
    "sync",
    "sync_status_get",
    "SyncStatusGetRequest",
    "SyncStatusGetResult",
    "Read sync status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const SYNC_PULL: OperationSpec = operation!(
    "sync.pull",
    "radroots sync pull",
    "sync",
    "sync_pull",
    "SyncPullRequest",
    "SyncPullResult",
    "Pull remote updates into local store.",
    Any,
    true,
    None,
    Medium,
    true,
    true
);

pub const SYNC_PUSH: OperationSpec = operation!(
    "sync.push",
    "radroots sync push",
    "sync",
    "sync_push",
    "SyncPushRequest",
    "SyncPushResult",
    "Push local signed updates to relays.",
    Any,
    true,
    Required,
    High,
    true,
    true
);

pub const SYNC_WATCH: OperationSpec = operation!(
    "sync.watch",
    "radroots sync watch",
    "sync",
    "sync_watch",
    "SyncWatchRequest",
    "SyncWatchResult",
    "Stream sync events.",
    Any,
    false,
    None,
    Low,
    true,
    false
);
