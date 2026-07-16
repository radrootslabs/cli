use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use crate::ops::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, TradeCancellationSubmitRequest,
    TradeCancellationSubmitResult, TradeCandidateDecideRequest, TradeCandidateDecideResult,
    TradeEvidenceInspectRequest, TradeEvidenceInspectResult, TradeEvidenceRefreshRequest,
    TradeEvidenceRefreshResult, TradeGetRequest, TradeGetResult, TradeListRequest, TradeListResult,
    TradeOperationResumeRequest, TradeOperationResumeResult, TradePrivateArtifactDeleteRequest,
    TradePrivateArtifactDeleteResult, TradePrivateArtifactOpenRequest,
    TradePrivateArtifactOpenResult, TradePrivateArtifactSealRequest,
    TradePrivateArtifactSealResult, TradeProposalSubmitRequest, TradeProposalSubmitResult,
    TradeRevisionProposeRequest, TradeRevisionProposeResult,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::trade::{
    TradeEnvelopeFileRuntimeArgs, TradeEvidenceInspectRuntimeArgs, TradeIdRuntimeArgs,
    TradePageRuntimeArgs, TradePrivateArtifactDeleteRuntimeArgs,
    TradePrivateArtifactOpenRuntimeArgs, TradePrivateArtifactSealRuntimeArgs,
};

