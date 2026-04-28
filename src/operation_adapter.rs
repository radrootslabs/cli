#![allow(dead_code)]

use std::fmt::Debug;
use std::io::ErrorKind;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::domain::runtime::CommandDisposition;
use crate::operation_registry::{OPERATION_REGISTRY, OperationSpec, get_operation};
use crate::output_contract::{
    CliExitCode, EnvelopeActor, EnvelopeContext, NextAction, OUTPUT_SCHEMA_VERSION, OutputEnvelope,
    OutputError, OutputWarning,
};
use crate::runtime::RuntimeError;
use crate::target_cli::{TargetCliArgs, TargetOutputFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutputFormat {
    Human,
    Json,
    Ndjson,
}

impl Default for OperationOutputFormat {
    fn default() -> Self {
        Self::Human
    }
}

impl From<TargetOutputFormat> for OperationOutputFormat {
    fn from(format: TargetOutputFormat) -> Self {
        match format {
            TargetOutputFormat::Human => Self::Human,
            TargetOutputFormat::Json => Self::Json,
            TargetOutputFormat::Ndjson => Self::Ndjson,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationNetworkMode {
    Default,
    Offline,
    Online,
}

impl Default for OperationNetworkMode {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationInputMode {
    PromptingAllowed,
    NoInput,
}

impl Default for OperationInputMode {
    fn default() -> Self {
        Self::PromptingAllowed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationContext {
    pub output_format: OperationOutputFormat,
    pub account_id: Option<String>,
    pub relays: Vec<String>,
    pub network_mode: OperationNetworkMode,
    pub dry_run: bool,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    pub approval_token: Option<String>,
    pub input_mode: OperationInputMode,
    pub quiet: bool,
    pub verbose: bool,
    pub trace: bool,
    pub color: bool,
}

impl OperationContext {
    pub fn from_target_args(args: &TargetCliArgs) -> Self {
        Self {
            output_format: OperationOutputFormat::from(args.format),
            account_id: args.account_id.clone(),
            relays: args.relay.clone(),
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
            approval_token: args.approval_token.clone(),
            input_mode: if args.no_input {
                OperationInputMode::NoInput
            } else {
                OperationInputMode::PromptingAllowed
            },
            quiet: args.quiet,
            verbose: args.verbose,
            trace: args.trace,
            color: !args.no_color,
        }
    }

    pub fn envelope_context(&self, request_id: impl Into<String>) -> EnvelopeContext {
        let mut context = EnvelopeContext::new(request_id, self.dry_run);
        context.correlation_id = self.correlation_id.clone();
        context.idempotency_key = self.idempotency_key.clone();
        context.actor = self.account_id.as_ref().map(|account_id| EnvelopeActor {
            account_id: account_id.clone(),
            role: "account".to_owned(),
        });
        context
    }

    pub fn requires_approval_token(&self) -> bool {
        !self.dry_run && !self.has_approval_token()
    }

    pub fn has_approval_token(&self) -> bool {
        self.approval_token
            .as_deref()
            .is_some_and(|token| !token.trim().is_empty())
    }
}

pub type OperationData = Map<String, Value>;

pub trait OperationRequestPayload: Debug + Clone + PartialEq + 'static {
    const OPERATION_ID: &'static str;
    const REQUEST_TYPE: &'static str;
}

pub trait OperationRequestData: OperationRequestPayload {
    fn input(&self) -> &OperationData;
}

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
        Ok(OutputEnvelope {
            schema_version: OUTPUT_SCHEMA_VERSION,
            operation_id: self.operation_id().to_owned(),
            kind: self.operation_id().to_owned(),
            request_id: context.request_id,
            correlation_id: context.correlation_id,
            idempotency_key: context.idempotency_key,
            dry_run: context.dry_run,
            actor: context.actor,
            result,
            warnings: self.warnings.clone(),
            errors: Vec::new(),
            next_actions: self.next_actions.clone(),
        })
    }
}

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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OperationAdapterError {
    #[error("unknown operation `{0}`")]
    UnknownOperation(String),
    #[error(
        "operation `{operation_id}` registry request `{registry_request}` does not match adapter request `{adapter_request}`"
    )]
    RequestTypeMismatch {
        operation_id: String,
        registry_request: String,
        adapter_request: String,
    },
    #[error(
        "operation `{operation_id}` registry result `{registry_result}` does not match adapter result `{adapter_result}`"
    )]
    ResultTypeMismatch {
        operation_id: String,
        registry_result: String,
        adapter_result: String,
    },
    #[error("failed to serialize operation result: {0}")]
    Serialization(String),
    #[error("invalid operation input for `{operation_id}`: {message}")]
    InvalidInput {
        operation_id: String,
        message: String,
    },
    #[error("resource not found for `{operation_id}`: {message}")]
    NotFound {
        operation_id: String,
        message: String,
    },
    #[error("validation failed for `{operation_id}`: {message}")]
    ValidationFailed {
        operation_id: String,
        message: String,
    },
    #[error("approval required for `{operation_id}`: {message}")]
    ApprovalRequired {
        operation_id: String,
        message: String,
    },
    #[error("operation `{operation_id}` is forbidden while offline: {message}")]
    OfflineForbidden {
        operation_id: String,
        message: String,
    },
    #[error("operation `{operation_id}` cannot run online: {message}")]
    NetworkUnavailable {
        operation_id: String,
        message: String,
    },
    #[error("account unresolved for `{operation_id}`: {message}")]
    AccountUnresolved {
        operation_id: String,
        message: String,
    },
    #[error("account is watch-only for `{operation_id}`: {message}")]
    AccountWatchOnly {
        operation_id: String,
        message: String,
    },
    #[error("account mismatch for `{operation_id}`: {message}")]
    AccountMismatch {
        operation_id: String,
        message: String,
    },
    #[error("signer unconfigured for `{operation_id}`: {message}")]
    SignerUnconfigured {
        operation_id: String,
        message: String,
    },
    #[error("signer unavailable for `{operation_id}`: {message}")]
    SignerUnavailable {
        operation_id: String,
        message: String,
    },
    #[error("signer mode deferred for `{operation_id}`: {message}")]
    SignerModeDeferred {
        operation_id: String,
        message: String,
    },
    #[error("provider unconfigured for `{operation_id}`: {message}")]
    ProviderUnconfigured {
        operation_id: String,
        message: String,
    },
    #[error("provider unavailable for `{operation_id}`: {message}")]
    ProviderUnavailable {
        operation_id: String,
        message: String,
    },
    #[error("operation `{operation_id}` is unavailable: {message}")]
    OperationUnavailable {
        operation_id: String,
        message: String,
    },
    #[error("operation `{operation_id}` failed: {message}")]
    DetailedFailure {
        operation_id: String,
        code: String,
        class: String,
        message: String,
        exit_code: CliExitCode,
        detail_json: String,
    },
    #[error("operation runtime error: {0}")]
    Runtime(String),
}

