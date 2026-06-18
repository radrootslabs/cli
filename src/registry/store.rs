use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const STORE_INIT: OperationSpec = operation!(
    "store.init",
    "radroots store init",
    "store",
    "store_init",
    "StoreInitRequest",
    "StoreInitResult",
    "Initialize local store.",
    Any,
    true,
    None,
    Medium,
    false,
    true
);

pub const STORE_STATUS_GET: OperationSpec = operation!(
    "store.status.get",
    "radroots store status get",
    "store",
    "store_status_get",
    "StoreStatusGetRequest",
    "StoreStatusGetResult",
    "Read local store status.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const STORE_EXPORT: OperationSpec = operation!(
    "store.export",
    "radroots store export",
    "store",
    "store_export",
    "StoreExportRequest",
    "StoreExportResult",
    "Export local store data according to filters/policy.",
    Any,
    false,
    Conditional,
    Medium,
    false,
    false
);

pub const STORE_BACKUP_CREATE: OperationSpec = operation!(
    "store.backup.create",
    "radroots store backup create",
    "store",
    "store_backup_create",
    "StoreBackupCreateRequest",
    "StoreBackupCreateResult",
    "Create local store backup.",
    Any,
    true,
    None,
    Medium,
    false,
    true
);

pub const STORE_BACKUP_RESTORE: OperationSpec = operation!(
    "store.backup.restore",
    "radroots store backup restore sdk-store-backup",
    "store",
    "store_backup_restore",
    "StoreBackupRestoreRequest",
    "StoreBackupRestoreResult",
    "Restore SDK canonical store backup.",
    Any,
    true,
    Conditional,
    High,
    false,
    true
);