pub struct TradeOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> TradeOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<TradeProposalSubmitRequest> for TradeOperationService<'_> {
    type Result = TradeProposalSubmitResult;

    fn execute(
        &self,
        request: OperationRequest<TradeProposalSubmitRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = envelope_args(&request)?;
        let receipt = crate::runtime::trade::submit_proposal(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradeRevisionProposeRequest> for TradeOperationService<'_> {
    type Result = TradeRevisionProposeResult;

    fn execute(
        &self,
        request: OperationRequest<TradeRevisionProposeRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = envelope_args(&request)?;
        let receipt = crate::runtime::trade::propose_revision(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradeCandidateDecideRequest> for TradeOperationService<'_> {
    type Result = TradeCandidateDecideResult;

    fn execute(
        &self,
        request: OperationRequest<TradeCandidateDecideRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = envelope_args(&request)?;
        let receipt = crate::runtime::trade::decide_candidate(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradeCancellationSubmitRequest> for TradeOperationService<'_> {
    type Result = TradeCancellationSubmitResult;

    fn execute(
        &self,
        request: OperationRequest<TradeCancellationSubmitRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = envelope_args(&request)?;
        let receipt = crate::runtime::trade::cancel_trade(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradeOperationResumeRequest> for TradeOperationService<'_> {
    type Result = TradeOperationResumeResult;

    fn execute(
        &self,
        request: OperationRequest<TradeOperationResumeRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = envelope_args(&request)?;
        let receipt = crate::runtime::trade::resume_operation(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradeGetRequest> for TradeOperationService<'_> {
    type Result = TradeGetResult;

    fn execute(
        &self,
        request: OperationRequest<TradeGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradeIdRuntimeArgs {
            trade_id: required_string(&request, "trade_id")?,
        };
        let view = crate::runtime::trade::get_trade(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&view)
    }
}

impl OperationService<TradeListRequest> for TradeOperationService<'_> {
    type Result = TradeListResult;

    fn execute(
        &self,
        request: OperationRequest<TradeListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradePageRuntimeArgs {
            limit: u32_input(&request, "limit")?,
            cursor: string_input(&request, "cursor"),
        };
        let page = crate::runtime::trade::list_trades(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&page)
    }
}

impl OperationService<TradeEvidenceRefreshRequest> for TradeOperationService<'_> {
    type Result = TradeEvidenceRefreshResult;

    fn execute(
        &self,
        request: OperationRequest<TradeEvidenceRefreshRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradeIdRuntimeArgs {
            trade_id: required_string(&request, "trade_id")?,
        };
        let receipt = crate::runtime::trade::refresh_evidence(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradeEvidenceInspectRequest> for TradeOperationService<'_> {
    type Result = TradeEvidenceInspectResult;

    fn execute(
        &self,
        request: OperationRequest<TradeEvidenceInspectRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradeEvidenceInspectRuntimeArgs {
            trade_id: required_string(&request, "trade_id")?,
            limit: u32_input(&request, "limit")?,
            cursor: string_input(&request, "cursor"),
        };
        let page = crate::runtime::trade::inspect_evidence(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&page)
    }
}

impl OperationService<TradePrivateArtifactSealRequest> for TradeOperationService<'_> {
    type Result = TradePrivateArtifactSealResult;

    fn execute(
        &self,
        request: OperationRequest<TradePrivateArtifactSealRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradePrivateArtifactSealRuntimeArgs {
            trade_id: required_string(&request, "trade_id")?,
            artifact_id: required_string(&request, "artifact_id")?,
            schema_id: required_string(&request, "schema_id")?,
            input: required_path(&request, "input")?,
            kind: string_input(&request, "kind").unwrap_or_else(|| "binding_terms".to_owned()),
            candidate_id: string_input(&request, "candidate_id"),
            retention_class: string_input(&request, "retention_class"),
            expires_at_ms: i64_input(&request, "expires_at_ms")?,
        };
        let receipt = crate::runtime::trade::seal_private_artifact(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

impl OperationService<TradePrivateArtifactOpenRequest> for TradeOperationService<'_> {
    type Result = TradePrivateArtifactOpenResult;

    fn execute(
        &self,
        request: OperationRequest<TradePrivateArtifactOpenRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradePrivateArtifactOpenRuntimeArgs {
            artifact_id: required_string(&request, "artifact_id")?,
            output: required_path(&request, "output")?,
        };
        let view = crate::runtime::trade::open_private_artifact(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        if view.state == "missing" {
            return Err(OperationAdapterError::not_found_with_detail(
                request.operation_id(),
                format!("private artifact `{}` was not found", view.artifact_id),
                json!({
                    "artifact_id": view.artifact_id,
                    "output": view.output,
                }),
            ));
        }
        serialized_target_result(&view)
    }
}

impl OperationService<TradePrivateArtifactDeleteRequest> for TradeOperationService<'_> {
    type Result = TradePrivateArtifactDeleteResult;

    fn execute(
        &self,
        request: OperationRequest<TradePrivateArtifactDeleteRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = TradePrivateArtifactDeleteRuntimeArgs {
            artifact_id: required_string(&request, "artifact_id")?,
        };
        let receipt = crate::runtime::trade::delete_private_artifact(self.config, &args)
            .map_err(|error| sdk_failure(request.operation_id(), error))?;
        serialized_target_result(&receipt)
    }
}

fn envelope_args<P>(
    request: &OperationRequest<P>,
) -> Result<TradeEnvelopeFileRuntimeArgs, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    Ok(TradeEnvelopeFileRuntimeArgs {
        file: required_path(request, "file")?,
        idempotency_key: request
            .context
            .idempotency_key
            .clone()
            .or_else(|| string_input(request, "idempotency_key")),
        acknowledge_private_terms: bool_input(request, "acknowledge_private_terms")
            .unwrap_or(false),
        operation_kind: string_input(request, "operation_kind"),
    })
}

fn serialized_target_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn required_string<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key).ok_or_else(|| {
        invalid_input(
            request.operation_id(),
            format!("missing required `{key}` input"),
        )
    })
}

fn string_input<P>(request: &OperationRequest<P>, key: &str) -> Option<String>
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
}

fn bool_input<P>(request: &OperationRequest<P>, key: &str) -> Option<bool>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_bool)
}

fn u32_input<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<Option<u32>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    invalid_input(
                        request.operation_id(),
                        format!("`{key}` must be an unsigned 32-bit integer"),
                    )
                })
        })
        .transpose()
}

fn i64_input<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<Option<i64>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .map(|value| {
            value.as_i64().ok_or_else(|| {
                invalid_input(
                    request.operation_id(),
                    format!("`{key}` must be a signed 64-bit integer"),
                )
            })
        })
        .transpose()
}

fn required_path<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<PathBuf, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key)
        .map(PathBuf::from)
        .ok_or_else(|| {
            invalid_input(
                request.operation_id(),
                format!("missing required `{key}` input"),
            )
        })
}

fn sdk_failure(
    operation_id: &str,
    error: crate::runtime::sdk::CliSdkAdapterError,
) -> OperationAdapterError {
    OperationAdapterError::sdk_adapter_failure(operation_id, error)
}

fn invalid_input(operation_id: &str, message: String) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message,
    }
}
