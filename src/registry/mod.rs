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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkRequirement {
    Local,
    External { dry_run_requires_network: bool },
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

mod account;
mod basket;
mod config;
mod farm;
mod health;
mod listing;
mod market;
mod order;
mod relay;
mod signer;
mod store;
mod sync;
mod validation;
mod workspace;

pub const OPERATION_REGISTRY: &[OperationSpec] = &[
    workspace::WORKSPACE_INIT,
    workspace::WORKSPACE_GET,
    health::HEALTH_STATUS_GET,
    health::HEALTH_CHECK_RUN,
    config::CONFIG_GET,
    account::ACCOUNT_CREATE,
    account::ACCOUNT_IMPORT,
    account::ACCOUNT_ATTACH_SECRET,
    account::ACCOUNT_GET,
    account::ACCOUNT_LIST,
    account::ACCOUNT_REMOVE,
    account::ACCOUNT_SELECTION_GET,
    account::ACCOUNT_SELECTION_UPDATE,
    account::ACCOUNT_SELECTION_CLEAR,
    signer::SIGNER_STATUS_GET,
    relay::RELAY_LIST,
    store::STORE_INIT,
    store::STORE_STATUS_GET,
    store::STORE_EXPORT,
    store::STORE_BACKUP_CREATE,
    store::STORE_BACKUP_RESTORE,
    sync::SYNC_STATUS_GET,
    sync::SYNC_PULL,
    sync::SYNC_PUSH,
    sync::SYNC_WATCH,
    farm::FARM_CREATE,
    farm::FARM_GET,
    farm::FARM_REBIND,
    farm::FARM_PROFILE_UPDATE,
    farm::FARM_LOCATION_UPDATE,
    farm::FARM_FULFILLMENT_UPDATE,
    farm::FARM_READINESS_CHECK,
    farm::FARM_PUBLISH,
    listing::LISTING_CREATE,
    listing::LISTING_GET,
    listing::LISTING_LIST,
    listing::LISTING_APP_LIST,
    listing::LISTING_APP_EXPORT,
    listing::LISTING_UPDATE,
    listing::LISTING_VALIDATE,
    listing::LISTING_REBIND,
    listing::LISTING_PUBLISH,
    listing::LISTING_ARCHIVE,
    market::MARKET_REFRESH,
    market::MARKET_PRODUCT_SEARCH,
    market::MARKET_LISTING_GET,
    basket::BASKET_CREATE,
    basket::BASKET_GET,
    basket::BASKET_LIST,
    basket::BASKET_ITEM_ADD,
    basket::BASKET_ITEM_UPDATE,
    basket::BASKET_ITEM_REMOVE,
    basket::BASKET_ADJUSTMENT_ADD,
    basket::BASKET_ADJUSTMENT_REMOVE,
    basket::BASKET_VALIDATE,
    basket::BASKET_QUOTE_CREATE,
    order::ORDER_SUBMIT,
    order::ORDER_GET,
    order::ORDER_LIST,
    order::ORDER_APP_LIST,
    order::ORDER_APP_EXPORT,
    order::ORDER_REBIND,
    order::ORDER_ACCEPT,
    order::ORDER_DECLINE,
    order::ORDER_CANCEL,
    order::ORDER_REVISION_PROPOSE,
    order::ORDER_REVISION_ACCEPT,
    order::ORDER_REVISION_DECLINE,
    order::ORDER_FULFILLMENT_UPDATE,
    order::ORDER_RECEIPT_RECORD,
    order::ORDER_PAYMENT_RECORD,
    order::ORDER_SETTLEMENT_ACCEPT,
    order::ORDER_SETTLEMENT_REJECT,
    order::ORDER_STATUS_GET,
    order::ORDER_EVENT_LIST,
    order::ORDER_EVENT_WATCH,
    validation::VALIDATION_RECEIPT_GET,
    validation::VALIDATION_RECEIPT_LIST,
    validation::VALIDATION_RECEIPT_VERIFY,
];

pub fn get_operation(operation_id: &str) -> Option<&'static OperationSpec> {
    OPERATION_REGISTRY
        .iter()
        .find(|operation| operation.operation_id == operation_id)
}

