use serde::Serialize;
use serde_json::json;

use crate::ops::{
    OperationAdapterError, OperationRequest, OperationResult, OperationResultData,
    OperationService, ValidationStatusRequest, ValidationStatusResult,
};
use crate::runtime::config::RuntimeConfig;

pub struct ValidationOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> ValidationOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<ValidationStatusRequest> for ValidationOperationService<'_> {
    type Result = ValidationStatusResult;

    fn execute(
        &self,
        _request: OperationRequest<ValidationStatusRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let state = if self.config.rhi.validator_set.is_some() {
            "ready"
        } else {
            "unconfigured"
        };
        let view = json!({
            "state": state,
            "source": "Runtime Contract V1 validation configuration",
            "validator_set_configured": self.config.rhi.validator_set.is_some(),
            "cryptographic_proof_required": self.config.rhi.require_cryptographic_proof,
        });
        serialized_operation_result::<ValidationStatusResult, _>(&view)
    }
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}
