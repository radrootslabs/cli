use radroots_events::kinds::KIND_LISTING;
use radroots_events_codec::trade::RadrootsTradeListingAddress;
use serde::Serialize;
use serde_json::{Value, json};

use crate::domain::runtime::{FindView, ListingGetView, SyncActionView};
use crate::operation_adapter::{
    MarketListingGetRequest, MarketListingGetResult, MarketProductSearchRequest,
    MarketProductSearchResult, MarketRefreshRequest, MarketRefreshResult, OperationAdapterError,
    OperationRequest, OperationRequestData, OperationRequestPayload, OperationResult,
    OperationResultData, OperationService,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime_args::{FindQueryArgs, RecordLookupArgs};

pub struct MarketOperationService<'a> {
    config: &'a RuntimeConfig,
}

impl<'a> MarketOperationService<'a> {
    pub fn new(config: &'a RuntimeConfig) -> Self {
        Self { config }
    }
}

impl OperationService<MarketRefreshRequest> for MarketOperationService<'_> {
    type Result = MarketRefreshResult;

    fn execute(
        &self,
        request: OperationRequest<MarketRefreshRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        if request.context.dry_run {
            return json_operation_result::<MarketRefreshResult>(json!({
                "state": "dry_run",
                "source": "market refresh target operation",
                "actions": ["radroots sync status get"],
            }));
        }

        let view = market_refresh_view(map_runtime(crate::runtime::sync::pull(self.config))?);
        serialized_operation_result::<MarketRefreshResult, _>(&view)
    }
}

impl OperationService<MarketProductSearchRequest> for MarketOperationService<'_> {
    type Result = MarketProductSearchResult;

    fn execute(
        &self,
        request: OperationRequest<MarketProductSearchRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = FindQueryArgs {
            query: required_query_terms(&request)?,
        };
        let view = market_product_search_view(map_runtime(crate::runtime::find::search(
            self.config,
            &args,
        ))?);
        serialized_operation_result::<MarketProductSearchResult, _>(&view)
    }
}

impl OperationService<MarketListingGetRequest> for MarketOperationService<'_> {
    type Result = MarketListingGetResult;

    fn execute(
        &self,
        request: OperationRequest<MarketListingGetRequest>,
    ) -> Result<OperationResult<Self::Result>, OperationAdapterError> {
        let args = RecordLookupArgs {
            key: required_lookup(&request)?,
        };
        let view = market_listing_get_view(map_runtime(crate::runtime::listing::get(
            self.config,
            &args,
        ))?);
        serialized_operation_result::<MarketListingGetResult, _>(&view)
    }
}

fn market_refresh_view(mut view: SyncActionView) -> SyncActionView {
    view.actions = match view.state.as_str() {
        "ready" => vec!["radroots market product search tomatoes".to_owned()],
        "unavailable" => vec![
            "radroots runtime status get".to_owned(),
            "radroots sync status get".to_owned(),
        ],
        "unconfigured" => {
            let mut actions = Vec::new();
            if view.replica_db == "missing" {
                actions.push("radroots store init".to_owned());
            }
            if view.relay_count == 0 {
                actions.push("radroots relay list".to_owned());
            }
            if actions.is_empty() {
                actions.extend(std::mem::take(&mut view.actions));
            }
            actions
        }
        _ => std::mem::take(&mut view.actions),
    };
    view
}

fn market_product_search_view(mut view: FindView) -> FindView {
    view.actions = match view.state.as_str() {
        "ready" => view
            .results
            .first()
            .map(|result| {
                let mut actions = vec![format!(
                    "radroots market listing get {}",
                    result.product_key
                )];
                if listing_addr_can_back_basket(result.listing_addr.as_deref()) {
                    actions.push("radroots basket create".to_owned());
                    actions.push(format!("radroots basket item add {}", result.product_key));
                }
                actions
            })
            .unwrap_or_default(),
        "empty" => vec![
            "radroots market refresh".to_owned(),
            "radroots market product search eggs".to_owned(),
        ],
        "unconfigured" => vec![
            "radroots store init".to_owned(),
            "radroots market refresh".to_owned(),
        ],
        _ => std::mem::take(&mut view.actions),
    };
    view
}

fn market_listing_get_view(mut view: ListingGetView) -> ListingGetView {
    view.actions = match view.state.as_str() {
        "ready" => {
            let listing_key = view
                .product_key
                .as_deref()
                .unwrap_or(view.lookup.as_str())
                .to_owned();
            if listing_addr_can_back_basket(view.listing_addr.as_deref()) {
                vec![
                    "radroots basket create".to_owned(),
                    format!("radroots basket item add {listing_key}"),
                ]
            } else {
                Vec::new()
            }
        }
        "missing" => vec![
            "radroots market product search tomatoes".to_owned(),
            "radroots market refresh".to_owned(),
        ],
        "unconfigured" => vec![
            "radroots store init".to_owned(),
            "radroots market refresh".to_owned(),
        ],
        _ => std::mem::take(&mut view.actions),
    };
    view
}

fn listing_addr_can_back_basket(listing_addr: Option<&str>) -> bool {
    let Some(listing_addr) = listing_addr else {
        return false;
    };
    RadrootsTradeListingAddress::parse(listing_addr).is_ok_and(|parsed| parsed.kind == KIND_LISTING)
}

