use std::fmt::Debug;

use serde_json::{Map, Value};

use super::context::OperationContext;
use super::error::OperationAdapterError;
use crate::registry::{OperationSpec, get_operation};

pub type OperationData = Map<String, Value>;

pub trait OperationRequestPayload: Debug + Clone + PartialEq + 'static {
    const OPERATION_ID: &'static str;
    const REQUEST_TYPE: &'static str;
}

pub trait OperationRequestData: OperationRequestPayload {
    fn input(&self) -> &OperationData;
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationRequest<P: OperationRequestPayload> {
    pub spec: &'static OperationSpec,
    pub context: OperationContext,
    pub payload: P,
}

impl<P: OperationRequestPayload> OperationRequest<P> {
    pub fn new(context: OperationContext, payload: P) -> Result<Self, OperationAdapterError> {
        let spec = get_operation(P::OPERATION_ID)
            .ok_or_else(|| OperationAdapterError::UnknownOperation(P::OPERATION_ID.to_owned()))?;
        if spec.rust_request != P::REQUEST_TYPE {
            return Err(OperationAdapterError::RequestTypeMismatch {
                operation_id: P::OPERATION_ID.to_owned(),
                registry_request: spec.rust_request.to_owned(),
                adapter_request: P::REQUEST_TYPE.to_owned(),
            });
        }
        Ok(Self {
            spec,
            context,
            payload,
        })
    }

    pub fn operation_id(&self) -> &'static str {
        P::OPERATION_ID
    }

    pub fn request_type_name(&self) -> &'static str {
        P::REQUEST_TYPE
    }
}
