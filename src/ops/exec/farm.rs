use serde::Serialize;
use serde_json::Value;

use crate::cli::global::{
    FarmCreateArgs, FarmFieldArg, FarmPublishArgs, FarmScopeArg, FarmScopedArgs, FarmUpdateArgs,
};
use crate::ops::{
    FarmCreateRequest, FarmCreateResult, FarmGetRequest, FarmGetResult, FarmListRequest,
    FarmListResult, FarmPublishRequest, FarmPublishResult, FarmUpdateRequest, FarmUpdateResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::{RuntimeConfig, TransportProfileKind};
use crate::view::runtime::{CommandDisposition, FarmPublishView};

pub struct FarmOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> FarmOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<FarmCreateRequest> for FarmOperationService<'_> {
    type Result = FarmCreateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmCreateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmCreateArgs {
            scope: scope_input(&request)?,
            farm_d_tag: string_input(&request, "farm_d_tag"),
            name: string_input(&request, "name"),
            display_name: string_input(&request, "display_name"),
            about: string_input(&request, "about"),
            website: string_input(&request, "website"),
            picture: string_input(&request, "picture"),
            banner: string_input(&request, "banner"),
            location: string_input(&request, "location"),
            city: string_input(&request, "city"),
            region: string_input(&request, "region"),
            country: string_input(&request, "country"),
            geohash: string_input(&request, "geohash"),
            delivery_method: string_input(&request, "delivery_method"),
        };
        if request.context.dry_run {
            let view =
                crate::runtime::farm::init_preflight(self.config, &args).map_err(|error| {
                    OperationAdapterError::runtime_failure(request.operation_id(), error)
                })?;
            return serialized_operation_result::<FarmCreateResult, _>(&view);
        }

        let view = crate::runtime::farm::init(self.config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        serialized_operation_result::<FarmCreateResult, _>(&view)
    }
}

impl OperationService<FarmGetRequest> for FarmOperationService<'_> {
    type Result = FarmGetResult;

    fn execute(
        &self,
        request: OperationRequest<FarmGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmScopedArgs {
            scope: scope_input(&request)?,
        };
        let view = map_runtime(crate::runtime::farm::get(self.config, &args))?;
        serialized_operation_result::<FarmGetResult, _>(&view)
    }
}

impl OperationService<FarmUpdateRequest> for FarmOperationService<'_> {
    type Result = FarmUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        farm_set::<FarmUpdateResult>(&request, self.config, profile_field(&request)?)
    }
}

impl OperationService<FarmListRequest> for FarmOperationService<'_> {
    type Result = FarmListResult;

    fn execute(
        &self,
        request: OperationRequest<FarmListRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmScopedArgs {
            scope: scope_input(&request)?,
        };
        let view = map_runtime(crate::runtime::farm::status(self.config, &args))?;
        serialized_operation_result::<FarmListResult, _>(&view)
    }
}

impl OperationService<FarmPublishRequest> for FarmOperationService<'_> {
    type Result = FarmPublishResult;

    fn execute(
        &self,
        request: OperationRequest<FarmPublishRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmPublishArgs {
            scope: scope_input(&request)?,
            idempotency_key: request
                .context
                .idempotency_key
                .clone()
                .or_else(|| string_input(&request, "idempotency_key")),
            print_event: bool_input(&request, "print_event").unwrap_or(false),
        };
        if matches!(
            self.config.transport.profile,
            TransportProfileKind::Nostr | TransportProfileKind::MultiTarget
        ) {
            require_nostr_delivery_target(&request, self.config)?;
        }

        let view = crate::runtime::farm::publish(self.config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        farm_publish_result(request.operation_id(), &view)
    }
}

fn farm_set<R>(
    request: &OperationRequest<impl OperationRequestData>,
    config: &RuntimeConfig,
    field: FarmFieldArg,
) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    let value = required_string(request, "value")?;
    let args = FarmUpdateArgs {
        scope: scope_input(request)?,
        field,
        value: vec![value.clone()],
    };
    if request.context.dry_run {
        let view = map_runtime(crate::runtime::farm::set_preflight(config, &args))?;
        return serialized_operation_result::<R, _>(&view);
    }

    let view = map_runtime(crate::runtime::farm::set(config, &args))?;
    serialized_operation_result::<R, _>(&view)
}