impl OperationAdapterError {
    pub fn approval_required(operation_id: &str) -> Self {
        Self::ApprovalRequired {
            operation_id: operation_id.to_owned(),
            message: "missing required `approval_token` input".to_owned(),
        }
    }

    pub fn from_command_disposition(
        operation_id: &str,
        disposition: CommandDisposition,
        message: String,
    ) -> Self {
        match disposition {
            CommandDisposition::Success => Self::Runtime(message),
            CommandDisposition::NotFound => Self::NotFound {
                operation_id: operation_id.to_owned(),
                message,
            },
            CommandDisposition::Unconfigured => Self::unconfigured(operation_id, message),
            CommandDisposition::ExternalUnavailable => Self::unavailable(operation_id, message),
            CommandDisposition::Unsupported => Self::InvalidInput {
                operation_id: operation_id.to_owned(),
                message,
            },
            CommandDisposition::InternalError => Self::Runtime(message),
        }
    }

    pub fn unconfigured(operation_id: &str, message: String) -> Self {
        classify_runtime_failure(
            operation_id,
            message,
            RuntimeFailureAvailability::Unconfigured,
        )
    }

    pub fn operation_unavailable_with_detail(
        operation_id: &str,
        message: String,
        detail: Value,
    ) -> Self {
        Self::DetailedFailure {
            operation_id: operation_id.to_owned(),
            code: "operation_unavailable".to_owned(),
            class: "operation".to_owned(),
            message,
            exit_code: CliExitCode::RuntimeUnavailable,
            detail_json: detail.to_string(),
        }
    }

    pub fn network_unavailable_with_detail(
        operation_id: &str,
        message: String,
        detail: Value,
    ) -> Self {
        Self::DetailedFailure {
            operation_id: operation_id.to_owned(),
            code: "network_unavailable".to_owned(),
            class: "network".to_owned(),
            message,
            exit_code: CliExitCode::SyncOrNetworkFailure,
            detail_json: detail.to_string(),
        }
    }

    pub fn validation_failed_with_detail(
        operation_id: &str,
        message: String,
        detail: Value,
    ) -> Self {
        Self::DetailedFailure {
            operation_id: operation_id.to_owned(),
            code: "validation_failed".to_owned(),
            class: "validation".to_owned(),
            message,
            exit_code: CliExitCode::ValidationFailed,
            detail_json: detail.to_string(),
        }
    }

    pub fn unavailable(operation_id: &str, message: String) -> Self {
        classify_runtime_failure(
            operation_id,
            message,
            RuntimeFailureAvailability::Unavailable,
        )
    }

