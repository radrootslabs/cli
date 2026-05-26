use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const VALIDATION_RECEIPT_GET: OperationSpec = operation!(
    "validation.receipt.get",
    "radroots validation receipt get",
    "validation",
    "validation_receipt_get",
    "ValidationReceiptGetRequest",
    "ValidationReceiptGetResult",
    "Fetch and inspect one validation receipt event.",
    Any,
    false,
    None,
    Low,
    false,
    false
);

pub const VALIDATION_RECEIPT_LIST: OperationSpec = operation!(
    "validation.receipt.list",
    "radroots validation receipt list",
    "validation",
    "validation_receipt_list",
    "ValidationReceiptListRequest",
    "ValidationReceiptListResult",
    "List validation receipts for one order.",
    Any,
    false,
    None,
    Low,
    true,
    false
);

pub const VALIDATION_RECEIPT_VERIFY: OperationSpec = operation!(
    "validation.receipt.verify",
    "radroots validation receipt verify",
    "validation",
    "validation_receipt_verify",
    "ValidationReceiptVerifyRequest",
    "ValidationReceiptVerifyResult",
    "Verify validation receipt tags, payload, and proof binding.",
    Any,
    false,
    None,
    Low,
    false,
    false
);