fn profile_field(
    request: &OperationRequest<impl OperationRequestData>,
) -> Result<FarmFieldArg, OperationAdapterError> {
    match string_input(request, "field").as_deref() {
        Some("name") | None => Ok(FarmFieldArg::Name),
        Some("display_name") | Some("display-name") => Ok(FarmFieldArg::DisplayName),
        Some("about") => Ok(FarmFieldArg::About),
        Some("website") => Ok(FarmFieldArg::Website),
        Some("picture") => Ok(FarmFieldArg::Picture),
        Some("banner") => Ok(FarmFieldArg::Banner),
        Some(other) => Err(invalid_input(
            request.operation_id(),
            format!("profile field `{other}` is not supported"),
        )),
    }
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn farm_publish_result(
    operation_id: &str,
    view: &FarmPublishView,
) -> Result<OperationResult<FarmPublishResult>, OperationAdapterError> {
    match view.disposition() {
        CommandDisposition::Success => serialized_operation_result::<FarmPublishResult, _>(view),
        CommandDisposition::ExternalUnavailable
            if farm_publish_transport_delivery_unavailable(view) =>
        {
            Err(OperationAdapterError::network_unavailable_with_detail(
                operation_id,
                view.reason.clone().unwrap_or_else(|| {
                    format!("farm publish finished with state `{}`", view.state)
                }),
                serde_json::to_value(view).unwrap_or(Value::Null),
            ))
        }
        disposition => Err(OperationAdapterError::from_command_disposition(
            operation_id,
            disposition,
            view.reason.clone().unwrap_or_else(|| match disposition {
                CommandDisposition::Success => "farm publish succeeded".to_owned(),
                CommandDisposition::NotFound => "farm publish target was not found".to_owned(),
                CommandDisposition::ValidationFailed => "farm publish validation failed".to_owned(),
                CommandDisposition::Unconfigured => "farm publish is unconfigured".to_owned(),
                CommandDisposition::ExternalUnavailable => "farm publish is unavailable".to_owned(),
                CommandDisposition::Unsupported => "farm publish is unsupported".to_owned(),
                CommandDisposition::InternalError => "farm publish failed".to_owned(),
            }),
        )),
    }
}

fn farm_publish_transport_delivery_unavailable(view: &FarmPublishView) -> bool {
    view.state == "partial"
        || !view.profile.failed_transport_targets.is_empty()
        || !view.farm.failed_transport_targets.is_empty()
}

fn require_nostr_delivery_target<P>(
    request: &OperationRequest<P>,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError>
where
    P: OperationRequestPayload,
{
    if !config.transport.nostr_relay_urls.is_empty() {
        return Ok(());
    }

    Err(OperationAdapterError::NetworkUnavailable {
        operation_id: request.operation_id().to_owned(),
        message: format!(
            "`{}` requires at least one configured Nostr relay in the active transport profile",
            request.spec.cli_path
        ),
    })
}

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
}

fn scope_input<P>(
    request: &OperationRequest<P>,
) -> Result<Option<FarmScopeArg>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    match string_input(request, "scope").as_deref() {
        Some("user") => Ok(Some(FarmScopeArg::User)),
        Some("workspace") => Ok(Some(FarmScopeArg::Workspace)),
        Some(other) => Err(invalid_input(
            request.operation_id(),
            format!("scope must be `user` or `workspace`, got `{other}`"),
        )),
        None => Ok(None),
    }
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
        .map(str::to_owned)
}

fn bool_input<P>(request: &OperationRequest<P>, key: &str) -> Option<bool>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request.payload.input().get(key).and_then(Value::as_bool)
}

fn invalid_input(operation_id: &str, message: String) -> OperationAdapterError {
    OperationAdapterError::InvalidInput {
        operation_id: operation_id.to_owned(),
        message,
    }
}
