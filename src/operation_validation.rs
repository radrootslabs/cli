use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::runtime::CommandDisposition;
use crate::operation_adapter::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, ValidationReceiptGetRequest,
    ValidationReceiptGetResult, ValidationReceiptListRequest, ValidationReceiptListResult,
    ValidationReceiptVerifyRequest, ValidationReceiptVerifyResult,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::validation_receipt::{
    ValidationReceiptEventArgs, ValidationReceiptInspectionView, ValidationReceiptListArgs,
    ValidationReceiptListView,
};

pub struct ValidationOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> ValidationOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
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

impl OperationService<ValidationReceiptListRequest> for ValidationOperationService<'_> {
    type Result = ValidationReceiptListResult;

    fn execute(
        &self,
        request: OperationRequest<ValidationReceiptListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = validation_receipt_list_args(&request)?;
        let view = crate::runtime::validation_receipt::list(self.config, &args);
        validation_receipt_list_result(&view)
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

fn validation_receipt_list_args<P>(
    request: &OperationRequest<P>,
) -> Result<ValidationReceiptListArgs, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    Ok(ValidationReceiptListArgs {
        order_id: required_string(request, "order_id")?,
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

fn validation_receipt_list_result(
    view: &ValidationReceiptListView,
) -> Result<OperationResult<ValidationReceiptListResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => {
            serialized_operation_result::<ValidationReceiptListResult, _>(view)
        }
        disposition => Err(validation_receipt_view_error(
            "validation.receipt.list",
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