fn required_query_terms<P>(
    request: &OperationRequest<P>,
) -> Result<Vec<String>, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    let input = request.payload.input();
    let Some(value) = input.get("query").or_else(|| input.get("terms")) else {
        return Err(invalid_input(
            request.operation_id(),
            "missing required `query` input".to_owned(),
        ));
    };
    let terms = match value {
        Value::String(value) => value
            .split_whitespace()
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    invalid_input(
                        request.operation_id(),
                        "`query` array entries must be strings".to_owned(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(invalid_input(
                request.operation_id(),
                "`query` input must be a string or string array".to_owned(),
            ));
        }
    };

    if terms.is_empty() {
        return Err(invalid_input(
            request.operation_id(),
            "`query` input must not be empty".to_owned(),
        ));
    }
    Ok(terms)
}

fn required_lookup<P>(request: &OperationRequest<P>) -> Result<String, OperationAdapterError>
where
    P: OperationRequestPayload + OperationRequestData,
{
    string_input(request, "key")
        .or_else(|| string_input(request, "listing_id"))
        .or_else(|| string_input(request, "listing"))
        .ok_or_else(|| {
            invalid_input(
                request.operation_id(),
                "missing required `key` input".to_owned(),
            )
        })
}

fn serialized_operation_result<R, T>(value: &T) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
    T: Serialize,
{
    OperationResult::new(R::from_serializable(value)?)
}

fn json_operation_result<R>(value: Value) -> Result<OperationResult<R>, OperationAdapterError>
where
    R: OperationResultData,
{
    OperationResult::new(R::from_value(value))
}

fn map_runtime<T>(result: Result<T, RuntimeError>) -> Result<T, OperationAdapterError> {
    result.map_err(|error| OperationAdapterError::Runtime(error.to_string()))
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

    use super::MarketOperationService;
    use crate::operation_adapter::{
        MarketListingGetRequest, MarketProductSearchRequest, MarketRefreshRequest,
        OperationAdapter, OperationContext, OperationData, OperationRequest,
    };
    use crate::runtime::config::{
        AccountConfig, AccountSecretContractConfig, HyfConfig, IdentityConfig, InteractionConfig,
        LocalConfig, LoggingConfig, MigrationConfig, MycConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfig, RelayConfigSource, RelayPublishPolicy, RpcConfig, RuntimeConfig,
        SignerBackend, SignerConfig, Verbosity,
    };

    #[test]
    fn market_refresh_preserves_unconfigured_ingest_truth() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(MarketOperationService::new(&config));
        let request =
            OperationRequest::new(OperationContext::default(), MarketRefreshRequest::default())
                .expect("market refresh request");
        let envelope = service
            .execute(request)
            .expect("market refresh result")
            .to_envelope(OperationContext::default().envelope_context("req_market_refresh"))
            .expect("market refresh envelope");

        assert_eq!(envelope.operation_id, "market.refresh");
        assert_eq!(envelope.result["state"], "unconfigured");
        assert_eq!(envelope.result["direction"], "pull");
        assert_eq!(envelope.result["actions"][0], "radroots store init");
    }

    #[test]
    fn market_refresh_supports_dry_run() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(MarketOperationService::new(&config));
        let mut context = OperationContext::default();
        context.dry_run = true;
        let request = OperationRequest::new(context.clone(), MarketRefreshRequest::default())
            .expect("market refresh request");
        let envelope = service
            .execute(request)
            .expect("market refresh dry run")
            .to_envelope(context.envelope_context("req_market_refresh"))
            .expect("market refresh envelope");

        assert_eq!(envelope.operation_id, "market.refresh");
        assert_eq!(envelope.dry_run, true);
        assert_eq!(envelope.result["state"], "dry_run");
    }

    #[test]
    fn market_product_search_uses_find_runtime_without_top_level_find() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(MarketOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            MarketProductSearchRequest::from_data(data(&[("query", "eggs")])),
        )
        .expect("market product search request");
        let envelope = service
            .execute(request)
            .expect("market product search result")
            .to_envelope(OperationContext::default().envelope_context("req_market_search"))
            .expect("market product search envelope");

        assert_eq!(envelope.operation_id, "market.product.search");
        assert_eq!(envelope.result["state"], "unconfigured");
        assert_eq!(envelope.result["query"], "eggs");
        assert_eq!(envelope.result["actions"][0], "radroots store init");
    }

    #[test]
    fn market_listing_get_requires_lookup_key() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(MarketOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            MarketListingGetRequest::default(),
        )
        .expect("market listing get request");
        let error = service.execute(request).expect_err("key required");

        assert!(format!("{error}").contains("`key`"));
    }

    #[test]
    fn market_listing_get_wraps_listing_runtime_with_target_actions() {
        let dir = tempdir().expect("tempdir");
        let config = sample_config(dir.path());
        let service = OperationAdapter::new(MarketOperationService::new(&config));
        let request = OperationRequest::new(
            OperationContext::default(),
            MarketListingGetRequest::from_data(data(&[("key", "eggs")])),
        )
        .expect("market listing get request");
        let envelope = service
            .execute(request)
            .expect("market listing get result")
            .to_envelope(OperationContext::default().envelope_context("req_market_listing"))
            .expect("market listing get envelope");

        assert_eq!(envelope.operation_id, "market.listing.get");
        assert_eq!(envelope.result["state"], "unconfigured");
        assert_eq!(envelope.result["actions"][0], "radroots store init");
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