pub fn network_requirement(operation_id: &str) -> NetworkRequirement {
    match operation_id {
        "sync.pull"
        | "sync.push"
        | "sync.watch"
        | "market.refresh"
        | "farm.publish"
        | "listing.publish"
        | "listing.update"
        | "listing.archive"
        | "order.submit"
        | "order.event.list"
        | "validation.receipt.get"
        | "validation.receipt.list"
        | "validation.receipt.verify" => NetworkRequirement::External {
            dry_run_requires_network: false,
        },
        "order.accept"
        | "order.decline"
        | "order.cancel"
        | "order.revision.propose"
        | "order.revision.accept"
        | "order.revision.decline"
        | "order.fulfillment.update"
        | "order.receipt.record" => NetworkRequirement::External {
            dry_run_requires_network: true,
        },
        _ => NetworkRequirement::Local,
    }
}

pub fn requires_local_signer_mode(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "signer.status.get"
            | "farm.publish"
            | "sync.push"
            | "listing.publish"
            | "listing.update"
            | "listing.archive"
            | "order.submit"
            | "order.accept"
            | "order.decline"
            | "order.cancel"
            | "order.revision.propose"
            | "order.revision.accept"
            | "order.revision.decline"
            | "order.fulfillment.update"
            | "order.receipt.record"
    )
}

