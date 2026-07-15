use serde::Serialize;
use serde_json::{Value, json};

use crate::ops::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, ValidationReceiptGetRequest,
    ValidationReceiptGetResult, ValidationReceiptVerifyRequest, ValidationReceiptVerifyResult,
    ValidationStatusRequest, ValidationStatusResult,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::validation_receipt::{
    ValidationReceiptEventArgs, ValidationReceiptInspectionView,
};
use crate::view::runtime::CommandDisposition;

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

impl OperationService<ValidationReceiptGetRequest> for ValidationOperationService<'_> {
    type Result = ValidationReceiptGetResult;

    fn execute(
        &self,
        request: OperationRequest<ValidationReceiptGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = validation_receipt_event_args(&request)?;
        let view = crate::runtime::validation_receipt::get(self.config, &args);
        validation_receipt_inspection_result::<ValidationReceiptGetResult>(
            "validation.receipt.get",
            &view,
        )
    }
}

impl OperationService<ValidationReceiptVerifyRequest> for ValidationOperationService<'_> {
    type Result = ValidationReceiptVerifyResult;

    fn execute(
        &self,
        request: OperationRequest<ValidationReceiptVerifyRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = validation_receipt_event_args(&request)?;
        let view = crate::runtime::validation_receipt::verify(self.config, &args);
        validation_receipt_inspection_result::<ValidationReceiptVerifyResult>(
            "validation.receipt.verify",
            &view,
        )
    }
}

fn validation_receipt_event_args<P>(
    request: &OperationRequest<P>,
) -> Result<ValidationReceiptEventArgs, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    Ok(ValidationReceiptEventArgs {
        receipt_event_id: required_string(request, "receipt_event_id")?,
    })
}

fn validation_receipt_inspection_result<R>(
    operation_id: &str,
    view: &ValidationReceiptInspectionView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<R, _>(view),
        disposition => Err(validation_receipt_view_error(
            operation_id,
            disposition,
            view,
            view.reason.as_deref(),
        )),
    }
}

fn validation_receipt_view_error<T>(
    operation_id: &str,
    disposition: CommandDisposition,
    view: &T,
    reason: Option<&str>,
) -> OperationAdapterError
where
    T: Serialize,
{
    let detail = serde_json::to_value(view).unwrap_or_else(|_| json!({}));
    let message = reason
        .map(str::to_owned)
        .unwrap_or_else(|| format!("`{operation_id}` validation receipt operation failed"));
    match disposition {
        CommandDisposition::NotFound => {
            OperationAdapterError::not_found_with_detail(operation_id, message, detail)
        }
        CommandDisposition::ValidationFailed => {
            OperationAdapterError::validation_failed_with_detail(operation_id, message, detail)
        }
        CommandDisposition::Unconfigured => {
            OperationAdapterError::operation_unavailable_with_detail(operation_id, message, detail)
        }
        CommandDisposition::ExternalUnavailable => {
            OperationAdapterError::network_unavailable_with_detail(operation_id, message, detail)
        }
        CommandDisposition::Unsupported => OperationAdapterError::InvalidInput {
            operation_id: operation_id.to_owned(),
            message,
        },
        CommandDisposition::InternalError | CommandDisposition::Success => {
            OperationAdapterError::Runtime(message)
        }
    }
}

fn required_string<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| OperationAdapterError::InvalidInput {
            operation_id: request.operation_id().to_owned(),
            message: format!("missing required `{key}` input"),
        })
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}
