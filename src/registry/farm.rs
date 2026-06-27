use super::{ApprovalPolicy, OperationRole, OperationSpec, RiskLevel};

pub const FARM_CREATE: OperationSpec = operation!(
    "farm.create",
    "radroots farm create",
    "farm",
    "farm_create",
    "FarmCreateRequest",
    "FarmCreateResult",
    "Create farm identity/profile resource.",
    Seller,
    true,
    None,
    Medium,
    false,
    true
);

pub const FARM_GET: OperationSpec = operation!(
    "farm.get",
    "radroots farm get",
    "farm",
    "farm_get",
    "FarmGetRequest",
    "FarmGetResult",
    "Get farm resource.",
    Seller,
    false,
    None,
    Low,
    false,
    false
);

pub const FARM_REBIND: OperationSpec = operation!(
    "farm.rebind",
    "radroots farm rebind",
    "farm",
    "farm_rebind",
    "FarmRebindRequest",
    "FarmRebindResult",
    "Rebind farm seller account.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const FARM_PROFILE_UPDATE: OperationSpec = operation!(
    "farm.profile.update",
    "radroots farm profile update",
    "farm",
    "farm_profile_update",
    "FarmProfileUpdateRequest",
    "FarmProfileUpdateResult",
    "Update farm public profile fields.",
    Seller,
    true,
    Conditional,
    Medium,
    false,
    true
);

pub const FARM_LOCATION_SET: OperationSpec = operation!(
    "farm.location.set",
    "radroots farm location set --lat 48.429456 --lng -123.349786",
    "farm",
    "farm_location_set",
    "FarmLocationSetRequest",
    "FarmLocationSetResult",
    "Set private farm location and derived public locality.",
    Seller,
    true,
    Conditional,
    Medium,
    true,
    true
);

pub const FARM_LOCATION_GET: OperationSpec = operation!(
    "farm.location.get",
    "radroots farm location get",
    "farm",
    "farm_location_get",
    "FarmLocationGetRequest",
    "FarmLocationGetResult",
    "Get private exact farm location for the configured farm.",
    Seller,
    false,
    None,
    Low,
    false,
    false
);

pub const FARM_LOCATION_CLEAR: OperationSpec = operation!(
    "farm.location.clear",
    "radroots farm location clear",
    "farm",
    "farm_location_clear",
    "FarmLocationClearRequest",
    "FarmLocationClearResult",
    "Clear private exact farm location for the configured farm.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);

pub const FARM_FULFILLMENT_UPDATE: OperationSpec = operation!(
    "farm.fulfillment.update",
    "radroots farm fulfillment update",
    "farm",
    "farm_fulfillment_update",
    "FarmFulfillmentUpdateRequest",
    "FarmFulfillmentUpdateResult",
    "Update farm fulfillment posture.",
    Seller,
    true,
    Conditional,
    Medium,
    false,
    true
);

pub const FARM_READINESS_CHECK: OperationSpec = operation!(
    "farm.readiness.check",
    "radroots farm readiness check",
    "farm",
    "farm_readiness_check",
    "FarmReadinessCheckRequest",
    "FarmReadinessCheckResult",
    "Check whether farm is publish-ready.",
    Seller,
    false,
    None,
    Low,
    false,
    false
);

pub const FARM_PUBLISH: OperationSpec = operation!(
    "farm.publish",
    "radroots farm publish",
    "farm",
    "farm_publish",
    "FarmPublishRequest",
    "FarmPublishResult",
    "Publish farm identity/profile externally.",
    Seller,
    true,
    Required,
    High,
    false,
    true
);
