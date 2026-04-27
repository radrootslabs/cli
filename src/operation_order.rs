#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;

use crate::cli::{OrderSubmitArgs, OrderWatchArgs, RecordKeyArgs};
use crate::domain::runtime::{CommandDisposition, OrderSubmitView};
use crate::operation_adapter::{
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService, OrderEventListRequest,
    OrderEventListResult, OrderEventWatchRequest, OrderEventWatchResult, OrderGetRequest,
    OrderGetResult, OrderListRequest, OrderListResult, OrderSubmitRequest, OrderSubmitResult,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

pub struct OrderOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> OrderOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<OrderSubmitRequest> for OrderOperationService<'_> {
    type Result = OrderSubmitResult;

    fn execute(
        &self,
        request: OperationRequest<OrderSubmitRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if !request.context.dry_run && request.context.approval_token.is_none() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let key = required_order_key(&request)?;
        let args = OrderSubmitArgs {
            key,
            watch: bool_input(&request, "watch").unwrap_or(false),
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            signer_session_id: string_input(&request, "signer_session_id"),
        };
        let mut config = self.config.clone();
        if request.context.dry_run {
            config.output.dry_run = true;
        }
        let view = map_runtime(crate::runtime::order::submit(&config, &args))?;
        if request.context.dry_run {
            serialized_target_result::<OrderSubmitResult, _>(&view)
        } else {
            submit_result::<OrderSubmitResult>(request.operation_id(), &view)
        }
    }
}

impl OperationService<OrderGetRequest> for OrderOperationService<'_> {
    type Result = OrderGetResult;

    fn execute(
        &self,
        request: OperationRequest<OrderGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = RecordKeyArgs {
            key: required_order_key(&request)?,
        };
        let view = map_runtime(crate::runtime::order::get(self.config, &args))?;
        serialized_target_result::<OrderGetResult, _>(&view)
    }
}

impl OperationService<OrderListRequest> for OrderOperationService<'_> {
    type Result = OrderListResult;

    fn execute(
        &self,
        _request: OperationRequest<OrderListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::order::list(self.config))?;
        serialized_target_result::<OrderListResult, _>(&view)
    }
}

impl OperationService<OrderEventListRequest> for OrderOperationService<'_> {
    type Result = OrderEventListResult;

    fn execute(
        &self,
        _request: OperationRequest<OrderEventListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let view = map_runtime(crate::runtime::order::history(self.config))?;
        serialized_target_result::<OrderEventListResult, _>(&view)
    }
}

impl OperationService<OrderEventWatchRequest> for OrderOperationService<'_> {
    type Result = OrderEventWatchResult;

    fn execute(
        &self,
        request: OperationRequest<OrderEventWatchRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = OrderWatchArgs {
            key: required_order_key(&request)?,
            frames: usize_input(&request, "frames").or(Some(1)),
            interval_ms: u64_input(&request, "interval_ms").unwrap_or(1_000),
        };
        let view = map_runtime(crate::runtime::order::watch(self.config, &args))?;
        serialized_target_result::<OrderEventWatchResult, _>(&view)
    }
}

fn serialized_target_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    let mut value = serde_json::to_value(value)
        .map_err(|error| OperationAdapterError::Serialization(error.to_string()))?;
    translate_actions_in_value(&mut value);
    OperationResult::new(R::from_value(value))
}

fn submit_result<R>(
    operation_id: &str,
    view: &OrderSubmitView,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    match view.disposition() {
        CommandDisposition::Success => serialized_target_result::<R, _>(view),
        disposition => Err(disposition_error(
            operation_id,
            disposition,
            view.reason
                .clone()
                .unwrap_or_else(|| format!("order submit finished with state `{}`", view.state)),
        )),
    }
}

fn disposition_error(
    operation_id: &str,
    disposition: CommandDisposition,
    message: String,
) -> OperationAdapterError {
    match disposition {
        CommandDisposition::Success => OperationAdapterError::Runtime(message),
        CommandDisposition::Unconfigured | CommandDisposition::ExternalUnavailable => {
            OperationAdapterError::UnavailableOrUnconfigured {
                operation_id: operation_id.to_owned(),
                message,
            }
        }
        CommandDisposition::Unsupported => OperationAdapterError::InvalidInput {
            operation_id: operation_id.to_owned(),
            message,
        },
        CommandDisposition::InternalError => OperationAdapterError::Runtime(message),
    }
}

