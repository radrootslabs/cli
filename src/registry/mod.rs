use radroots_runtime_contract_v1::{
    ApprovalRequirementV1, DryRunSupportV1, IdempotencyPolicyV1, OperationMutabilityV1,
    RuntimeOperationDescriptorV1, RuntimeOperationIdV1, SignerRequirementV1,
    TransportCapabilityRouteV1,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub runtime_operation_id: RuntimeOperationIdV1,
    pub descriptor: RuntimeOperationDescriptorV1,
    pub operation_id: &'static str,
    pub cli_path: &'static str,
    pub namespace: &'static str,
    pub mcp_tool: &'static str,
    pub rust_request: &'static str,
    pub rust_result: &'static str,
    pub json_kind: &'static str,
    pub description: &'static str,
    pub role: OperationRole,
    pub supports_json: bool,
    pub supports_ndjson: bool,
}

impl OperationSpec {
    pub fn mutates(self) -> bool {
        self.descriptor.mutability == OperationMutabilityV1::Mutation
    }

    pub fn supports_dry_run(self) -> bool {
        self.descriptor.dry_run == DryRunSupportV1::PureLocalPlan
    }

    pub fn requires_idempotency(self) -> bool {
        self.descriptor.idempotency == IdempotencyPolicyV1::RequiredUuidV7
    }

    pub fn forbids_idempotency(self) -> bool {
        self.descriptor.idempotency == IdempotencyPolicyV1::Forbidden
    }

    pub fn requires_approval(self) -> bool {
        matches!(
            self.descriptor.approval,
            ApprovalRequirementV1::Required | ApprovalRequirementV1::ConditionalOrRequiredByMode
        )
    }
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

include!("../generated/runtime_contract_registry.rs");

pub fn get_operation(operation_id: &str) -> Option<&'static OperationSpec> {
    OPERATION_REGISTRY
        .iter()
        .find(|operation| operation.operation_id == operation_id)
}

pub fn runtime_operation_id(operation_id: &str) -> Option<RuntimeOperationIdV1> {
    RuntimeOperationIdV1::parse(operation_id).ok()
}

pub fn network_requirement(operation_id: &str) -> NetworkRequirement {
    let Some(spec) = get_operation(operation_id) else {
        return NetworkRequirement::Local;
    };
    if requires_external_transport(spec.descriptor.transport_capability) {
        NetworkRequirement::External {
            dry_run_requires_network: spec.descriptor.dry_run != DryRunSupportV1::PureLocalPlan,
        }
    } else {
        NetworkRequirement::Local
    }
}

pub fn requires_local_signer_mode(operation_id: &str) -> bool {
    get_operation(operation_id).is_some_and(|operation| {
        matches!(
            operation.descriptor.signer,
            SignerRequirementV1::Required | SignerRequirementV1::ConditionalRelayAuth
        )
    })
}

pub fn requires_delivery_capable_transport_profile(operation_id: &str) -> bool {
    get_operation(operation_id)
        .is_some_and(|operation| operation.descriptor.transport_capability.deliver)
}

pub fn registry_linkage_is_valid() -> bool {
    OPERATION_REGISTRY.iter().all(|operation| {
        get_operation(operation.operation_id).is_some()
            && operation.operation_id == operation.json_kind
            && operation.mcp_tool == operation.operation_id.replace('.', "_")
            && operation.supports_json
            && operation.descriptor.operation_id == operation.runtime_operation_id
            && operation.descriptor.operation_id.as_str() == operation.operation_id
    })
}

fn requires_external_transport(capability: TransportCapabilityRouteV1) -> bool {
    capability.deliver || capability.fetch || capability.synchronize || capability.diagnostics
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use radroots_runtime_contract_v1::{OperationRiskV1, RUNTIME_OPERATION_DESCRIPTORS_V1};

    use super::{OPERATION_REGISTRY, get_operation};

    #[test]
    fn registry_matches_runtime_contract_v1_catalog_exactly() {
        let actual = OPERATION_REGISTRY
            .iter()
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let expected = RUNTIME_OPERATION_DESCRIPTORS_V1
            .iter()
            .map(|descriptor| descriptor.operation_id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn generated_registry_identity_fields_are_consistent() {
        for operation in OPERATION_REGISTRY {
            assert_eq!(operation.operation_id, operation.json_kind);
            assert_eq!(
                operation.operation_id,
                operation.runtime_operation_id.as_str()
            );
            assert_eq!(operation.mcp_tool, operation.operation_id.replace('.', "_"));
            assert!(operation.cli_path.starts_with("radroots "));
            assert_eq!(
                operation.namespace,
                operation.operation_id.split('.').next().unwrap()
            );
            assert!(operation.supports_json);
            assert!(!operation.description.is_empty());
            assert_eq!(
                get_operation(operation.operation_id)
                    .expect("operation")
                    .operation_id,
                operation.operation_id
            );
        }
    }

    #[test]
    fn generated_registry_uses_contract_policy() {
        for operation in OPERATION_REGISTRY {
            if operation.requires_approval() {
                assert!(
                    matches!(
                        operation.descriptor.risk,
                        OperationRiskV1::High | OperationRiskV1::Critical
                    ),
                    "{}",
                    operation.operation_id
                );
            }
            if operation.mutates() {
                assert!(operation.supports_dry_run(), "{}", operation.operation_id);
                assert!(
                    operation.requires_idempotency(),
                    "{}",
                    operation.operation_id
                );
            } else {
                assert!(
                    operation.forbids_idempotency(),
                    "{}",
                    operation.operation_id
                );
            }
        }
    }
}
