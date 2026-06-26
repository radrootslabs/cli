use serde::Serialize;
use serde_json::Value;

use crate::cli::global::{
    FarmCreateArgs, FarmFieldArg, FarmPrivateLocationKeyArgs, FarmPrivateLocationSetArgs,
    FarmPrivateLocationSetInput, FarmPublishArgs, FarmRebindArgs, FarmScopeArg, FarmScopedArgs,
    FarmUpdateArgs,
};
use crate::ops::{
    FarmCreateRequest, FarmCreateResult, FarmFulfillmentUpdateRequest, FarmFulfillmentUpdateResult,
    FarmGetRequest, FarmGetResult, FarmLocationClearRequest, FarmLocationClearResult,
    FarmLocationGetRequest, FarmLocationGetResult, FarmLocationSetRequest, FarmLocationSetResult,
    FarmProfileUpdateRequest, FarmProfileUpdateResult, FarmPublishRequest, FarmPublishResult,
    FarmReadinessCheckRequest, FarmReadinessCheckResult, FarmRebindRequest, FarmRebindResult,
    OperationAdapterError, OperationRequest, OperationRequestData, OperationRequestPayload,
    OperationResult, OperationResultData, OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::{PublishTransport, RuntimeConfig};
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

impl OperationService<FarmRebindRequest> for FarmOperationService<'_> {
    type Result = FarmRebindResult;

    fn execute(
        &self,
        request: OperationRequest<FarmRebindRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmRebindArgs {
            scope: scope_input(&request)?,
            selector: required_string(&request, "selector")?,
        };
        if request.context.dry_run {
            let view =
                crate::runtime::farm::rebind_preflight(self.config, &args).map_err(|error| {
                    OperationAdapterError::runtime_failure(request.operation_id(), error)
                })?;
            return serialized_operation_result::<FarmRebindResult, _>(&view);
        }
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }

        let view = crate::runtime::farm::rebind(self.config, &args).map_err(|error| {
            OperationAdapterError::runtime_failure(request.operation_id(), error)
        })?;
        serialized_operation_result::<FarmRebindResult, _>(&view)
    }
}

impl OperationService<FarmProfileUpdateRequest> for FarmOperationService<'_> {
    type Result = FarmProfileUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmProfileUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        farm_set::<FarmProfileUpdateResult>(&request, self.config, profile_field(&request)?)
    }
}

impl OperationService<FarmLocationSetRequest> for FarmOperationService<'_> {
    type Result = FarmLocationSetResult;

    fn execute(
        &self,
        request: OperationRequest<FarmLocationSetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmPrivateLocationSetArgs {
            farm_d_tag: string_input(&request, "farm_d_tag"),
            input: farm_private_location_input(&request)?,
            label: string_input(&request, "label"),
        };
        let view =
            crate::runtime::farm::private_location_set(self.config, &args).map_err(|error| {
                OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
            })?;
        farm_private_location_set_result(request.operation_id(), &view)
    }
}

impl OperationService<FarmLocationGetRequest> for FarmOperationService<'_> {
    type Result = FarmLocationGetResult;

    fn execute(
        &self,
        request: OperationRequest<FarmLocationGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmPrivateLocationKeyArgs {
            farm_d_tag: string_input(&request, "farm_d_tag"),
        };
        let view =
            crate::runtime::farm::private_location_get(self.config, &args).map_err(|error| {
                OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
            })?;
        serialized_operation_result::<FarmLocationGetResult, _>(&view)
    }
}

impl OperationService<FarmLocationClearRequest> for FarmOperationService<'_> {
    type Result = FarmLocationClearResult;

    fn execute(
        &self,
        request: OperationRequest<FarmLocationClearRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }
        let args = FarmPrivateLocationKeyArgs {
            farm_d_tag: string_input(&request, "farm_d_tag"),
        };
        let view =
            crate::runtime::farm::private_location_clear(self.config, &args).map_err(|error| {
                OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
            })?;
        serialized_operation_result::<FarmLocationClearResult, _>(&view)
    }
}