pub fn requires_nostr_relay_publish_mode(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "sync.push"
            | "farm.publish"
            | "listing.publish"
            | "listing.update"
            | "listing.archive"
            | "order.submit"
            | "order.accept"
            | "order.decline"
            | "order.cancel"
            | "order.revision.propose"
            | "order.revision.accept"
            | "order.revision.decline"
            | "order.fulfillment.update"
            | "order.receipt.record"
    )
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

    use super::{
        ApprovalPolicy, NetworkRequirement, OPERATION_REGISTRY, OperationRole, RiskLevel,
        get_operation, network_requirement, requires_local_signer_mode,
        requires_nostr_relay_publish_mode,
    };

    const EXPECTED_OPERATION_IDS: &[&str] = &[
        "workspace.init",
        "workspace.get",
        "health.status.get",
        "health.check.run",
        "config.get",
        "account.create",
        "account.import",
        "account.attach_secret",
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
        "store.backup.restore",
        "sync.status.get",
        "sync.pull",
        "sync.push",
        "sync.watch",
        "farm.create",
        "farm.get",
        "farm.rebind",
        "farm.profile.update",
        "farm.location.update",
        "farm.fulfillment.update",
        "farm.readiness.check",
        "farm.publish",
        "listing.create",
        "listing.get",
        "listing.list",
        "listing.app.list",
        "listing.app.export",
        "listing.update",
        "listing.validate",
        "listing.rebind",
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
        "basket.adjustment.add",
        "basket.adjustment.remove",
        "basket.validate",
        "basket.quote.create",
        "order.submit",
        "order.get",
        "order.list",
        "order.app.list",
        "order.app.export",
        "order.rebind",
        "order.accept",
        "order.decline",
        "order.cancel",
        "order.revision.propose",
        "order.revision.accept",
        "order.revision.decline",
        "order.fulfillment.update",
        "order.receipt.record",
        "order.payment.record",
        "order.settlement.accept",
        "order.settlement.reject",
        "order.status.get",
        "order.event.list",
        "order.event.watch",
        "validation.receipt.get",
        "validation.receipt.list",
        "validation.receipt.verify",
    ];

    const SUPPORTED_MUTATING_DRY_RUN_OPERATION_IDS: &[&str] = &[
        "workspace.init",
        "account.create",
        "account.import",
        "account.attach_secret",
        "account.remove",
        "account.selection.update",
        "account.selection.clear",
        "store.init",
        "store.backup.create",
        "store.backup.restore",
        "sync.pull",
        "sync.push",
        "farm.create",
        "farm.rebind",
        "farm.profile.update",
        "farm.location.update",
        "farm.fulfillment.update",
        "farm.publish",
        "listing.create",
        "listing.app.export",
        "listing.update",
        "listing.rebind",
        "listing.publish",
        "listing.archive",
        "market.refresh",
        "basket.create",
        "basket.item.add",
        "basket.item.update",
        "basket.item.remove",
        "basket.adjustment.add",
        "basket.adjustment.remove",
        "basket.quote.create",
        "order.submit",
        "order.app.export",
        "order.rebind",
        "order.accept",
        "order.decline",
        "order.cancel",
        "order.revision.propose",
        "order.revision.accept",
        "order.revision.decline",
        "order.fulfillment.update",
        "order.receipt.record",
    ];

    const INTENTIONALLY_UNSUPPORTED_MUTATING_DRY_RUN_OPERATION_IDS: &[&str] = &[];

    #[test]
    fn registry_contains_exact_target_operation_set() {
        let actual = operation_ids();
        let expected = EXPECTED_OPERATION_IDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
        assert_eq!(OPERATION_REGISTRY.len(), 79);
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
            "account.attach_secret",
            "account.remove",
            "sync.push",
            "farm.rebind",
            "farm.publish",
            "listing.rebind",
            "listing.publish",
            "listing.update",
            "listing.archive",
            "order.submit",
            "order.rebind",
            "order.accept",
            "order.decline",
            "order.cancel",
            "order.revision.propose",
            "order.revision.accept",
            "order.revision.decline",
            "order.fulfillment.update",
            "order.receipt.record",
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
    fn mutating_dry_run_registry_inventory_is_complete() {
        let advertised = OPERATION_REGISTRY
            .iter()
            .filter(|operation| operation.mutates && operation.supports_dry_run)
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let supported = SUPPORTED_MUTATING_DRY_RUN_OPERATION_IDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let unsupported = INTENTIONALLY_UNSUPPORTED_MUTATING_DRY_RUN_OPERATION_IDS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let classified = supported
            .union(&unsupported)
            .copied()
            .collect::<BTreeSet<_>>();

        assert_eq!(advertised, classified);
        assert!(supported.is_disjoint(&unsupported));
    }

    #[test]
    fn registry_ndjson_support_is_explicit() {
        let actual = OPERATION_REGISTRY
            .iter()
            .filter(|operation| operation.supports_ndjson)
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected = [
            "health.status.get",
            "health.check.run",
            "config.get",
            "account.list",
            "relay.list",
            "sync.pull",
            "sync.push",
            "sync.watch",
            "listing.list",
            "market.refresh",
            "market.product.search",
            "basket.list",
            "order.list",
            "order.event.list",
            "validation.receipt.list",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn registry_network_requirements_are_explicit() {
        let external = OPERATION_REGISTRY
            .iter()
            .filter(|operation| {
                matches!(
                    network_requirement(operation.operation_id),
                    NetworkRequirement::External { .. }
                )
            })
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected = [
            "sync.pull",
            "sync.push",
            "sync.watch",
            "market.refresh",
            "farm.publish",
            "listing.publish",
            "listing.update",
            "listing.archive",
            "order.submit",
            "order.accept",
            "order.decline",
            "order.cancel",
            "order.revision.propose",
            "order.revision.accept",
            "order.revision.decline",
            "order.fulfillment.update",
            "order.receipt.record",
            "order.event.list",
            "validation.receipt.get",
            "validation.receipt.list",
            "validation.receipt.verify",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(external, expected);
    }

    #[test]
    fn registry_local_signer_requirements_are_explicit() {
        let signed = OPERATION_REGISTRY
            .iter()
            .filter(|operation| requires_local_signer_mode(operation.operation_id))
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected = [
            "signer.status.get",
            "sync.push",
            "farm.publish",
            "listing.publish",
            "listing.update",
            "listing.archive",
            "order.submit",
            "order.accept",
            "order.decline",
            "order.cancel",
            "order.revision.propose",
            "order.revision.accept",
            "order.revision.decline",
            "order.fulfillment.update",
            "order.receipt.record",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(signed, expected);
    }

    #[test]
    fn registry_nostr_relay_publish_requirements_are_explicit() {
        let publish = OPERATION_REGISTRY
            .iter()
            .filter(|operation| requires_nostr_relay_publish_mode(operation.operation_id))
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected = [
            "sync.push",
            "farm.publish",
            "listing.publish",
            "listing.update",
            "listing.archive",
            "order.submit",
            "order.accept",
            "order.decline",
            "order.cancel",
            "order.revision.propose",
            "order.revision.accept",
            "order.revision.decline",
            "order.fulfillment.update",
            "order.receipt.record",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        assert_eq!(publish, expected);
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
        assert!(!namespaces.contains("runtime"));
        assert!(!namespaces.contains("job"));
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
