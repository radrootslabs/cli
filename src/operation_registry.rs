#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub operation_id: &'static str,
    pub cli_path: &'static str,
    pub namespace: &'static str,
    pub mcp_tool: &'static str,
    pub rust_request: &'static str,
    pub rust_result: &'static str,
    pub json_kind: &'static str,
    pub description: &'static str,
    pub role: OperationRole,
    pub mutates: bool,
    pub approval_policy: ApprovalPolicy,
    pub risk_level: RiskLevel,
    pub supports_json: bool,
    pub supports_ndjson: bool,
    pub supports_dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    None,
    Conditional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationRole {
    Any,
    Buyer,
    Seller,
}

macro_rules! operation {
    (
        $operation_id:literal,
        $cli_path:literal,
        $namespace:literal,
        $mcp_tool:literal,
        $rust_request:literal,
        $rust_result:literal,
        $description:literal,
        $role:ident,
        $mutates:literal,
        $approval_policy:ident,
        $risk_level:ident,
        $supports_ndjson:literal,
        $supports_dry_run:literal
    ) => {
        OperationSpec {
            operation_id: $operation_id,
            cli_path: $cli_path,
            namespace: $namespace,
            mcp_tool: $mcp_tool,
            rust_request: $rust_request,
            rust_result: $rust_result,
            json_kind: $operation_id,
            description: $description,
            role: OperationRole::$role,
            mutates: $mutates,
            approval_policy: ApprovalPolicy::$approval_policy,
            risk_level: RiskLevel::$risk_level,
            supports_json: true,
            supports_ndjson: $supports_ndjson,
            supports_dry_run: $supports_dry_run,
        }
    };
}

