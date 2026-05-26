use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const ACCOUNT_CREATE: OperationSpec = operation!(
    "account.create",
    "radroots account create",
    "account",
    "account_create",
    "AccountCreateRequest",
    "AccountCreateResult",
    "Create a local account identity.",
    Any,
    true,
    None,
    Medium,
    false,
    true
);

pub const ACCOUNT_IMPORT: OperationSpec = operation!(
    "account.import",
    "radroots account import",
    "account",
    "account_import",
    "AccountImportRequest",
    "AccountImportResult",
    "Import an existing account identity.",
    Any,
    true,
    Required,
    High,
    false,
    true
);

pub const ACCOUNT_ATTACH_SECRET: OperationSpec = operation!(
    "account.attach_secret",
    "radroots account attach-secret",
    "account",
    "account_attach_secret",
    "AccountAttachSecretRequest",
    "AccountAttachSecretResult",
    "Attach local secret custody to an existing account.",
    Any,
    true,
    Required,
    High,
    false,
    true
);

pub const ACCOUNT_GET: OperationSpec = operation!(
    "account.get",
    "radroots account get",
    "account",
    "account_get",
    "AccountGetRequest",
    "AccountGetResult",
    "Get account details.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const ACCOUNT_LIST: OperationSpec = operation!(
    "account.list",
    "radroots account list",
    "account",
    "account_list",
    "AccountListRequest",
    "AccountListResult",
    "List known local accounts.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const ACCOUNT_REMOVE: OperationSpec = operation!(
    "account.remove",
    "radroots account remove",
    "account",
    "account_remove",
    "AccountRemoveRequest",
    "AccountRemoveResult",
    "Remove an account from local configuration/store.",
    Any,
    true,
    Required,
    High,
    false,
    true
);

pub const ACCOUNT_SELECTION_GET: OperationSpec = operation!(
    "account.selection.get",
    "radroots account selection get",
    "account",
    "account_selection_get",
    "AccountSelectionGetRequest",
    "AccountSelectionGetResult",
    "Get selected account context.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const ACCOUNT_SELECTION_UPDATE: OperationSpec = operation!(
    "account.selection.update",
    "radroots account selection update",
    "account",
    "account_selection_update",
    "AccountSelectionUpdateRequest",
    "AccountSelectionUpdateResult",
    "Update selected account context.",
    Any,
    true,
    None,
    Medium,
    false,
    true
);

pub const ACCOUNT_SELECTION_CLEAR: OperationSpec = operation!(
    "account.selection.clear",
    "radroots account selection clear",
    "account",
    "account_selection_clear",
    "AccountSelectionClearRequest",
    "AccountSelectionClearResult",
    "Clear selected account context.",
    Any,
    true,
    None,
    Medium,
    false,
    true
);