impl OperationService<FarmFulfillmentUpdateRequest> for FarmOperationService<'_> {
    type Result = FarmFulfillmentUpdateResult;

    fn execute(
        &self,
        request: OperationRequest<FarmFulfillmentUpdateRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        farm_set::<FarmFulfillmentUpdateResult>(&request, self.config, FarmFieldArg::Delivery)
    }
}

impl OperationService<FarmReadinessCheckRequest> for FarmOperationService<'_> {
    type Result = FarmReadinessCheckResult;

    fn execute(
        &self,
        request: OperationRequest<FarmReadinessCheckRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FarmScopedArgs {
            scope: scope_input(&request)?,
        };
        let view = map_runtime(crate::runtime::farm::status(self.config, &args))?;
        serialized_operation_result::<FarmReadinessCheckResult, _>(&view)
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
        if request.context.requires_approval_token() {
            return Err(OperationAdapterError::approval_required(
                request.operation_id(),
            ));
        }
        if matches!(
            self.config.publish.transport,
            PublishTransport::DirectNostrRelay
        ) {
            require_relay_target(&request, self.config)?;
        }

        let view = crate::runtime::farm::publish(self.config, &args).map_err(|error| {
            OperationAdapterError::sdk_adapter_failure(request.operation_id(), error)
        })?;
        farm_publish_result(request.operation_id(), &view)
    }
}