pub const OPERATION_REGISTRY: &[OperationSpec] = &[
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
        false,
        false
    ),
    operation!(
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
        false,
        false
    ),
    operation!(
        "config.get",
        "radroots config get",
        "config",
        "config_get",
        "ConfigGetRequest",
        "ConfigGetResult",
        "Read effective configuration.",
        Any,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
        "signer.status.get",
        "radroots signer status get",
        "signer",
        "signer_status_get",
        "SignerStatusGetRequest",
        "SignerStatusGetResult",
        "Get signer availability and permission status.",
        Any,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "relay.list",
        "radroots relay list",
        "relay",
        "relay_list",
        "RelayListRequest",
        "RelayListResult",
        "List configured relays.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
        "sync.push",
        "radroots sync push",
        "sync",
        "sync_push",
        "SyncPushRequest",
        "SyncPushResult",
        "Push local signed updates to relays.",
        Any,
        true,
        Conditional,
        High,
        true,
        true
    ),
    operation!(
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
    ),
    operation!(
        "runtime.status.get",
        "radroots runtime status get",
        "runtime",
        "runtime_status_get",
        "RuntimeStatusGetRequest",
        "RuntimeStatusGetResult",
        "Get runtime status.",
        Any,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "runtime.start",
        "radroots runtime start",
        "runtime",
        "runtime_start",
        "RuntimeStartRequest",
        "RuntimeStartResult",
        "Start runtime.",
        Any,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "runtime.stop",
        "radroots runtime stop",
        "runtime",
        "runtime_stop",
        "RuntimeStopRequest",
        "RuntimeStopResult",
        "Stop runtime.",
        Any,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "runtime.restart",
        "radroots runtime restart",
        "runtime",
        "runtime_restart",
        "RuntimeRestartRequest",
        "RuntimeRestartResult",
        "Restart runtime.",
        Any,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "runtime.log.watch",
        "radroots runtime log watch",
        "runtime",
        "runtime_log_watch",
        "RuntimeLogWatchRequest",
        "RuntimeLogWatchResult",
        "Stream runtime logs.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
        "runtime.config.get",
        "radroots runtime config get",
        "runtime",
        "runtime_config_get",
        "RuntimeConfigGetRequest",
        "RuntimeConfigGetResult",
        "Read runtime configuration.",
        Any,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "job.get",
        "radroots job get",
        "job",
        "job_get",
        "JobGetRequest",
        "JobGetResult",
        "Get job details.",
        Any,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "job.list",
        "radroots job list",
        "job",
        "job_list",
        "JobListRequest",
        "JobListResult",
        "List jobs.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
        "job.watch",
        "radroots job watch",
        "job",
        "job_watch",
        "JobWatchRequest",
        "JobWatchResult",
        "Stream job events.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
        "farm.location.update",
        "radroots farm location update",
        "farm",
        "farm_location_update",
        "FarmLocationUpdateRequest",
        "FarmLocationUpdateResult",
        "Update farm location fields.",
        Seller,
        true,
        Conditional,
        Medium,
        false,
        true
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
        "listing.update",
        "radroots listing update",
        "listing",
        "listing_update",
        "ListingUpdateRequest",
        "ListingUpdateResult",
        "Update general listing fields.",
        Seller,
        true,
        Conditional,
        Medium,
        false,
        true
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
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
    ),
    operation!(
        "market.refresh",
        "radroots market refresh",
        "market",
        "market_refresh",
        "MarketRefreshRequest",
        "MarketRefreshResult",
        "Refresh local market projection.",
        Buyer,
        true,
        None,
        Medium,
        true,
        true
    ),
    operation!(
        "market.product.search",
        "radroots market product search",
        "market",
        "market_product_search",
        "MarketProductSearchRequest",
        "MarketProductSearchResult",
        "Search market products/listings.",
        Buyer,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
        "market.listing.get",
        "radroots market listing get",
        "market",
        "market_listing_get",
        "MarketListingGetRequest",
        "MarketListingGetResult",
        "Get market listing details.",
        Buyer,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "basket.create",
        "radroots basket create",
        "basket",
        "basket_create",
        "BasketCreateRequest",
        "BasketCreateResult",
        "Create local basket.",
        Buyer,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "basket.get",
        "radroots basket get",
        "basket",
        "basket_get",
        "BasketGetRequest",
        "BasketGetResult",
        "Get local basket.",
        Buyer,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "basket.list",
        "radroots basket list",
        "basket",
        "basket_list",
        "BasketListRequest",
        "BasketListResult",
        "List local baskets.",
        Buyer,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
        "basket.item.add",
        "radroots basket item add",
        "basket",
        "basket_item_add",
        "BasketItemAddRequest",
        "BasketItemAddResult",
        "Add item to local basket.",
        Buyer,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "basket.item.update",
        "radroots basket item update",
        "basket",
        "basket_item_update",
        "BasketItemUpdateRequest",
        "BasketItemUpdateResult",
        "Update local basket item.",
        Buyer,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "basket.item.remove",
        "radroots basket item remove",
        "basket",
        "basket_item_remove",
        "BasketItemRemoveRequest",
        "BasketItemRemoveResult",
        "Remove item from local basket.",
        Buyer,
        true,
        None,
        Medium,
        false,
        true
    ),
    operation!(
        "basket.validate",
        "radroots basket validate",
        "basket",
        "basket_validate",
        "BasketValidateRequest",
        "BasketValidateResult",
        "Validate basket orderability.",
        Buyer,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "basket.quote.create",
        "radroots basket quote create",
        "basket",
        "basket_quote_create",
        "BasketQuoteCreateRequest",
        "BasketQuoteCreateResult",
        "Create deterministic basket quote.",
        Buyer,
        true,
        Conditional,
        Medium,
        false,
        true
    ),
    operation!(
        "order.submit",
        "radroots order submit",
        "order",
        "order_submit",
        "OrderSubmitRequest",
        "OrderSubmitResult",
        "Submit quoted basket as an order.",
        Buyer,
        true,
        Required,
        Critical,
        false,
        true
    ),
    operation!(
        "order.get",
        "radroots order get",
        "order",
        "order_get",
        "OrderGetRequest",
        "OrderGetResult",
        "Get order details.",
        Buyer,
        false,
        None,
        Low,
        false,
        false
    ),
    operation!(
        "order.list",
        "radroots order list",
        "order",
        "order_list",
        "OrderListRequest",
        "OrderListResult",
        "List orders.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
        "order.event.list",
        "radroots order event list",
        "order",
        "order_event_list",
        "OrderEventListRequest",
        "OrderEventListResult",
        "List order events.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
    operation!(
        "order.event.watch",
        "radroots order event watch",
        "order",
        "order_event_watch",
        "OrderEventWatchRequest",
        "OrderEventWatchResult",
        "Stream order events.",
        Any,
        false,
        None,
        Low,
        true,
        false
    ),
];

pub fn get_operation(operation_id: &str) -> Option<&'static OperationSpec> {
    OPERATION_REGISTRY
        .iter()
        .find(|operation| operation.operation_id == operation_id)
}

