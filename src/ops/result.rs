use std::fmt::Debug;

use serde::Serialize;
use serde_json::Value;

use super::error::OperationAdapterError;
use super::request::OperationData;
use crate::out::envelope::{
    EnvelopeContext, NextAction, OutputEnvelope, OutputWarning, next_actions_from_result_value,
};
use crate::registry::{OperationSpec, get_operation};

pub trait OperationResultPayload: Debug + Clone + PartialEq + Serialize + 'static {
    const OPERATION_ID: &'static str;
    const RESULT_TYPE: &'static str;
}

pub trait OperationResultData: OperationResultPayload + Sized {
    fn from_data(data: OperationData) -> Self;

    fn from_value(value: Value) -> Self {
        Self::from_data(value_to_data(value))
    }

    fn from_serializable<T: Serialize>(value: &T) -> Result<Self, OperationAdapterError> {
        Ok(Self::from_value(serde_json::to_value(value).map_err(
            |error| OperationAdapterError::Serialization(error.to_string()),
        )?))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationResult<P: OperationResultPayload> {
    pub spec: &'static OperationSpec,
    pub payload: P,
    pub warnings: Vec<OutputWarning>,
    pub next_actions: Vec<NextAction>,
}

impl<P: OperationResultPayload> OperationResult<P> {
    pub fn new(payload: P) -> Result<Self, OperationAdapterError> {
        let spec = get_operation(P::OPERATION_ID)
            .ok_or_else(|| OperationAdapterError::UnknownOperation(P::OPERATION_ID.to_owned()))?;
        if spec.rust_result != P::RESULT_TYPE {
            return Err(OperationAdapterError::ResultTypeMismatch {
                operation_id: P::OPERATION_ID.to_owned(),
                registry_result: spec.rust_result.to_owned(),
                adapter_result: P::RESULT_TYPE.to_owned(),
            });
        }
        Ok(Self {
            spec,
            payload,
            warnings: Vec::new(),
            next_actions: Vec::new(),
        })
    }

    pub fn operation_id(&self) -> &'static str {
        P::OPERATION_ID
    }

    pub fn result_type_name(&self) -> &'static str {
        P::RESULT_TYPE
    }

    pub fn to_envelope(
        &self,
        context: EnvelopeContext,
    ) -> Result<OutputEnvelope, OperationAdapterError> {
        let result = serde_json::to_value(&self.payload)
            .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?;
        let next_actions = if self.next_actions.is_empty() {
            next_actions_from_result(&result)
        } else {
            self.next_actions.clone()
        };
        let mut envelope = OutputEnvelope::success(self.operation_id(), result, context);
        envelope.warnings = self.warnings.clone();
        envelope.next_actions = next_actions;
        Ok(envelope)
    }
}

fn next_actions_from_result(result: &Value) -> Vec<NextAction> {
    next_actions_from_result_value(result)
}

pub(crate) fn value_to_data(value: Value) -> OperationData {
    match value {
        Value::Object(map) => map,
        other => {
            let mut map = OperationData::new();
            map.insert("value".to_owned(), other);
            map
        }
    }
}
