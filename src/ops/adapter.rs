use super::error::OperationAdapterError;
use super::request::{OperationRequest, OperationRequestPayload};
use super::result::{OperationResult, OperationResultPayload};

pub trait OperationService<P: OperationRequestPayload> {
    type Result: OperationResultPayload;

    fn execute(
        &self,
        request: OperationRequest<P>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError>;
}

#[derive(Debug, Clone)]
pub struct OperationAdapter<S> {
    service: S,
}

impl<S> OperationAdapter<S> {
    pub fn new(service: S) -> Self {
        Self { service }
    }

    pub fn execute<P>(
        &self,
        request: OperationRequest<P>,
    ) -> Result<OperationResult<<S as OperationService<P>>::Result>, OperationAdapterError>
    where
        P: OperationRequestPayload,
        S: OperationService<P>,
    {
        self.service.execute(request)
    }
}