    pub fn runtime_failure(operation_id: &str, error: RuntimeError) -> Self {
        let message = error.to_string();
        let lowered = message.to_ascii_lowercase();
        match &error {
            RuntimeError::Io(io_error) if io_error.kind() == ErrorKind::NotFound => {
                Self::NotFound {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeError::Config(_) if looks_like_not_found(&lowered) => Self::NotFound {
                operation_id: operation_id.to_owned(),
                message,
            },
            RuntimeError::Config(_)
                if contains_any(
                    &lowered,
                    &[
                        "no local account",
                        "watch_only",
                        "not secret-backed",
                        "selected local account",
                    ],
                ) =>
            {
                classify_runtime_failure(
                    operation_id,
                    message,
                    RuntimeFailureAvailability::Unconfigured,
                )
            }
            RuntimeError::Config(_) if looks_like_validation_failure(&lowered) => {
                Self::ValidationFailed {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeError::Network(_) => Self::NetworkUnavailable {
                operation_id: operation_id.to_owned(),
                message,
            },
            RuntimeError::Accounts(_) => classify_runtime_failure(
                operation_id,
                message,
                RuntimeFailureAvailability::Unavailable,
            ),
            _ => Self::Runtime(message),
        }
    }

    pub fn to_output_error(&self) -> OutputError {
        match self {
            Self::ApprovalRequired { message, .. } => OutputError::new(
                "approval_required",
                message.clone(),
                CliExitCode::ApprovalRequiredOrDenied,
            ),
            Self::InvalidInput { message, .. } => {
                OutputError::new("invalid_input", message.clone(), CliExitCode::InvalidInput)
            }
            Self::NotFound {
                operation_id,
                message,
            } => runtime_output_error(
                "not_found",
                operation_id,
                "resource",
                message,
                CliExitCode::NotFound,
            ),
            Self::ValidationFailed {
                operation_id,
                message,
            } => runtime_output_error(
                "validation_failed",
                operation_id,
                "validation",
                message,
                CliExitCode::ValidationFailed,
            ),
            Self::OfflineForbidden {
                operation_id,
                message,
            } => runtime_output_error(
                "offline_forbidden",
                operation_id,
                "network",
                message,
                CliExitCode::SyncOrNetworkFailure,
            ),
            Self::NetworkUnavailable {
                operation_id,
                message,
            } => runtime_output_error(
                "network_unavailable",
                operation_id,
                "network",
                message,
                CliExitCode::SyncOrNetworkFailure,
            ),
            Self::AccountUnresolved {
                operation_id,
                message,
            } => runtime_output_error(
                "account_unresolved",
                operation_id,
                "account",
                message,
                CliExitCode::AuthorizationFailed,
            ),
            Self::AccountWatchOnly {
                operation_id,
                message,
            } => runtime_output_error(
                "account_watch_only",
                operation_id,
                "account",
                message,
                CliExitCode::SignerUnavailable,
            ),
            Self::AccountMismatch {
                operation_id,
                message,
            } => runtime_output_error(
                "account_mismatch",
                operation_id,
                "account",
                message,
                CliExitCode::AuthorizationFailed,
            ),
            Self::SignerUnconfigured {
                operation_id,
                message,
            } => runtime_output_error(
                "signer_unconfigured",
                operation_id,
                "signer",
                message,
                CliExitCode::SignerUnavailable,
            ),
            Self::SignerUnavailable {
                operation_id,
                message,
            } => runtime_output_error(
                "signer_unavailable",
                operation_id,
                "signer",
                message,
                CliExitCode::SignerUnavailable,
            ),
            Self::SignerModeDeferred {
                operation_id,
                message,
            } => runtime_output_error(
                "signer_mode_deferred",
                operation_id,
                "signer",
                message,
                CliExitCode::SignerUnavailable,
            ),
            Self::ProviderUnconfigured {
                operation_id,
                message,
            } => runtime_output_error(
                "provider_unconfigured",
                operation_id,
                "provider",
                message,
                CliExitCode::RuntimeUnavailable,
            ),
            Self::ProviderUnavailable {
                operation_id,
                message,
            } => runtime_output_error(
                "provider_unavailable",
                operation_id,
                "provider",
                message,
                CliExitCode::RuntimeUnavailable,
            ),
            Self::OperationUnavailable {
                operation_id,
                message,
            } => runtime_output_error(
                "operation_unavailable",
                operation_id,
                "operation",
                message,
                CliExitCode::RuntimeUnavailable,
            ),
            Self::DetailedFailure {
                operation_id,
                code,
                class,
                message,
                exit_code,
                detail_json,
            } => runtime_output_error_with_detail(
                code.as_str(),
                operation_id,
                class,
                message,
                *exit_code,
                detail_json,
            ),
            Self::UnknownOperation(operation_id) => OutputError::new(
                "unknown_operation",
                format!("unknown operation `{operation_id}`"),
                CliExitCode::InvalidInput,
            ),
            Self::RequestTypeMismatch { .. } | Self::ResultTypeMismatch { .. } => OutputError::new(
                "contract_mismatch",
                self.to_string(),
                CliExitCode::InternalError,
            ),
            Self::Serialization(message) => OutputError::new(
                "serialization_failed",
                message.clone(),
                CliExitCode::InternalError,
            ),
            Self::Runtime(message) => {
                OutputError::new("runtime_error", message.clone(), CliExitCode::InternalError)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeFailureAvailability {
    Unconfigured,
    Unavailable,
}

fn classify_runtime_failure(
    operation_id: &str,
    message: String,
    availability: RuntimeFailureAvailability,
) -> OperationAdapterError {
    let lowered = message.to_ascii_lowercase();
    if contains_any(&lowered, &["watch_only", "watch-only", "watch only"]) {
        return OperationAdapterError::AccountWatchOnly {
            operation_id: operation_id.to_owned(),
            message,
        };
    }
    if contains_any(
        &lowered,
        &[
            "account mismatch",
            "selected local account",
            "cannot sign listing seller_pubkey",
        ],
    ) {
        return OperationAdapterError::AccountMismatch {
            operation_id: operation_id.to_owned(),
            message,
        };
    }
    if contains_any(
        &lowered,
        &[
            "no account",
            "no local account",
            "account selector",
            "account selection",
            "did not match any local account",
            "unresolved account",
            "selected account",
        ],
    ) {
        return OperationAdapterError::AccountUnresolved {
            operation_id: operation_id.to_owned(),
            message,
        };
    }
    if contains_any(
        &lowered,
        &[
            "signer",
            "sign_event",
            "remote_nip46",
            "nip46",
            "secret-backed",
            "secret backed",
        ],
    ) {
        return match availability {
            RuntimeFailureAvailability::Unconfigured => OperationAdapterError::SignerUnconfigured {
                operation_id: operation_id.to_owned(),
                message,
            },
            RuntimeFailureAvailability::Unavailable => OperationAdapterError::SignerUnavailable {
                operation_id: operation_id.to_owned(),
                message,
            },
        };
    }
    if contains_any(
        &lowered,
        &[
            "provider",
            "write-plane",
            "write plane",
            "radrootsd",
            "bridge",
            "rpc",
            "daemon",
        ],
    ) {
        return match availability {
            RuntimeFailureAvailability::Unconfigured => {
                OperationAdapterError::ProviderUnconfigured {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeFailureAvailability::Unavailable => OperationAdapterError::ProviderUnavailable {
                operation_id: operation_id.to_owned(),
                message,
            },
        };
    }
    OperationAdapterError::OperationUnavailable {
        operation_id: operation_id.to_owned(),
        message,
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn looks_like_not_found(value: &str) -> bool {
    contains_any(
        value,
        &[
            "not found",
            "no such file or directory",
            "path not found",
            "missing file",
        ],
    )
}

fn looks_like_validation_failure(value: &str) -> bool {
    contains_any(
        value,
        &[
            "invalid",
            "parse ",
            "parse:",
            "must not",
            "must be",
            "validation",
            "failed to import account",
        ],
    )
}

fn runtime_output_error(
    code: &str,
    operation_id: &str,
    class: &str,
    message: &str,
    exit_code: CliExitCode,
) -> OutputError {
    let mut error = OutputError::new(code, message.to_owned(), exit_code);
    error.detail = Some(json!({
        "operation_id": operation_id,
        "class": class,
    }));
    error
}

fn runtime_output_error_with_detail(
    code: &str,
    operation_id: &str,
    class: &str,
    message: &str,
    exit_code: CliExitCode,
    detail_json: &str,
) -> OutputError {
    let mut error = OutputError::new(code, message.to_owned(), exit_code);
    let mut detail = serde_json::from_str::<Map<String, Value>>(detail_json).unwrap_or_default();
    detail.insert(
        "operation_id".to_owned(),
        Value::from(operation_id.to_owned()),
    );
    detail.insert("class".to_owned(), Value::from(class.to_owned()));
    error.detail = Some(Value::Object(detail));
    error
}

macro_rules! target_operation_contracts {
    ($( $variant:ident => ($request:ident, $result:ident, $operation_id:literal) ),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq)]
        pub enum TargetOperationRequest {
            $( $variant(OperationRequest<$request>), )+
        }

        impl TargetOperationRequest {
            pub fn from_target_args(args: &TargetCliArgs) -> Result<Self, OperationAdapterError> {
                Self::from_operation_id_with_input(
                    args.command.operation_id(),
                    OperationContext::from_target_args(args),
                    target_operation_input(&args.command),
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

fn value_to_data(value: Value) -> OperationData {
    match value {
        Value::Object(map) => map,
        other => {
            let mut map = OperationData::new();
            map.insert("value".to_owned(), other);
            map
        }
    }
}

fn target_operation_input(command: &crate::target_cli::TargetCommand) -> OperationData {
    use crate::target_cli::{
        AccountCommand, AccountSelectionCommand, BasketCommand, BasketItemCommand,
        BasketQuoteCommand, FarmCommand, FarmFulfillmentCommand, FarmLocationCommand,
        FarmProfileCommand, ListingCommand, MarketCommand, MarketListingCommand,
        MarketProductCommand, OrderCommand, OrderEventCommand, OrderStatusCommand, TargetCommand,
    };

    let mut input = OperationData::new();
    match command {
        TargetCommand::Account(args) => match &args.command {
            AccountCommand::Import(args) => {
                insert_path(&mut input, "path", &args.path);
                if args.default {
                    input.insert("default".to_owned(), Value::Bool(true));
                }
            }
            AccountCommand::Get(args) => insert_string(&mut input, "selector", &args.selector),
            AccountCommand::Remove(args) => insert_string(&mut input, "selector", &args.selector),
            AccountCommand::Selection(args) => match &args.command {
                AccountSelectionCommand::Update(args) => {
                    insert_string(&mut input, "selector", &args.selector)
                }
                AccountSelectionCommand::Get | AccountSelectionCommand::Clear => {}
            },
            AccountCommand::Create | AccountCommand::List => {}
        },
        TargetCommand::Farm(args) => match &args.command {
            FarmCommand::Create(args) => {
                insert_string(&mut input, "farm_d_tag", &args.farm_d_tag);
                insert_string(&mut input, "name", &args.name);
                insert_string(&mut input, "display_name", &args.display_name);
                insert_string(&mut input, "about", &args.about);
                insert_string(&mut input, "website", &args.website);
                insert_string(&mut input, "picture", &args.picture);
                insert_string(&mut input, "banner", &args.banner);
                insert_string(&mut input, "location", &args.location);
                insert_string(&mut input, "city", &args.city);
                insert_string(&mut input, "region", &args.region);
                insert_string(&mut input, "country", &args.country);
                insert_string(&mut input, "delivery_method", &args.delivery_method);
            }
            FarmCommand::Profile(args) => match &args.command {
                FarmProfileCommand::Update(args) => {
                    insert_string(&mut input, "field", &args.field);
                    insert_string(&mut input, "value", &args.value);
                }
            },
            FarmCommand::Location(args) => match &args.command {
                FarmLocationCommand::Update(args) => {
                    insert_string(&mut input, "field", &args.field);
                    insert_string(&mut input, "value", &args.value);
                }
            },
            FarmCommand::Fulfillment(args) => match &args.command {
                FarmFulfillmentCommand::Update(args) => {
                    insert_string(&mut input, "value", &args.value);
                }
            },
            FarmCommand::Get | FarmCommand::Readiness(_) | FarmCommand::Publish => {}
        },
        TargetCommand::Listing(args) => match &args.command {
            ListingCommand::Create(args) => {
                insert_path(&mut input, "output", &args.output);
                insert_string(&mut input, "key", &args.key);
                insert_string(&mut input, "title", &args.title);
                insert_string(&mut input, "category", &args.category);
                insert_string(&mut input, "summary", &args.summary);
                insert_string(&mut input, "bin_id", &args.bin_id);
                insert_string(&mut input, "quantity_amount", &args.quantity_amount);
                insert_string(&mut input, "quantity_unit", &args.quantity_unit);
                insert_string(&mut input, "price_amount", &args.price_amount);
                insert_string(&mut input, "price_currency", &args.price_currency);
                insert_string(&mut input, "price_per_amount", &args.price_per_amount);
                insert_string(&mut input, "price_per_unit", &args.price_per_unit);
                insert_string(&mut input, "available", &args.available);
                insert_string(&mut input, "label", &args.label);
            }
            ListingCommand::Get(args) => insert_string(&mut input, "key", &args.key),
            ListingCommand::Update(args)
            | ListingCommand::Validate(args)
            | ListingCommand::Publish(args)
            | ListingCommand::Archive(args) => insert_path(&mut input, "file", &args.file),
            ListingCommand::List => {}
        },
        TargetCommand::Market(args) => match &args.command {
            MarketCommand::Product(product) => match &product.command {
                MarketProductCommand::Search(args) => {
                    insert_string_array(&mut input, "query", args.query.as_slice())
                }
            },
            MarketCommand::Listing(listing) => match &listing.command {
                MarketListingCommand::Get(args) => insert_string(&mut input, "key", &args.key),
            },
            MarketCommand::Refresh => {}
        },
        TargetCommand::Basket(args) => match &args.command {
            BasketCommand::Create(args) => {
                insert_string(&mut input, "basket_id", &args.basket_id);
                insert_string(&mut input, "listing", &args.listing);
                insert_string(&mut input, "listing_addr", &args.listing_addr);
                insert_string(&mut input, "bin_id", &args.bin_id);
                insert_string(&mut input, "quantity", &args.quantity);
            }
            BasketCommand::Get(args) | BasketCommand::Validate(args) => {
                insert_string(&mut input, "basket_id", &args.basket_id)
            }
            BasketCommand::Item(item) => match &item.command {
                BasketItemCommand::Add(args) | BasketItemCommand::Update(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id);
                    insert_string(&mut input, "item_id", &args.item_id);
                    insert_string(&mut input, "listing", &args.listing);
                    insert_string(&mut input, "listing_addr", &args.listing_addr);
                    insert_string(&mut input, "bin_id", &args.bin_id);
                    insert_string(&mut input, "quantity", &args.quantity);
                }
                BasketItemCommand::Remove(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id);
                    insert_string(&mut input, "item_id", &args.item_id);
                }
            },
            BasketCommand::Quote(quote) => match &quote.command {
                BasketQuoteCommand::Create(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id)
                }
            },
            BasketCommand::List => {}
        },
        TargetCommand::Order(args) => match &args.command {
            OrderCommand::Submit(args) => {
                insert_string(&mut input, "order_id", &args.order_id);
            }
            OrderCommand::Get(args) => insert_string(&mut input, "order_id", &args.order_id),
            OrderCommand::Accept(args) => insert_string(&mut input, "order_id", &args.order_id),
            OrderCommand::Decline(args) => {
                insert_string(&mut input, "order_id", &args.order_id);
                insert_string(&mut input, "reason", &args.reason);
            }
            OrderCommand::Status(status) => match &status.command {
                OrderStatusCommand::Get(args) => {
                    insert_string(&mut input, "order_id", &args.order_id)
                }
            },
            OrderCommand::Event(event) => match &event.command {
                OrderEventCommand::List(args) | OrderEventCommand::Watch(args) => {
                    insert_string(&mut input, "order_id", &args.order_id)
                }
            },
            OrderCommand::List => {}
        },
        _ => {}
    }
    input
}

fn insert_string(input: &mut OperationData, key: &str, value: &Option<String>) {
    if let Some(value) = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        input.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn insert_string_array(input: &mut OperationData, key: &str, values: &[String]) {
    let values = values
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Value::String(value.to_owned()))
        .collect::<Vec<_>>();
    if !values.is_empty() {
        input.insert(key.to_owned(), Value::Array(values));
    }
}

fn insert_path(input: &mut OperationData, key: &str, value: &Option<std::path::PathBuf>) {
    if let Some(value) = value {
        input.insert(
            key.to_owned(),
            Value::String(value.to_string_lossy().into_owned()),
        );
    }
}

target_operation_contracts! {
    WorkspaceInit => (WorkspaceInitRequest, WorkspaceInitResult, "workspace.init"),
    WorkspaceGet => (WorkspaceGetRequest, WorkspaceGetResult, "workspace.get"),
    HealthStatusGet => (HealthStatusGetRequest, HealthStatusGetResult, "health.status.get"),
    HealthCheckRun => (HealthCheckRunRequest, HealthCheckRunResult, "health.check.run"),
    ConfigGet => (ConfigGetRequest, ConfigGetResult, "config.get"),
    AccountCreate => (AccountCreateRequest, AccountCreateResult, "account.create"),
    AccountImport => (AccountImportRequest, AccountImportResult, "account.import"),
    AccountGet => (AccountGetRequest, AccountGetResult, "account.get"),
    AccountList => (AccountListRequest, AccountListResult, "account.list"),
    AccountRemove => (AccountRemoveRequest, AccountRemoveResult, "account.remove"),
    AccountSelectionGet => (AccountSelectionGetRequest, AccountSelectionGetResult, "account.selection.get"),
    AccountSelectionUpdate => (AccountSelectionUpdateRequest, AccountSelectionUpdateResult, "account.selection.update"),
    AccountSelectionClear => (AccountSelectionClearRequest, AccountSelectionClearResult, "account.selection.clear"),
    SignerStatusGet => (SignerStatusGetRequest, SignerStatusGetResult, "signer.status.get"),
    RelayList => (RelayListRequest, RelayListResult, "relay.list"),
    StoreInit => (StoreInitRequest, StoreInitResult, "store.init"),
    StoreStatusGet => (StoreStatusGetRequest, StoreStatusGetResult, "store.status.get"),
    StoreExport => (StoreExportRequest, StoreExportResult, "store.export"),
    StoreBackupCreate => (StoreBackupCreateRequest, StoreBackupCreateResult, "store.backup.create"),
    SyncStatusGet => (SyncStatusGetRequest, SyncStatusGetResult, "sync.status.get"),
    SyncPull => (SyncPullRequest, SyncPullResult, "sync.pull"),
    SyncPush => (SyncPushRequest, SyncPushResult, "sync.push"),
    SyncWatch => (SyncWatchRequest, SyncWatchResult, "sync.watch"),
    FarmCreate => (FarmCreateRequest, FarmCreateResult, "farm.create"),
    FarmGet => (FarmGetRequest, FarmGetResult, "farm.get"),
    FarmProfileUpdate => (FarmProfileUpdateRequest, FarmProfileUpdateResult, "farm.profile.update"),
    FarmLocationUpdate => (FarmLocationUpdateRequest, FarmLocationUpdateResult, "farm.location.update"),
    FarmFulfillmentUpdate => (FarmFulfillmentUpdateRequest, FarmFulfillmentUpdateResult, "farm.fulfillment.update"),
    FarmReadinessCheck => (FarmReadinessCheckRequest, FarmReadinessCheckResult, "farm.readiness.check"),
    FarmPublish => (FarmPublishRequest, FarmPublishResult, "farm.publish"),
    ListingCreate => (ListingCreateRequest, ListingCreateResult, "listing.create"),
    ListingGet => (ListingGetRequest, ListingGetResult, "listing.get"),
    ListingList => (ListingListRequest, ListingListResult, "listing.list"),
    ListingUpdate => (ListingUpdateRequest, ListingUpdateResult, "listing.update"),
    ListingValidate => (ListingValidateRequest, ListingValidateResult, "listing.validate"),
    ListingPublish => (ListingPublishRequest, ListingPublishResult, "listing.publish"),
    ListingArchive => (ListingArchiveRequest, ListingArchiveResult, "listing.archive"),
    MarketRefresh => (MarketRefreshRequest, MarketRefreshResult, "market.refresh"),
    MarketProductSearch => (MarketProductSearchRequest, MarketProductSearchResult, "market.product.search"),
    MarketListingGet => (MarketListingGetRequest, MarketListingGetResult, "market.listing.get"),
    BasketCreate => (BasketCreateRequest, BasketCreateResult, "basket.create"),
    BasketGet => (BasketGetRequest, BasketGetResult, "basket.get"),
    BasketList => (BasketListRequest, BasketListResult, "basket.list"),
    BasketItemAdd => (BasketItemAddRequest, BasketItemAddResult, "basket.item.add"),
    BasketItemUpdate => (BasketItemUpdateRequest, BasketItemUpdateResult, "basket.item.update"),
    BasketItemRemove => (BasketItemRemoveRequest, BasketItemRemoveResult, "basket.item.remove"),
    BasketValidate => (BasketValidateRequest, BasketValidateResult, "basket.validate"),
    BasketQuoteCreate => (BasketQuoteCreateRequest, BasketQuoteCreateResult, "basket.quote.create"),
    OrderSubmit => (OrderSubmitRequest, OrderSubmitResult, "order.submit"),
    OrderGet => (OrderGetRequest, OrderGetResult, "order.get"),
    OrderList => (OrderListRequest, OrderListResult, "order.list"),
    OrderAccept => (OrderAcceptRequest, OrderAcceptResult, "order.accept"),
    OrderDecline => (OrderDeclineRequest, OrderDeclineResult, "order.decline"),
    OrderStatusGet => (OrderStatusGetRequest, OrderStatusGetResult, "order.status.get"),
    OrderEventList => (OrderEventListRequest, OrderEventListResult, "order.event.list"),
    OrderEventWatch => (OrderEventWatchRequest, OrderEventWatchResult, "order.event.watch"),
}

pub fn adapter_registry_linkage_is_valid() -> bool {
    OPERATION_REGISTRY.iter().all(|operation| {
        TargetOperationRequest::request_type_for_operation(operation.operation_id)
            == Some(operation.rust_request)
            && TargetOperationResult::result_type_for_operation(operation.operation_id)
                == Some(operation.rust_result)
    })
}

#[cfg(test)]
mod tests {
    use std::io;

    use clap::Parser;
    use serde_json::json;

    use super::{
        OperationAdapter, OperationAdapterError, OperationContext, OperationInputMode,
        OperationNetworkMode, OperationOutputFormat, OperationRequest, OperationResult,
        OperationService, TargetOperationRequest, WorkspaceGetRequest, WorkspaceGetResult,
        adapter_registry_linkage_is_valid,
    };
    use crate::operation_registry::OPERATION_REGISTRY;
    use crate::runtime::RuntimeError;
    use crate::target_cli::TargetCliArgs;

    #[test]
    fn adapter_binds_every_registry_entry() {
        assert!(adapter_registry_linkage_is_valid());

        for operation in OPERATION_REGISTRY {
            let parsed = TargetCliArgs::try_parse_from(operation.cli_path.split_whitespace())
                .unwrap_or_else(|error| {
                    panic!("{} failed to parse: {error}", operation.cli_path);
                });
            let request = TargetOperationRequest::from_target_args(&parsed)
                .expect("operation request from target args");

            assert_eq!(request.operation_id(), operation.operation_id);
            assert_eq!(request.spec().mcp_tool, operation.mcp_tool);
            assert_eq!(request.request_type_name(), operation.rust_request);
            assert_eq!(
                TargetOperationRequest::request_type_for_operation(operation.operation_id),
                Some(operation.rust_request)
            );
        }
    }

    #[test]
    fn adapter_context_carries_target_global_scope() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "--format",
            "json",
            "--account-id",
            "acct_test",
            "--relay",
            "wss://relay.one",
            "--online",
            "--dry-run",
            "--idempotency-key",
            "idem_test",
            "--correlation-id",
            "corr_test",
            "--approval-token",
            "approval_test",
            "--no-input",
            "--quiet",
            "--verbose",
            "--trace",
            "--no-color",
            "workspace",
            "get",
        ])
        .expect("target args parse");

        let request = TargetOperationRequest::from_target_args(&parsed)
            .expect("operation request from target args");
        let context = request.context();

        assert_eq!(context.output_format, OperationOutputFormat::Json);
        assert_eq!(context.account_id.as_deref(), Some("acct_test"));
        assert_eq!(context.relays, vec!["wss://relay.one".to_owned()]);
        assert_eq!(context.network_mode, OperationNetworkMode::Online);
        assert!(context.dry_run);
        assert_eq!(context.idempotency_key.as_deref(), Some("idem_test"));
        assert_eq!(context.correlation_id.as_deref(), Some("corr_test"));
        assert_eq!(context.approval_token.as_deref(), Some("approval_test"));
        assert_eq!(context.input_mode, OperationInputMode::NoInput);
        assert!(context.quiet);
        assert!(context.verbose);
        assert!(context.trace);
        assert!(!context.color);

        let envelope_context = context.envelope_context("req_test");
        let actor = envelope_context.actor.expect("account actor");
        assert_eq!(actor.account_id, "acct_test");
        assert_eq!(actor.role, "account");
    }

    #[test]
    fn typed_service_boundary_returns_enveloped_result() {
        struct WorkspaceService;

        impl OperationService<WorkspaceGetRequest> for WorkspaceService {
            type Result = WorkspaceGetResult;

            fn execute(
                &self,
                request: OperationRequest<WorkspaceGetRequest>,
            ) -> Result<OperationResult<Self::Result>, super::OperationAdapterError> {
                assert_eq!(request.operation_id(), "workspace.get");
                OperationResult::new(WorkspaceGetResult::default())
            }
        }

        let adapter = OperationAdapter::new(WorkspaceService);
        let context = OperationContext::default();
        let request = OperationRequest::new(context.clone(), WorkspaceGetRequest::default())
            .expect("typed request");
        let result = adapter.execute(request).expect("typed result");
        let envelope = result
            .to_envelope(context.envelope_context("req_test"))
            .expect("operation envelope");

        assert_eq!(envelope.operation_id, "workspace.get");
        assert_eq!(envelope.kind, "workspace.get");
        assert_eq!(envelope.request_id, "req_test");
        assert_eq!(envelope.result, json!({}));
    }

    #[test]
    fn approval_errors_map_to_structured_exit_code() {
        let error = OperationAdapterError::approval_required("order.submit");
        let output_error = error.to_output_error();

        assert_eq!(output_error.code, "approval_required");
        assert_eq!(output_error.exit_code, 6);
        assert!(output_error.message.contains("approval_token"));
    }

    #[test]
    fn runtime_failures_map_to_specific_machine_codes() {
        let cases = [
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "no selected account for seller write".to_owned(),
                ),
                "account_unresolved",
                "account",
                5,
            ),
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "watch_only account cannot sign".to_owned(),
                ),
                "account_watch_only",
                "account",
                7,
            ),
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "selected local account pubkey `b` cannot sign listing seller_pubkey `a`"
                        .to_owned(),
                ),
                "account_mismatch",
                "account",
                5,
            ),
            (
                OperationAdapterError::unconfigured(
                    "listing.publish",
                    "signer.remote_nip46 binding is missing".to_owned(),
                ),
                "signer_unconfigured",
                "signer",
                7,
            ),
            (
                OperationAdapterError::unavailable(
                    "listing.publish",
                    "radrootsd bridge is unavailable".to_owned(),
                ),
                "provider_unavailable",
                "provider",
                3,
            ),
            (
                OperationAdapterError::SignerModeDeferred {
                    operation_id: "signer.status.get".to_owned(),
                    message: "signer mode `myc` is deferred".to_owned(),
                },
                "signer_mode_deferred",
                "signer",
                7,
            ),
            (
                OperationAdapterError::unconfigured(
                    "basket.quote.create",
                    "quote engine not ready".to_owned(),
                ),
                "operation_unavailable",
                "operation",
                3,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "listing.publish",
                    RuntimeError::Io(io::Error::new(io::ErrorKind::NotFound, "missing draft")),
                ),
                "not_found",
                "resource",
                4,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "listing.validate",
                    RuntimeError::Config("invalid listing draft listing.toml".to_owned()),
                ),
                "validation_failed",
                "validation",
                10,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "listing.archive",
                    RuntimeError::Config(
                        "selected local account pubkey `b` cannot sign listing seller_pubkey `a`"
                            .to_owned(),
                    ),
                ),
                "account_mismatch",
                "account",
                5,
            ),
            (
                OperationAdapterError::runtime_failure(
                    "farm.publish",
                    RuntimeError::Network("direct relay connection failed".to_owned()),
                ),
                "network_unavailable",
                "network",
                8,
            ),
        ];

        for (error, code, class, exit_code) in cases {
            let output = error.to_output_error();
            assert_eq!(output.code, code);
            assert_eq!(output.exit_code, exit_code);
            assert_eq!(
                output.detail.expect("detail")["class"],
                serde_json::Value::String(class.to_owned())
            );
        }
    }
}
