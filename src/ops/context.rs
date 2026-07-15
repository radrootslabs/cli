use std::time::{SystemTime, UNIX_EPOCH};

use radroots_runtime_contract_v1::{ApprovalProofV1, RuntimeContractErrorV1};

use crate::cli::{TargetCliArgs, TargetOutputFormat};
use crate::ops::OperationAdapterError;
use crate::out::envelope::{EnvelopeActor, EnvelopeContext, OutputFormat as EnvelopeOutputFormat};
use crate::runtime::config::OutputFormat as RuntimeOutputFormat;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OperationOutputFormat {
    #[default]
    Terminal,
    Json,
    Ndjson,
}

impl From<TargetOutputFormat> for OperationOutputFormat {
    fn from(format: TargetOutputFormat) -> Self {
        match format {
            TargetOutputFormat::Terminal => Self::Terminal,
            TargetOutputFormat::Json => Self::Json,
            TargetOutputFormat::Ndjson => Self::Ndjson,
        }
    }
}

impl From<RuntimeOutputFormat> for OperationOutputFormat {
    fn from(format: RuntimeOutputFormat) -> Self {
        match format {
            RuntimeOutputFormat::Terminal => Self::Terminal,
            RuntimeOutputFormat::Json => Self::Json,
            RuntimeOutputFormat::Ndjson => Self::Ndjson,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OperationNetworkMode {
    #[default]
    Default,
    Offline,
    Online,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OperationInputMode {
    #[default]
    PromptingAllowed,
    NoInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationContext {
    pub output_format: OperationOutputFormat,
    pub account_id: Option<String>,
    pub network_mode: OperationNetworkMode,
    pub dry_run: bool,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    pub approval_proof: Option<ApprovalProofV1>,
    pub yes: bool,
    pub input_mode: OperationInputMode,
    pub quiet: bool,
    pub verbose: bool,
    pub trace: bool,
}

impl OperationContext {
    pub fn from_target_args(
        args: &TargetCliArgs,
        operation_id: &'static str,
    ) -> Result<Self, OperationAdapterError> {
        Ok(Self {
            output_format: args
                .format
                .map(OperationOutputFormat::from)
                .unwrap_or_default(),
            account_id: args.account_id.clone(),
            network_mode: if args.offline {
                OperationNetworkMode::Offline
            } else if args.online {
                OperationNetworkMode::Online
            } else {
                OperationNetworkMode::Default
            },
            dry_run: args.dry_run,
            idempotency_key: args.idempotency_key.clone(),
            correlation_id: args.correlation_id.clone(),
            approval_proof: parse_approval_proof(args.approval_proof.as_deref(), operation_id)?,
            yes: args.yes,
            input_mode: if args.no_input {
                OperationInputMode::NoInput
            } else {
                OperationInputMode::PromptingAllowed
            },
            quiet: args.quiet,
            verbose: args.verbose,
            trace: args.trace,
        })
    }

    pub fn envelope_context(&self, request_id: impl Into<String>) -> EnvelopeContext {
        let mut context = EnvelopeContext::new(request_id, self.dry_run);
        context.output_format = match self.output_format {
            OperationOutputFormat::Terminal => EnvelopeOutputFormat::Terminal,
            OperationOutputFormat::Json => EnvelopeOutputFormat::Json,
            OperationOutputFormat::Ndjson => EnvelopeOutputFormat::Ndjson,
        };
        context.correlation_id = self.correlation_id.clone();
        context.idempotency_key = self.idempotency_key.clone();
        context.actor = self.account_id.as_ref().map(|account_id| EnvelopeActor {
            account_id: account_id.clone(),
            role: "account".to_owned(),
        });
        context
    }

    pub fn has_operator_approval(&self) -> bool {
        self.yes || self.approval_proof.is_some()
    }
}

fn parse_approval_proof(
    value: Option<&str>,
    operation_id: &'static str,
) -> Result<Option<ApprovalProofV1>, OperationAdapterError> {
    let Some(value) = value.map(str::trim) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(invalid_approval(
            operation_id,
            "approval proof JSON must not be empty",
        ));
    }
    let proof = serde_json::from_str::<ApprovalProofV1>(value).map_err(|error| {
        invalid_approval(
            operation_id,
            format!("approval proof must be valid ApprovalProofV1 JSON: {error}"),
        )
    })?;
    validate_approval_proof(&proof, operation_id)?;
    Ok(Some(proof))
}

fn validate_approval_proof(
    proof: &ApprovalProofV1,
    operation_id: &'static str,
) -> Result<(), OperationAdapterError> {
    if proof.operation_id.as_str() != operation_id {
        return Err(invalid_approval(
            operation_id,
            format!(
                "approval proof operation `{}` does not match request operation `{operation_id}`",
                proof.operation_id.as_str()
            ),
        ));
    }
    proof
        .request_digest
        .validate()
        .map_err(|error| invalid_approval(operation_id, runtime_contract_error(error)))?;
    for (field, value) in [
        ("proof_id", proof.proof_id.as_str()),
        ("signer_pubkey", proof.signer_pubkey.as_str()),
        ("signature", proof.signature.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(invalid_approval(
                operation_id,
                format!("approval proof `{field}` must not be empty"),
            ));
        }
    }
    if proof.signed_at_unix_ms == 0 {
        return Err(invalid_approval(
            operation_id,
            "approval proof `signed_at_unix_ms` must be nonzero",
        ));
    }
    if let Some(expires_at_unix_ms) = proof.expires_at_unix_ms {
        let now_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or_default();
        if expires_at_unix_ms <= now_unix_ms {
            return Err(OperationAdapterError::ApprovalRequired {
                operation_id: operation_id.to_owned(),
                message: "approval proof has expired".to_owned(),
            });
        }
    }
    Ok(())
}

fn invalid_approval(
    operation_id: &'static str,
    message: impl Into<String>,
) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message: message.into(),
    }
}

fn runtime_contract_error(error: RuntimeContractErrorV1) -> String {
    error.to_string()
}