fn farm_set<R>(
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
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
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
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

fn location_field(
    request: &OperationRequest<impl OperationRequestPayload + OperationRequestData>,
) -> Result<FarmFieldArg, OperationAdapterError> {
    match string_input(request, "field").as_deref() {
        Some("location") | None => Ok(FarmFieldArg::Location),
        Some("city") => Ok(FarmFieldArg::City),
        Some("region") => Ok(FarmFieldArg::Region),
        Some("country") => Ok(FarmFieldArg::Country),
        Some("geohash") => Ok(FarmFieldArg::Geohash),
        Some(other) => Err(invalid_input(
            request.operation_id(),
            format!("location field `{other}` is not supported"),
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
        CommandDisposition::ExternalUnavailable if farm_publish_relay_unavailable(view) => {
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

fn farm_private_location_set_result(
    operation_id: &str,
    view: &crate::view::runtime::FarmPrivateLocationView,
) -> Result<OperationResult<FarmLocationSetResult>, OperationAdapterError> {
    match view.state.as_str() {
        "no_match" => Err(OperationAdapterError::not_found_with_detail(
            operation_id,
            view.reason
                .clone()
                .unwrap_or_else(|| "GeoNames lookup returned no matching locality".to_owned()),
            serde_json::to_value(view).unwrap_or(Value::Null),
        )),
        "ambiguous" => Err(OperationAdapterError::validation_failed_with_detail(
            operation_id,
            view.reason
                .clone()
                .unwrap_or_else(|| "GeoNames lookup matched multiple localities".to_owned()),
            serde_json::to_value(view).unwrap_or(Value::Null),
        )),
        _ => serialized_operation_result::<FarmLocationSetResult, _>(view),
    }
}

fn farm_publish_relay_unavailable(view: &FarmPublishView) -> bool {
    view.state == "partial"
        || !view.profile.failed_relays.is_empty()
        || !view.farm.failed_relays.is_empty()
}

fn require_relay_target<P>(
    request: &OperationRequest<P>,
    config: &RuntimeConfig,
) -> Result<(), OperationAdapterError>
where
    P: OperationRequestPayload,
{
    if !config.relay.urls.is_empty() {
        return Ok(());
    }

    Err(OperationAdapterError::NetworkUnavailable {
        operation_id: request.operation_id().to_owned(),
        message: format!(
            "`{}` requires at least one configured relay for direct relay publication",
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

fn farm_private_location_input<P>(
    request: &OperationRequest<P>,
) -> Result<FarmPrivateLocationSetInput, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    if request.payload.input().contains_key("lookup") {
        return Err(invalid_input(
            request.operation_id(),
            "`lookup` is not a supported farm location input".to_owned(),
        ));
    }

    let lat = optional_f64(request, "lat")?;
    let lng = optional_f64(request, "lng")?;
    let city = trimmed_string_input(request, "city")?;
    let region = trimmed_string_input(request, "region")?;
    let country = trimmed_string_input(request, "country")?;
    let query = trimmed_string_input(request, "query")?;
    let geonames_id = optional_i64(request, "geonames_id")?;

    if (lat.is_some() || lng.is_some()) && !(lat.is_some() && lng.is_some()) {
        return Err(invalid_input(
            request.operation_id(),
            "`lat` and `lng` must be provided together".to_owned(),
        ));
    }
    if (region.is_some() || country.is_some()) && city.is_none() {
        return Err(invalid_input(
            request.operation_id(),
            "`region` and `country` require `city`".to_owned(),
        ));
    }

    let mode_count = usize::from(lat.is_some())
        + usize::from(city.is_some())
        + usize::from(query.is_some())
        + usize::from(geonames_id.is_some());

    if mode_count != 1 {
        return Err(invalid_input(
            request.operation_id(),
            "farm location requires exactly one of `lat`/`lng`, `city`, `query`, or `geonames_id`"
                .to_owned(),
        ));
    }

    if let (Some(latitude), Some(longitude)) = (lat, lng) {
        return Ok(FarmPrivateLocationSetInput::Exact {
            latitude,
            longitude,
        });
    }
    if let Some(city) = city {
        return Ok(FarmPrivateLocationSetInput::City {
            city,
            region,
            country,
        });
    }
    if let Some(query) = query {
        return Ok(FarmPrivateLocationSetInput::Query(query));
    }
    if let Some(geonames_id) = geonames_id {
        if geonames_id <= 0 {
            return Err(invalid_input(
                request.operation_id(),
                "`geonames_id` must be a positive integer".to_owned(),
            ));
        }
        return Ok(FarmPrivateLocationSetInput::GeonamesId(geonames_id));
    }

    Err(invalid_input(
        request.operation_id(),
        "farm location input could not be resolved".to_owned(),
    ))
}

fn optional_f64<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<Option<f64>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    request
        .payload
        .input()
        .get(key)
        .map(|value| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    invalid_input(
                        request.operation_id(),
                        format!("`{key}` must be a finite number"),
                    )
                })
        })
        .transpose()
}

fn optional_i64<P>(
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
                    format!("`{key}` must be an integer"),
                )
            })
        })
        .transpose()
}

