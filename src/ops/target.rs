use serde::Serialize;
use serde_json::Value;

use super::context::{OperationContext, OperationOutputFormat};
use super::error::OperationAdapterError;
use super::request::{
    OperationData, OperationRequest, OperationRequestData, OperationRequestPayload,
};
use super::result::{OperationResult, OperationResultData, OperationResultPayload, value_to_data};
use crate::cli::TargetCliArgs;
use crate::registry::{OPERATION_REGISTRY, OperationSpec};

macro_rules! target_operation_contracts {
    ($( $variant:ident => ($request:ident, $result:ident, $operation_id:literal) ),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum TargetOperationRequest {
            $( $variant(OperationRequest<$request>), )+
        }

        impl TargetOperationRequest {
            pub fn from_target_args(args: &TargetCliArgs) -> Result<Self, OperationAdapterError> {
                let operation_id = crate::cli::input::operation_id_from_target(args);
                Self::from_operation_id_with_input(
                    operation_id,
                    OperationContext::from_target_args(args, operation_id)?,
                    crate::cli::input::target_operation_input(&args.command),
                )
            }

            pub fn from_operation_id(
                operation_id: &'static str,
                context: OperationContext,
            ) -> Result<Self, OperationAdapterError> {
                Self::from_operation_id_with_input(operation_id, context, OperationData::new())
            }

            fn from_operation_id_with_input(
                operation_id: &'static str,
                context: OperationContext,
                input: OperationData,
            ) -> Result<Self, OperationAdapterError> {
                match operation_id {
                    $( $operation_id => Ok(Self::$variant(OperationRequest::new(context, $request::from_data(input))?)), )+
                    _ => Err(OperationAdapterError::UnknownOperation(operation_id.to_owned())),
                }
            }

            pub fn operation_id(&self) -> &'static str {
                match self {
                    $( Self::$variant(request) => request.operation_id(), )+
                }
            }

            pub fn spec(&self) -> &'static OperationSpec {
                match self {
                    $( Self::$variant(request) => request.spec, )+
                }
            }

            pub fn context(&self) -> &OperationContext {
                match self {
                    $( Self::$variant(request) => &request.context, )+
                }
            }

            pub fn set_output_format(&mut self, output_format: OperationOutputFormat) {
                match self {
                    $( Self::$variant(request) => request.context.output_format = output_format, )+
                }
            }

            pub fn request_type_name(&self) -> &'static str {
                match self {
                    $( Self::$variant(request) => request.request_type_name(), )+
                }
            }

            pub fn request_type_for_operation(operation_id: &str) -> Option<&'static str> {
                match operation_id {
                    $( $operation_id => Some(stringify!($request)), )+
                    _ => None,
                }
            }
        }

        #[derive(Debug, Clone, PartialEq)]
        pub enum TargetOperationResult {
            $( $variant(OperationResult<$result>), )+
        }

        impl TargetOperationResult {
            pub fn operation_id(&self) -> &'static str {
                match self {
                    $( Self::$variant(result) => result.operation_id(), )+
                }
            }

            pub fn result_type_name(&self) -> &'static str {
                match self {
                    $( Self::$variant(result) => result.result_type_name(), )+
                }
            }

            pub fn result_type_for_operation(operation_id: &str) -> Option<&'static str> {
                match operation_id {
                    $( $operation_id => Some(stringify!($result)), )+
                    _ => None,
                }
            }
        }

        $(
            #[derive(Debug, Default, Clone, PartialEq, Serialize)]
            pub struct $request {
                #[serde(flatten)]
                pub input: OperationData,
            }

            impl $request {
                pub fn from_data(input: OperationData) -> Self {
                    Self { input }
                }
            }

            impl OperationRequestPayload for $request {
                const OPERATION_ID: &'static str = $operation_id;
                const REQUEST_TYPE: &'static str = stringify!($request);
            }

            impl OperationRequestData for $request {
                fn input(&self) -> &OperationData {
                    &self.input
                }
            }

            #[derive(Debug, Default, Clone, PartialEq, Serialize)]
            pub struct $result {
                #[serde(flatten)]
                pub data: OperationData,
            }

            impl $result {
                pub fn from_data(data: OperationData) -> Self {
                    Self { data }
                }

                pub fn from_value(value: Value) -> Self {
                    Self {
                        data: value_to_data(value),
                    }
                }

                pub fn from_serializable<T: Serialize>(
                    value: &T,
                ) -> Result<Self, OperationAdapterError> {
                    Ok(Self::from_value(
                        serde_json::to_value(value)
                            .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?,
                    ))
                }
            }

            impl OperationResultPayload for $result {
                const OPERATION_ID: &'static str = $operation_id;
                const RESULT_TYPE: &'static str = stringify!($result);
            }

            impl OperationResultData for $result {
                fn from_data(data: OperationData) -> Self {
                    Self { data }
                }
            }
        )+
    };
}

include!("../generated/runtime_contract_target.rs");

pub fn adapter_registry_linkage_is_valid() -> bool {
    OPERATION_REGISTRY.iter().all(|operation| {
        TargetOperationRequest::request_type_for_operation(operation.operation_id)
            == Some(operation.rust_request)
            && TargetOperationResult::result_type_for_operation(operation.operation_id)
                == Some(operation.rust_result)
    })
}