fn translate_actions_in_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Array(actions)) = object.get_mut("actions") {
                for action in actions {
                    if let Value::String(action) = action {
                        *action = target_action(action);
                    }
                }
            }
            for nested in object.values_mut() {
                translate_actions_in_value(nested);
            }
        }
        Value::Array(values) => {
            for nested in values {
                translate_actions_in_value(nested);
            }
        }
        _ => {}
    }
}

fn target_action(action: &str) -> String {
    match action {
        "radroots order ls" => "radroots order list".to_owned(),
        "radroots order new" | "radroots order create" => "radroots basket create".to_owned(),
        "radroots order history" => "radroots order event list".to_owned(),
        "radroots rpc status" => "radroots runtime status get".to_owned(),
        other if other.starts_with("radroots order watch ") => {
            other.replacen("radroots order watch ", "radroots order event watch ", 1)
        }
        other => other.to_owned(),
    }
}

fn required_order_key<P>(request: &OperationRequest<P>) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, "order_id")
        .or_else(|| string_input(request, "key"))
        .ok_or_else(|| {
            invalid_input(
                request.operation_id(),
                "missing required `order_id` input".to_owned(),
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
        .map(str::to_owned)
}

fn bool_input<P>(request: &OperationRequest<P>, key: &str) -> Option<bool>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_bool)
}

fn usize_input<P>(request: &OperationRequest<P>, key: &str) -> Option<usize>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn u64_input<P>(request: &OperationRequest<P>, key: &str) -> Option<u64>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_u64)
}

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
}

