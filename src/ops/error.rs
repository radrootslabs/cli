use std::io::ErrorKind;

use radroots_protocol::error::v1::{Class as ErrorClass, RecoveryAction};
use radroots_sdk::Error as SdkError;
use serde_json::{Map, Value, json};

use crate::out::envelope::{CliExitCode, OutputError};
use crate::runtime::RuntimeError;
use crate::runtime::account::AccountRuntimeFailure;
use crate::runtime::sdk::CliSdkAdapterError;
use crate::view::runtime::CommandDisposition;

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
    #[error("operation `{operation_id}` is not implemented: {message}")]
    NotImplemented {
        operation_id: String,
        message: String,
    },
    #[error("operation `{}` failed: {}", .0.operation_id, .0.message)]
    DetailedFailure(Box<OperationDetailedFailure>),
    #[error("operation runtime error: {0}")]
    Runtime(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDetailedFailure {
    pub operation_id: String,
    pub code: String,
    pub class: String,
    pub message: String,
    pub exit_code: CliExitCode,
    pub detail_json: String,
}

impl OperationAdapterError {
    fn detailed_failure(
        operation_id: String,
        code: String,
        class: String,
        message: String,
        exit_code: CliExitCode,
        detail_json: String,
    ) -> Self {
        Self::DetailedFailure(Box::new(OperationDetailedFailure {
            operation_id,
            code,
            class,
            message,
            exit_code,
            detail_json,
        }))
    }

    pub fn approval_required(operation_id: &str) -> Self {
        Self::ApprovalRequired {
            operation_id: operation_id.to_owned(),
            message: "missing required V1 approval proof or --yes confirmation".to_owned(),
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
            CommandDisposition::ValidationFailed => Self::ValidationFailed {
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
        Self::detailed_failure(
            operation_id.to_owned(),
            "operation_unavailable".to_owned(),
            "operation".to_owned(),
            message,
            CliExitCode::RuntimeUnavailable,
            detail.to_string(),
        )
    }

    pub fn not_found_with_detail(operation_id: &str, message: String, detail: Value) -> Self {
        Self::detailed_failure(
            operation_id.to_owned(),
            "not_found".to_owned(),
            "resource".to_owned(),
            message,
            CliExitCode::NotFound,
            detail.to_string(),
        )
    }

    pub fn not_implemented(operation_id: &str, message: String) -> Self {
        Self::NotImplemented {
            operation_id: operation_id.to_owned(),
            message,
        }
    }

    pub fn not_implemented_with_detail(operation_id: &str, message: String, detail: Value) -> Self {
        Self::detailed_failure(
            operation_id.to_owned(),
            "not_implemented".to_owned(),
            "operation".to_owned(),
            message,
            CliExitCode::RuntimeUnavailable,
            detail.to_string(),
        )
    }

    pub fn network_unavailable_with_detail(
        operation_id: &str,
        message: String,
        detail: Value,
    ) -> Self {
        Self::detailed_failure(
            operation_id.to_owned(),
            "network_unavailable".to_owned(),
            "network".to_owned(),
            message,
            CliExitCode::SyncOrNetworkFailure,
            detail.to_string(),
        )
    }

    pub fn validation_failed_with_detail(
        operation_id: &str,
        message: String,
        detail: Value,
    ) -> Self {
        Self::detailed_failure(
            operation_id.to_owned(),
            "validation_failed".to_owned(),
            "validation".to_owned(),
            message,
            CliExitCode::ValidationFailed,
            detail.to_string(),
        )
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
            RuntimeError::Account(failure) => account_runtime_failure(operation_id, failure),
            RuntimeError::Config(_)
                if contains_any(
                    &lowered,
                    &[
                        "no local account",
                        "account selector",
                        "account selection",
                        "account mismatch",
                        "did not match any local account",
                        "unresolved account",
                    ],
                ) =>
            {
                classify_runtime_failure(
                    operation_id,
                    message,
                    RuntimeFailureAvailability::Unconfigured,
                )
            }
            RuntimeError::Config(_) if looks_like_signer_failure(&lowered) => {
                Self::SignerUnconfigured {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeError::Config(_) if looks_like_validation_failure(&lowered) => {
                Self::ValidationFailed {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeError::Network(_) if looks_like_auth_failure(&lowered) => {
                auth_runtime_failure(operation_id, message, &lowered)
            }
            RuntimeError::Network(_) if looks_like_signer_failure(&lowered) => {
                Self::SignerUnavailable {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeError::Network(_) if looks_like_provider_failure(&lowered) => {
                Self::ProviderUnavailable {
                    operation_id: operation_id.to_owned(),
                    message,
                }
            }
            RuntimeError::Network(_) if looks_like_operation_failure(&lowered) => {
                Self::OperationUnavailable {
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

    pub fn sdk_adapter_failure(operation_id: &str, error: CliSdkAdapterError) -> Self {
        match error {
            CliSdkAdapterError::Runtime(error) => Self::runtime_failure(operation_id, error),
            CliSdkAdapterError::Sdk(error) => Self::sdk_failure(operation_id, error),
            CliSdkAdapterError::Sync(error) => Self::Runtime(error.to_string()),
            CliSdkAdapterError::Storage(error) => Self::Runtime(error.to_string()),
            CliSdkAdapterError::Transport(error) => Self::NetworkUnavailable {
                operation_id: operation_id.to_owned(),
                message: error.to_string(),
            },
            CliSdkAdapterError::Io(error) => Self::Runtime(error.to_string()),
        }
    }

    pub fn sdk_failure(operation_id: &str, error: SdkError) -> Self {
        let report = error.to_report();
        let code = report.code().as_str().to_owned();
        let class = sdk_error_class_name(report.class()).to_owned();
        let message = report.message().as_str().to_owned();
        let exit_code = sdk_error_exit_code(report.class());
        let mut detail = serde_json::to_value(&report).unwrap_or_else(|_| json!({}));
        let actions = sdk_recovery_next_actions(operation_id, report.recovery_actions());
        if !actions.is_empty()
            && let Some(detail) = detail.as_object_mut()
        {
            detail.insert(
                "actions".to_owned(),
                Value::Array(actions.into_iter().map(Value::String).collect()),
            );
        }
        Self::detailed_failure(
            operation_id.to_owned(),
            code,
            class,
            message,
            exit_code,
            detail.to_string(),
        )
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
            Self::NotImplemented {
                operation_id,
                message,
            } => runtime_output_error(
                "not_implemented",
                operation_id,
                "operation",
                message,
                CliExitCode::RuntimeUnavailable,
            ),
            Self::DetailedFailure(failure) => runtime_output_error_with_detail(
                failure.code.as_str(),
                failure.operation_id.as_str(),
                failure.class.as_str(),
                failure.message.as_str(),
                failure.exit_code,
                failure.detail_json.as_str(),
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

fn sdk_error_exit_code(class: ErrorClass) -> CliExitCode {
    match class {
        ErrorClass::Authorization | ErrorClass::Security => CliExitCode::AuthorizationFailed,
        ErrorClass::Validation | ErrorClass::Contract | ErrorClass::Conflict => {
            CliExitCode::InvalidInput
        }
        ErrorClass::Storage
        | ErrorClass::Resource
        | ErrorClass::Capability
        | ErrorClass::Maintenance => CliExitCode::RuntimeUnavailable,
        ErrorClass::Network | ErrorClass::Sync => CliExitCode::SyncOrNetworkFailure,
        _ => CliExitCode::InternalError,
    }
}

fn sdk_error_class_name(class: ErrorClass) -> &'static str {
    match class {
        ErrorClass::Validation => "validation",
        ErrorClass::Contract => "contract",
        ErrorClass::Storage => "storage",
        ErrorClass::Resource => "resource",
        ErrorClass::Conflict => "conflict",
        ErrorClass::Operation => "operation",
        ErrorClass::Authorization => "authorization",
        ErrorClass::Signer => "signer",
        ErrorClass::Network => "network",
        ErrorClass::Sync => "sync",
        ErrorClass::Runtime => "runtime",
        ErrorClass::Projection => "projection",
        ErrorClass::Query => "query",
        ErrorClass::Capability => "capability",
        ErrorClass::Privacy => "privacy",
        ErrorClass::Security => "security",
        ErrorClass::Maintenance => "maintenance",
        ErrorClass::Internal => "internal",
        ErrorClass::Unknown => "unknown",
    }
}

fn sdk_recovery_next_actions(
    operation_id: &str,
    recovery_actions: &[RecoveryAction],
) -> Vec<String> {
    recovery_actions
        .iter()
        .filter_map(|action| match action {
            RecoveryAction::RetryOperationWithSameIdempotencyKey | RecoveryAction::FixRequest => {
                Some(operation_retry_action(operation_id))
            }
            RecoveryAction::InspectLocalStores => Some("radroots store inspect".to_owned()),
            RecoveryAction::ConfigureStorage => Some("radroots store init".to_owned()),
            RecoveryAction::InspectGeoNamesAsset | RecoveryAction::ConfigureGeoNamesCache => {
                Some("radroots health inspect".to_owned())
            }
            RecoveryAction::ConfigureTransportTargets => {
                Some("radroots transport config inspect".to_owned())
            }
            RecoveryAction::ConfigureSigner => Some("radroots signer status".to_owned()),
            RecoveryAction::SelectAuthorizedActor => Some("radroots account list".to_owned()),
            RecoveryAction::CompleteSignerAuthentication => {
                Some("radroots signer status".to_owned())
            }
            RecoveryAction::RetryAfterTransportFailure | RecoveryAction::RetryGeoNamesDownload => {
                Some(operation_retry_action(operation_id))
            }
            RecoveryAction::EnableRequiredFeature | RecoveryAction::RecreateClient => {
                Some("radroots health inspect".to_owned())
            }
        })
        .fold(Vec::new(), |mut actions, action| {
            if !actions.contains(&action) {
                actions.push(action);
            }
            actions
        })
}

fn operation_retry_action(operation_id: &str) -> String {
    format!("radroots {}", operation_id.replace('.', " "))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeFailureAvailability {
    Unconfigured,
    Unavailable,
}

fn account_runtime_failure(
    operation_id: &str,
    failure: &AccountRuntimeFailure,
) -> OperationAdapterError {
    let message = failure.message().to_owned();
    match failure {
        AccountRuntimeFailure::Unresolved(_) => account_failure_output(
            operation_id,
            "account_unresolved",
            message,
            CliExitCode::AuthorizationFailed,
            failure.detail_json(),
            || OperationAdapterError::AccountUnresolved {
                operation_id: operation_id.to_owned(),
                message: failure.message().to_owned(),
            },
        ),
        AccountRuntimeFailure::WatchOnly(_) => account_failure_output(
            operation_id,
            "account_watch_only",
            message,
            CliExitCode::SignerUnavailable,
            failure.detail_json(),
            || OperationAdapterError::AccountWatchOnly {
                operation_id: operation_id.to_owned(),
                message: failure.message().to_owned(),
            },
        ),
        AccountRuntimeFailure::Mismatch(_) => account_failure_output(
            operation_id,
            "account_mismatch",
            message,
            CliExitCode::AuthorizationFailed,
            failure.detail_json(),
            || OperationAdapterError::AccountMismatch {
                operation_id: operation_id.to_owned(),
                message: failure.message().to_owned(),
            },
        ),
    }
}

fn account_failure_output(
    operation_id: &str,
    code: &str,
    message: String,
    exit_code: CliExitCode,
    detail_json: Option<&str>,
    default_error: impl FnOnce() -> OperationAdapterError,
) -> OperationAdapterError {
    match detail_json {
        Some(detail_json) => OperationAdapterError::detailed_failure(
            operation_id.to_owned(),
            code.to_owned(),
            "account".to_owned(),
            message,
            exit_code,
            detail_json.to_owned(),
        ),
        None => default_error(),
    }
}

fn auth_runtime_failure(
    operation_id: &str,
    message: String,
    lowered: &str,
) -> OperationAdapterError {
    let unauthorized = contains_any(
        lowered,
        &[
            "unauthorized",
            "forbidden",
            "permission denied",
            "invalid token",
            "bearer token rejected",
            "http 401",
            "http 403",
            "status 401",
            "status 403",
        ],
    );
    OperationAdapterError::detailed_failure(
        operation_id.to_owned(),
        if unauthorized {
            "auth_unauthorized".to_owned()
        } else {
            "auth_unavailable".to_owned()
        },
        "auth".to_owned(),
        message,
        CliExitCode::AuthorizationFailed,
        Value::Null.to_string(),
    )
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
    if contains_any(&lowered, &["account mismatch"]) {
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
    if contains_any(&lowered, &["provider", "write-plane", "write plane", "rpc"]) {
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

fn looks_like_auth_failure(value: &str) -> bool {
    contains_any(
        value,
        &[
            "authentication",
            "daemon execution auth",
            "authorization",
            "authorize",
            "unauthorized",
            "forbidden",
            "bearer token",
            "invalid token",
            "permission denied",
            "status 401",
            "status 403",
            "http 401",
            "http 403",
        ],
    )
}

fn looks_like_signer_failure(value: &str) -> bool {
    contains_any(
        value,
        &[
            "signer",
            "sign_event",
            "sign event",
            "signer session",
            "nip46",
            "nip-46",
            "remote_nip46",
        ],
    )
}

fn looks_like_provider_failure(value: &str) -> bool {
    contains_any(
        value,
        &[
            "provider unavailable",
            "provider unconfigured",
            "provider runtime",
            "provider failed",
            "execution provider",
        ],
    )
}

fn looks_like_operation_failure(value: &str) -> bool {
    contains_any(
        value,
        &[
            "method not found",
            "unknown method",
            "unsupported method",
            "unsupported operation",
            "operation unavailable",
            "operation disabled",
            "publish execution disabled",
            "publish.event is disabled",
        ],
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_sdk_error_maps_from_its_protocol_report() {
        let sdk_error = radroots_sdk::ClientBuilder::new()
            .build()
            .expect_err("missing storage must fail");
        let error = OperationAdapterError::sdk_failure("store.inspect", sdk_error);

        let output = error.to_output_error();
        let detail = output.detail.expect("detail");
        assert_eq!(detail["operation_id"], "store.inspect");
        assert!(detail["class"].is_string());
    }
}