pub fn registry_linkage_is_valid() -> bool {
    OPERATION_REGISTRY.iter().all(|operation| {
        get_operation(operation.operation_id).is_some()
            && operation.operation_id == operation.json_kind
            && operation.mcp_tool == operation.operation_id.replace('.', "_")
            && operation.supports_json
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ApprovalPolicy, OPERATION_REGISTRY, OperationRole, RiskLevel, get_operation};

    const EXPECTED_OPERATION_IDS: &[&str] = &[
        "workspace.init",
        "workspace.get",
        "health.status.get",
        "health.check.run",
        "config.get",
        "account.create",
        "account.import",
        "account.get",
        "account.list",
        "account.remove",
        "account.selection.get",
        "account.selection.update",
        "account.selection.clear",
        "signer.status.get",
        "relay.list",
        "store.init",
        "store.status.get",
        "store.export",
        "store.backup.create",
        "sync.status.get",
        "sync.pull",
        "sync.push",
        "sync.watch",
        "runtime.status.get",
        "runtime.start",
        "runtime.stop",
        "runtime.restart",
        "runtime.log.watch",
        "runtime.config.get",
        "job.get",
        "job.list",
        "job.watch",
        "farm.create",
        "farm.get",
        "farm.profile.update",
        "farm.location.update",
        "farm.fulfillment.update",
        "farm.readiness.check",
        "farm.publish",
        "listing.create",
        "listing.get",
        "listing.list",
        "listing.update",
        "listing.validate",
        "listing.publish",
        "listing.archive",
        "market.refresh",
        "market.product.search",
        "market.listing.get",
        "basket.create",
        "basket.get",
        "basket.list",
        "basket.item.add",
        "basket.item.update",
        "basket.item.remove",
        "basket.validate",
        "basket.quote.create",
        "order.submit",
        "order.get",
        "order.list",
        "order.event.list",
        "order.event.watch",
    ];

    #[test]
    fn registry_contains_exact_target_operation_set() {
        let actual = operation_ids();
        let expected = EXPECTED_OPERATION_IDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
        assert_eq!(OPERATION_REGISTRY.len(), 62);
    }

    #[test]
    fn registry_identity_fields_are_consistent() {
        let mut operation_ids = BTreeSet::new();
        let mut cli_paths = BTreeSet::new();
        let mut mcp_tools = BTreeSet::new();

        for operation in OPERATION_REGISTRY {
            assert!(operation_ids.insert(operation.operation_id));
            assert!(cli_paths.insert(operation.cli_path));
            assert!(mcp_tools.insert(operation.mcp_tool));
            assert_eq!(operation.operation_id, operation.json_kind);
            assert_eq!(operation.mcp_tool, operation.operation_id.replace('.', "_"));
            assert!(operation.cli_path.starts_with("radroots "));
            assert_eq!(
                operation.namespace,
                operation.operation_id.split('.').next().unwrap()
            );
            assert_eq!(
                operation.rust_request,
                format!("{}Request", pascal_case(operation.operation_id))
            );
            assert_eq!(
                operation.rust_result,
                format!("{}Result", pascal_case(operation.operation_id))
            );
            assert!(operation.supports_json);
            assert!(!operation.description.is_empty());
        }
    }

    #[test]
    fn registry_policy_invariants_hold() {
        let required = OPERATION_REGISTRY
            .iter()
            .filter(|operation| operation.approval_policy == ApprovalPolicy::Required)
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected_required = [
            "account.import",
            "account.remove",
            "farm.publish",
            "listing.publish",
            "listing.archive",
            "order.submit",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(required, expected_required);

        for operation in OPERATION_REGISTRY {
            if operation.mutates {
                assert!(operation.supports_dry_run, "{}", operation.operation_id);
            }

            if operation.approval_policy == ApprovalPolicy::Required {
                assert!(
                    matches!(operation.risk_level, RiskLevel::High | RiskLevel::Critical),
                    "{}",
                    operation.operation_id
                );
            }
        }
    }

    #[test]
    fn registry_ndjson_support_is_explicit() {
        let actual = OPERATION_REGISTRY
            .iter()
            .filter(|operation| operation.supports_ndjson)
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected = [
            "account.list",
            "relay.list",
            "sync.pull",
            "sync.push",
            "sync.watch",
            "runtime.log.watch",
            "job.list",
            "job.watch",
            "listing.list",
            "market.refresh",
            "market.product.search",
            "basket.list",
            "order.list",
            "order.event.list",
            "order.event.watch",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn deferred_namespaces_are_absent() {
        let namespaces = OPERATION_REGISTRY
            .iter()
            .map(|operation| operation.namespace)
            .collect::<BTreeSet<_>>();

        assert!(!namespaces.contains("product"));
        assert!(!namespaces.contains("message"));
        assert!(!namespaces.contains("approval"));
        assert!(!namespaces.contains("agent"));
    }

    #[test]
    fn roles_are_assigned_to_marketplace_operations() {
        assert_eq!(
            get_operation("listing.publish").unwrap().role,
            OperationRole::Seller
        );
        assert_eq!(
            get_operation("basket.quote.create").unwrap().role,
            OperationRole::Buyer
        );
        assert_eq!(
            get_operation("order.list").unwrap().role,
            OperationRole::Any
        );
    }

    fn operation_ids() -> BTreeSet<&'static str> {
        OPERATION_REGISTRY
            .iter()
            .map(|operation| operation.operation_id)
            .collect()
    }

    fn pascal_case(operation_id: &str) -> String {
        operation_id
            .split('.')
            .flat_map(|part| part.split('_'))
            .map(|part| {
                let mut chars = part.chars();
                let first = chars.next().unwrap().to_ascii_uppercase();
                format!("{first}{}", chars.as_str())
            })
            .collect()
    }
}