fn invalid_input(operation_id: &str, message: String) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value};
    use tempfile::tempdir;

    use super::OrderOperationService;
    use crate::operation_adapter::{
        OperationAdapter, OperationContext, OperationData, OperationRequest, OrderEventListRequest,
        OrderEventWatchRequest, OrderGetRequest, OrderListRequest, OrderSubmitRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn order_service_get_and_list_preserve_order_truth() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let get = OperationRequest::new(
            OperationContext::default(),
            OrderGetRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order get request");
        let get_envelope = service
            .execute(get)
            .expect("order get result")
            .to_envelope(OperationContext::default().envelope_context("req_order_get"))
            .expect("order get envelope");

        assert_eq!(get_envelope.operation_id, "order.get");
        assert_eq!(get_envelope.result["state"], "missing");
        assert_eq!(get_envelope.result["actions"][0], "radroots order list");
        assert_eq!(get_envelope.result["actions"][1], "radroots basket create");

        let list = OperationRequest::new(OperationContext::default(), OrderListRequest::default())
            .expect("order list request");
        let list_envelope = service
            .execute(list)
            .expect("order list result")
            .to_envelope(OperationContext::default().envelope_context("req_order_list"))
            .expect("order list envelope");
        assert_eq!(list_envelope.operation_id, "order.list");
        assert_eq!(list_envelope.result["state"], "empty");
        assert_eq!(list_envelope.result["actions"][0], "radroots basket create");
    }

    #[test]
    fn order_submit_requires_approval_token() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let submit = OperationRequest::new(
            OperationContext::default(),
            OrderSubmitRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order submit request");
        let error = service.execute(submit).expect_err("approval required");

        assert!(format!("{error}").contains("approval_token"));
        assert_eq!(error.to_output_error().code, "approval_required");
        assert_eq!(error.to_output_error().exit_code, 6);
    }

    #[test]
    fn order_submit_with_approval_preserves_missing_order_truth() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let mut context = OperationContext::default();
        context.approval_token = Some("approve_test".to_owned());
        let submit = OperationRequest::new(
            context.clone(),
            OrderSubmitRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order submit request");
        let envelope = service
            .execute(submit)
            .expect("order submit result")
            .to_envelope(context.envelope_context("req_order_submit"))
            .expect("order submit envelope");

        assert_eq!(envelope.operation_id, "order.submit");
        assert_eq!(envelope.result["state"], "missing");
        assert_eq!(envelope.result["actions"][0], "radroots order list");
    }

    #[test]
    fn order_event_list_wraps_history_without_legacy_action() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            OrderEventListRequest::default(),
        )
        .expect("order event list request");
        let envelope = service
            .execute(request)
            .expect("order event list result")
            .to_envelope(OperationContext::default().envelope_context("req_order_events"))
            .expect("order event list envelope");

        assert_eq!(envelope.operation_id, "order.event.list");
        assert_eq!(envelope.result["state"], "empty");
        assert_eq!(envelope.result["actions"][0], "radroots order list");
    }

    #[test]
    fn order_event_watch_reports_missing_order_with_target_actions() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(OrderOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            OrderEventWatchRequest::from_data(data(&[("order_id", "ord_missing")])),
        )
        .expect("order event watch request");
        let envelope = service
            .execute(request)
            .expect("order event watch result")
            .to_envelope(OperationContext::default().envelope_context("req_order_watch"))
            .expect("order event watch envelope");

        assert_eq!(envelope.operation_id, "order.event.watch");
        assert_eq!(envelope.result["state"], "missing");
        assert_eq!(envelope.result["actions"][0], "radroots order list");
    }

    fn sample_config(root: &Path) -> RuntimeConfig {
        let data = root.join("data");
        let logs = root.join("logs");
        let secrets = root.join("secrets");
        RuntimeConfig {
            output: OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Normal,
                color: true,
                dry_run: false,
            },
            interaction: InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: false,
                stdout_tty: false,
                prompts_allowed: false,
                confirmations_allowed: false,
            },
            paths: PathsConfig {
                profile: "interactive_user".into(),
                profile_source: "test".into(),
                allowed_profiles: vec!["interactive_user".into(), "repo_local".into()],
                root_source: "test".into(),
                repo_local_root: None,
                repo_local_root_source: None,
                subordinate_path_override_source: "runtime_config".into(),
                app_namespace: "apps/cli".into(),
                shared_accounts_namespace: "shared/accounts".into(),
                shared_identities_namespace: "shared/identities".into(),
                app_config_path: root.join("config/apps/cli/config.toml"),
                workspace_config_path: None,
                app_data_root: data.join("apps/cli"),
                app_logs_root: logs.join("apps/cli"),
                shared_accounts_data_root: data.join("shared/accounts"),
                shared_accounts_secrets_root: secrets.join("shared/accounts"),
                default_identity_path: secrets.join("shared/identities/default.json"),
            },
            migration: MigrationConfig {
                report: RadrootsMigrationReport::empty(),
            },
            logging: LoggingConfig {
                filter: "info".into(),
                directory: None,
                stdout: false,
            },
            account: AccountConfig {
                selector: None,
                store_path: data.join("shared/accounts/store.json"),
                secrets_dir: secrets.join("shared/accounts"),
                secret_backend: RadrootsSecretBackend::EncryptedFile,
                secret_fallback: None,
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: "host_vault".into(),
                default_fallback: Some("encrypted_file".into()),
                allowed_backends: vec!["host_vault".into(), "encrypted_file".into()],
                host_vault_policy: Some("desktop".into()),
                uses_protected_store: true,
            },
            identity: IdentityConfig {
                path: secrets.join("shared/identities/default.json"),
            },
            signer: SignerConfig {
                backend: SignerBackend::Local,
            },
            relay: RelayConfig {
                urls: Vec::new(),
                publish_policy: RelayPublishPolicy::Any,
                source: RelayConfigSource::Defaults,
            },
            local: LocalConfig {
                root: data.join("apps/cli/replica"),
                replica_db_path: data.join("apps/cli/replica/replica.sqlite"),
                backups_dir: data.join("apps/cli/replica/backups"),
                exports_dir: data.join("apps/cli/replica/exports"),
            },
            myc: MycConfig {
                executable: PathBuf::from("myc"),
                status_timeout_ms: 2_000,
            },
            hyf: HyfConfig {
                enabled: false,
                executable: PathBuf::from("hyfd"),
            },
            rpc: RpcConfig {
                url: "http://127.0.0.1:7070".into(),
                bridge_bearer_token: None,
            },
            capability_bindings: Vec::new(),
        }
    }

    fn data(entries: &[(&str, &str)]) -> OperationData {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
            .collect::<Map<String, Value>>()
    }
}
