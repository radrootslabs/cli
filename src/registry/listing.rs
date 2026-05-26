use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const LISTING_CREATE: OperationSpec = operation!(
    "listing.create",
    "radroots listing create",
    "listing",
    "listing_create",
    "ListingCreateRequest",
    "ListingCreateResult",
    "Create seller-owned listing/offer.",
    Seller,
    true,
    None,
    Medium,
    false,
    true
);

pub const LISTING_GET: OperationSpec = operation!(
    "listing.get",
    "radroots listing get",
    "listing",
    "listing_get",
    "ListingGetRequest",
    "ListingGetResult",
    "Get seller-owned listing/offer.",
    Seller,
    false,
    None,
    Low,
    false,
    false
);

pub const LISTING_LIST: OperationSpec = operation!(
    "listing.list",
    "radroots listing list",
    "listing",
    "listing_list",
    "ListingListRequest",
    "ListingListResult",
    "List seller-owned listings/offers.",
    Seller,
    false,
    None,
    Low,
    true,
    false
);

pub const LISTING_APP_LIST: OperationSpec = operation!(
    "listing.app.list",
    "radroots listing app list",
    "listing",
    "listing_app_list",
    "ListingAppListRequest",
    "ListingAppListResult",
    "List app-authored shared local listing records.",
    Seller,
    false,
    None,
    Low,
    false,
    false
);

pub const LISTING_APP_EXPORT: OperationSpec = operation!(
    "listing.app.export",
    "radroots listing app export",
    "listing",
    "listing_app_export",
    "ListingAppExportRequest",
    "ListingAppExportResult",
    "Export an app-authored shared listing record as a CLI draft.",
    Seller,
    true,
    None,
    Medium,
    false,
    true
);

pub const LISTING_UPDATE: OperationSpec = operation!(
    "listing.update",
    "radroots listing update",
    "listing",
    "listing_update",
    "ListingUpdateRequest",
    "ListingUpdateResult",
    "Update general listing fields.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const LISTING_VALIDATE: OperationSpec = operation!(
    "listing.validate",
    "radroots listing validate",
    "listing",
    "listing_validate",
    "ListingValidateRequest",
    "ListingValidateResult",
    "Validate listing for publication or orderability.",
    Seller,
    false,
    None,
    Low,
    false,
    false
);

pub const LISTING_REBIND: OperationSpec = operation!(
    "listing.rebind",
    "radroots listing rebind",
    "listing",
    "listing_rebind",
    "ListingRebindRequest",
    "ListingRebindResult",
    "Rebind a listing draft to an explicit seller actor.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const LISTING_PUBLISH: OperationSpec = operation!(
    "listing.publish",
    "radroots listing publish",
    "listing",
    "listing_publish",
    "ListingPublishRequest",
    "ListingPublishResult",
    "Publish listing externally.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const LISTING_ARCHIVE: OperationSpec = operation!(
    "listing.archive",
    "radroots listing archive",
    "listing",
    "listing_archive",
    "ListingArchiveRequest",
    "ListingArchiveResult",
    "Archive listing resource.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);