fn trimmed_string_input<P>(
    request: &OperationRequest<P>,
    key: &str,
) -> Result<Option<String>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, key)
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(invalid_input(
                    request.operation_id(),
                    format!("`{key}` must not be empty"),
                ))
            } else {
                Ok(trimmed.to_owned())
            }
        })
        .transpose()
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use radroots_runtime_paths::RadrootsMigrationReport;
    use radroots_secret_vault::RadrootsSecretBackend;
    use serde_json::{Map, Value, json};
    use tempfile::tempdir;

    use super::FarmOperationService;
    use crate::ops::{
        FarmCreateRequest, FarmGetRequest, FarmLocationSetRequest, FarmPublishRequest,
        FarmReadinessCheckRequest, FarmRebindRequest, OperationAdapter, OperationContext,
        OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, PublishConfig, PublishTransport, PublishTransportSource, RelayConfig,
        RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig, SignerBackend,
        SignerConfig, Verbosity,
    };
    use crate::view::runtime::{
        FarmPrivateExactLocationView, FarmPrivateLocationCandidateView, FarmPrivateLocationView,
    };

    #[test]
    fn farm_service_reports_missing_farm_config() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let request = OperationRequest::new(OperationContext::default(), FarmGetRequest::default())
            .expect("farm get request");
        let envelope = service
            .execute(request)
            .expect("farm get result")
            .to_envelope(OperationContext::default().envelope_context("req_farm_get"))
            .expect("farm get envelope");

        assert_eq!(envelope.operation_id, "farm.get");
        assert_eq!(envelope.result["state"], "unconfigured");
        assert_eq!(envelope.result["config_present"], false);
    }

    #[test]
    fn farm_service_supports_create_and_readiness_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let mut context = OperationContext::default();
        context.dry_run = true;
        let request = OperationRequest::new(
            context.clone(),
            FarmCreateRequest::from_data(data(&[("name", "dry farm"), ("location", "earth")])),
        )
        .expect("farm create request");
        let envelope = service
            .execute(request)
            .expect("farm create result")
            .to_envelope(context.envelope_context("req_farm_create"))
            .expect("farm create envelope");

        assert_eq!(envelope.operation_id, "farm.create");
        assert_eq!(envelope.dry_run, true);
        assert_eq!(envelope.result["state"], "unconfigured");

        let readiness = OperationRequest::new(
            OperationContext::default(),
            FarmReadinessCheckRequest::default(),
        )
        .expect("farm readiness request");
        let readiness_envelope = service
            .execute(readiness)
            .expect("farm readiness result")
            .to_envelope(OperationContext::default().envelope_context("req_farm_ready"))
            .expect("farm readiness envelope");
        assert_eq!(readiness_envelope.operation_id, "farm.readiness.check");
        assert_eq!(readiness_envelope.result["state"], "unconfigured");
    }

    #[test]
    fn farm_publish_requires_approval_token_unless_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let request =
            OperationRequest::new(OperationContext::default(), FarmPublishRequest::default())
                .expect("farm publish request");
        let error = service.execute(request).expect_err("approval required");
        assert!(format!("{error}").contains("approval_token"));
        assert_eq!(error.to_output_error().code, "approval_required");
        assert_eq!(error.to_output_error().exit_code, 6);
    }

    #[test]
    fn farm_rebind_requires_approval_token_unless_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            FarmRebindRequest::from_data(data(&[("selector", "acct_test")])),
        )
        .expect("farm rebind request");
        let error = service.execute(request).expect_err("approval required");
        assert!(format!("{error}").contains("approval_token"));
        assert_eq!(error.to_output_error().code, "approval_required");
        assert_eq!(error.to_output_error().exit_code, 6);
    }

    #[test]
    fn farm_service_accepts_canonical_location_set_modes() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let cases = [
            value_data(&[
                ("lat", json!(48.429456)),
                ("lng", json!(-123.349786)),
                ("label", json!("farm gate")),
            ]),
            value_data(&[
                ("city", json!("Victoria")),
                ("region", json!("BC")),
                ("country", json!("CA")),
            ]),
            value_data(&[("query", json!("Victoria, BC"))]),
            value_data(&[("geonames_id", json!(6174041))]),
        ];

        for input in cases {
            let request = OperationRequest::new(
                OperationContext::default(),
                FarmLocationSetRequest::from_data(input),
            )
            .expect("farm location request");
            let envelope = service
                .execute(request)
                .expect("farm location set result")
                .to_envelope(OperationContext::default().envelope_context("req_farm_location"))
                .expect("farm location envelope");

            assert_eq!(envelope.operation_id, "farm.location.set");
            assert_eq!(envelope.result["state"], "unconfigured");
        }
    }

    #[test]
    fn farm_service_rejects_invalid_location_set_modes() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(FarmOperationService::new(&config));
        let cases = [
            (
                value_data(&[("lookup", json!("Victoria, BC"))]),
                "`lookup` is not a supported farm location input",
            ),
            (
                value_data(&[("lat", json!(48.429456))]),
                "`lat` and `lng` must be provided together",
            ),
            (
                value_data(&[
                    ("lat", json!(48.429456)),
                    ("lng", json!(-123.349786)),
                    ("city", json!("Victoria")),
                ]),
                "requires exactly one",
            ),
            (
                value_data(&[("query", json!("Victoria")), ("country", json!("CA"))]),
                "`region` and `country` require `city`",
            ),
            (
                value_data(&[("geonames_id", json!(0))]),
                "`geonames_id` must be a positive integer",
            ),
            (
                value_data(&[("city", json!(" "))]),
                "`city` must not be empty",
            ),
        ];

        for (input, expected) in cases {
            let request = OperationRequest::new(
                OperationContext::default(),
                FarmLocationSetRequest::from_data(input),
            )
            .expect("farm location request");
            let error = service
                .execute(request)
                .expect_err("invalid location input");

            assert!(format!("{error}").contains(expected));
        }
    }

    #[test]
    fn farm_location_set_maps_lookup_failures_to_output_errors() {
        let no_match = location_lookup_view("no_match", Vec::new());
        let no_match_error =
            super::farm_private_location_set_result("farm.location.set", &no_match)
                .expect_err("no match error")
                .to_output_error();
        assert_eq!(no_match_error.code, "not_found");
        assert_eq!(
            no_match_error.detail.as_ref().expect("no match detail")["state"],
            "no_match"
        );

        let ambiguous = location_lookup_view(
            "ambiguous",
            vec![FarmPrivateLocationCandidateView {
                geonames_feature_id: 3002,
                geonames_country_id: "CA".to_owned(),
                name: "Shared Market".to_owned(),
                display_name: "Shared Market, British Columbia, Canada".to_owned(),
                exact_location: FarmPrivateExactLocationView {
                    lat: 48.7,
                    lng: -123.2,
                },
                region: Some("British Columbia".to_owned()),
                country: Some("Canada".to_owned()),
            }],
        );
        let ambiguous_error =
            super::farm_private_location_set_result("farm.location.set", &ambiguous)
                .expect_err("ambiguous error")
                .to_output_error();
        assert_eq!(ambiguous_error.code, "validation_failed");
        assert_eq!(
            ambiguous_error.detail.as_ref().expect("ambiguous detail")["candidates"][0]["geonames_feature_id"],
            3002
        );
    }

    fn sample_config(root: &Path) -> RuntimeConfig {
        let data = root.join("data");
        let cache = root.join("cache");
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
                shared_cache_root: cache.clone(),
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
            publish: PublishConfig {
                transport: PublishTransport::DirectNostrRelay,
                source: PublishTransportSource::Defaults,
                radrootsd_proxy: crate::runtime::config::RadrootsdProxyConfig::default(),
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
            },
            rhi: crate::runtime::config::RhiConfig {
                trusted_worker_pubkeys: Vec::new(),
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

    fn value_data(entries: &[(&str, Value)]) -> OperationData {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect::<Map<String, Value>>()
    }

    fn location_lookup_view(
        state: &str,
        candidates: Vec<FarmPrivateLocationCandidateView>,
    ) -> FarmPrivateLocationView {
        FarmPrivateLocationView {
            state: state.to_owned(),
            source: "test".to_owned(),
            farm_addr: Some("30401:1111111111111111111111111111111111111111111111111111111111111111:AAAAAAAAAAAAAAAAAAAAAA".to_owned()),
            farm_d_tag: Some("AAAAAAAAAAAAAAAAAAAAAA".to_owned()),
            seller_account_id: Some("acct_test".to_owned()),
            seller_pubkey: Some("1111111111111111111111111111111111111111111111111111111111111111".to_owned()),
            label: None,
            exact_location: None,
            public_locality: None,
            geonames_feature_id: None,
            geonames_country_id: None,
            geonames_database_path: Some("cache/shared/geonames/geonames-1.0.db".to_owned()),
            cleared: None,
            candidates,
            reason: Some(format!("{state} reason")),
            actions: Vec::new(),
        }
    }
}
