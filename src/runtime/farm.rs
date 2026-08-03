use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_event::contract::AuthorRole;
use radroots_event::envelope::kind::{KIND_FARM, KIND_PROFILE};
use radroots_event::farm::{Farm, FarmPublicLocation};
use radroots_event::id::AddressableCoordinate;
use radroots_event::listing::operational::OperationalListingPublicLocation;
use radroots_event::profile::{AuthoredProfile, Nip05Identifier};
use radroots_event_codec::d_tag::is_d_tag_base64url;
use radroots_event_codec::profile::authored::authored_profile_to_wire_parts;
use radroots_sdk::farm::{self as sdk_farm, Plan as FarmPlan};
use radroots_signing::{Actor, actor::ActorSource};
use serde_json::json;

use crate::cli::global::{
    FarmCreateArgs, FarmFieldArg, FarmPrivateLocationKeyArgs, FarmPrivateLocationSetArgs,
    FarmPublishArgs, FarmRebindArgs, FarmScopeArg, FarmScopedArgs, FarmUpdateArgs,
};
use crate::runtime::RuntimeError;
use crate::runtime::account::{self, AccountRecordView};
use crate::runtime::config::{RuntimeConfig, SignerBackend, TransportProfileKind};
use crate::runtime::farm_config::{
    self, FarmConfigDocument, FarmConfigScope, FarmConfigSelection, FarmListingDefaults,
    FarmMissingField, FarmProfileDraft, FarmPublicationStatus, ResolvedFarmConfig,
    SUPPORTED_FARM_CONFIG_VERSION,
};
use crate::runtime::runtime_store::append_local_work;
use crate::runtime::sdk::{CliSdkAdapterError, validate_configured_signer_for_actor};
use crate::runtime::signer::ActorWriteBindingError;
use crate::view::runtime::{
    FarmConfigDocumentView, FarmConfigSummaryView, FarmGetView, FarmListingDefaultsView,
    FarmPrivateLocationView, FarmProfileDraftView, FarmPublicationView, FarmPublishComponentView,
    FarmPublishEventView, FarmPublishView, FarmRebindView, FarmSelectionView, FarmSetView,
    FarmSetupView, FarmStatusView,
};

const FARM_CONFIG_SOURCE: &str = "farm config · local first";
const FARM_SELLER_ACTOR_SOURCE: &str = "farm_config";
const SDK_FARM_WRITE_SOURCE: &str = "SDK farm publish · configured signer";
const SDK_PROFILE_NOT_SUBMITTED_METHOD: &str = "sdk.farm.profile.not_submitted";
const SDK_FARM_PUBLISH_METHOD: &str = "sdk.farm.publish.v1";
const SDK_FARM_PRIVATE_LOCATION_SOURCE: &str = "SDK private farm location · local private store";
const SDK_PROFILE_NOT_SUBMITTED_REASON: &str =
    "profile publish is not part of SDK farm.publish.v1; profile draft was not submitted";

static D_TAG_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn init(config: &RuntimeConfig, args: &FarmCreateArgs) -> Result<FarmSetupView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let Some(selected_account) = selected_account_for_draft(config)? else {
        return Ok(missing_selected_account_setup_view());
    };
    let existing = farm_config::load(config, Some(resolved_scope))?;
    let document = init_document(resolved_scope, &selected_account, existing.as_ref(), args)?;
    save_draft_view(
        "saved",
        resolved_scope,
        &selected_account,
        &document,
        Some("The farm draft is local until you publish it.".to_owned()),
        farm_setup_actions(config, &document, Some(&selected_account)),
        config,
    )
}

pub fn init_preflight(
    config: &RuntimeConfig,
    args: &FarmCreateArgs,
) -> Result<FarmSetupView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let Some(selected_account) = selected_account_for_draft(config)? else {
        return Ok(missing_selected_account_setup_view());
    };
    let existing = farm_config::load(config, Some(resolved_scope))?;
    let document = init_document(resolved_scope, &selected_account, existing.as_ref(), args)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    Ok(FarmSetupView {
        state: "dry_run".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        config: Some(summary_view(
            resolved_scope,
            path.display().to_string(),
            &document,
            Some(
                selected_account
                    .record
                    .public_identity()
                    .public_key()
                    .to_hex()
                    .as_str(),
            ),
        )),
        reason: Some("dry run requested; farm draft was not written".to_owned()),
        actions: farm_setup_actions(config, &document, Some(&selected_account)),
    })
}

pub fn rebind(
    config: &RuntimeConfig,
    args: &FarmRebindArgs,
) -> Result<FarmRebindView, RuntimeError> {
    rebind_inner(config, args, false)
}

pub fn rebind_preflight(
    config: &RuntimeConfig,
    args: &FarmRebindArgs,
) -> Result<FarmRebindView, RuntimeError> {
    rebind_inner(config, args, true)
}

fn rebind_inner(
    config: &RuntimeConfig,
    args: &FarmRebindArgs,
    dry_run: bool,
) -> Result<FarmRebindView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmRebindView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            scope: resolved_scope.as_str().to_owned(),
            path: path.display().to_string(),
            config_present: false,
            dry_run,
            seller_actor_source: FARM_SELLER_ACTOR_SOURCE.to_owned(),
            from_seller_account_id: None,
            from_seller_pubkey: None,
            to_seller_account_id: None,
            to_seller_pubkey: None,
            seller_pubkey_changed: None,
            publication_state_action: None,
            config: None,
            reason: Some(format!("no farm config found at {}", path.display())),
            actions: vec!["radroots farm create".to_owned()],
        });
    };

    let from_account = configured_account(config, &resolved.document.selection.account)?;
    let from_seller_pubkey = from_account
        .as_ref()
        .map(|account| account.record.public_identity().public_key().to_hex());
    let target_account = account::resolve_account_selector(config, args.selector.as_str())
        .map_err(|error| farm_rebind_selector_error(args.selector.as_str(), error))?;
    let to_seller_pubkey = target_account
        .record
        .public_identity()
        .public_key()
        .to_hex();
    let seller_pubkey_changed = from_seller_pubkey
        .as_deref()
        .is_none_or(|pubkey| !pubkey.eq_ignore_ascii_case(to_seller_pubkey.as_str()));
    let publication_state_action = if seller_pubkey_changed {
        "cleared"
    } else {
        "preserved"
    };
    let mut document = resolved.document.clone();
    document.selection.account = target_account.record.id().to_string();
    if seller_pubkey_changed {
        document.publication = FarmPublicationStatus::default();
    }
    let written_path = if dry_run {
        resolved.path.clone()
    } else {
        let written_path = farm_config::write(&config.paths, resolved.scope, &document)?;
        append_farm_local_work(
            config,
            resolved.scope,
            written_path.display().to_string(),
            &document,
            Some(to_seller_pubkey.as_str()),
        )?;
        written_path
    };
    let state = if dry_run { "dry_run" } else { "rebound" };

    Ok(FarmRebindView {
        state: state.to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: written_path.display().to_string(),
        config_present: true,
        dry_run,
        seller_actor_source: FARM_SELLER_ACTOR_SOURCE.to_owned(),
        from_seller_account_id: Some(resolved.document.selection.account.clone()),
        from_seller_pubkey,
        to_seller_account_id: Some(target_account.record.id().to_string()),
        to_seller_pubkey: Some(to_seller_pubkey.clone()),
        seller_pubkey_changed: Some(seller_pubkey_changed),
        publication_state_action: Some(publication_state_action.to_owned()),
        config: Some(summary_view(
            resolved.scope,
            written_path.display().to_string(),
            &document,
            Some(to_seller_pubkey.as_str()),
        )),
        reason: Some(if dry_run {
            "dry run requested; farm seller binding was not written".to_owned()
        } else {
            "farm seller binding updated".to_owned()
        }),
        actions: if dry_run {
            vec![farm_rebind_live_action(args.selector.as_str())]
        } else {
            vec!["radroots farm get".to_owned()]
        },
    })
}

fn farm_rebind_selector_error(selector: &str, error: RuntimeError) -> RuntimeError {
    match error {
        RuntimeError::Account(account::AccountRuntimeFailure::Unresolved(issue)) => {
            account::AccountRuntimeFailure::unresolved_with_detail(
                issue.message().to_owned(),
                json!({
                    "seller_actor_source": FARM_SELLER_ACTOR_SOURCE,
                    "selector": selector,
                    "actions": account_recovery_actions(),
                }),
            )
            .into()
        }
        other => other,
    }
}

pub fn set(config: &RuntimeConfig, args: &FarmUpdateArgs) -> Result<FarmSetView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(mut resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmSetView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            field: terminal_field_name(args.field).to_owned(),
            value: terminal_field_value(args.field, args.value.join(" ").trim()).to_owned(),
            config: None,
            reason: Some(format!("no farm draft found at {}", path.display())),
            actions: vec!["radroots farm create".to_owned()],
        });
    };

    let raw_value = args.value.join(" ");
    let field_value = required_text(raw_value.as_str(), "farm set value")?;
    apply_field_update(&mut resolved.document, args.field, field_value.as_str())?;
    let written_path = farm_config::write(&config.paths, resolved.scope, &resolved.document)?;
    let configured_account = configured_account(config, &resolved.document.selection.account)?;
    let account_public_key_hex = configured_account
        .as_ref()
        .map(|account| account.record.public_identity().public_key().to_hex());
    let account_pubkey = account_public_key_hex.as_deref();
    append_farm_local_work(
        config,
        resolved.scope,
        written_path.display().to_string(),
        &resolved.document,
        account_pubkey,
    )?;
    let reason = if configured_account.is_none() {
        Some(missing_farm_bound_seller_reason(
            resolved.document.selection.account.as_str(),
        ))
    } else {
        None
    };

    Ok(FarmSetView {
        state: "updated".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        field: terminal_field_name(args.field).to_owned(),
        value: terminal_field_value(args.field, field_value.as_str()).to_owned(),
        config: Some(summary_view(
            resolved.scope,
            written_path.display().to_string(),
            &resolved.document,
            account_pubkey,
        )),
        reason,
        actions: farm_update_actions(config, &resolved.document, configured_account.as_ref()),
    })
}

pub fn set_preflight(
    config: &RuntimeConfig,
    args: &FarmUpdateArgs,
) -> Result<FarmSetView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(mut resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmSetView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            field: terminal_field_name(args.field).to_owned(),
            value: terminal_field_value(args.field, args.value.join(" ").trim()).to_owned(),
            config: None,
            reason: Some(format!("no farm draft found at {}", path.display())),
            actions: vec!["radroots farm create".to_owned()],
        });
    };

    let raw_value = args.value.join(" ");
    let field_value = required_text(raw_value.as_str(), "farm set value")?;
    apply_field_update(&mut resolved.document, args.field, field_value.as_str())?;
    let configured_account = configured_account(config, &resolved.document.selection.account)?;
    let account_public_key_hex = configured_account
        .as_ref()
        .map(|account| account.record.public_identity().public_key().to_hex());
    let account_pubkey = account_public_key_hex.as_deref();
    let reason = if configured_account.is_none() {
        Some(format!(
            "dry run requested; farm draft was not written; {}",
            missing_farm_bound_seller_reason(resolved.document.selection.account.as_str())
        ))
    } else {
        Some("dry run requested; farm draft was not written".to_owned())
    };

    Ok(FarmSetView {
        state: "dry_run".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        field: terminal_field_name(args.field).to_owned(),
        value: terminal_field_value(args.field, field_value.as_str()).to_owned(),
        config: Some(summary_view(
            resolved.scope,
            path.display().to_string(),
            &resolved.document,
            account_pubkey,
        )),
        reason,
        actions: farm_update_actions(config, &resolved.document, configured_account.as_ref()),
    })
}

pub fn private_location_set(
    config: &RuntimeConfig,
    args: &FarmPrivateLocationSetArgs,
) -> Result<FarmPrivateLocationView, CliSdkAdapterError> {
    match private_location_target(config, args.farm_d_tag.as_deref())? {
        Some(_) => Err(RuntimeError::Config(
            "private farm location writes require a host-owned private artifact adapter".to_owned(),
        )
        .into()),
        None => Ok(private_location_unconfigured_view()),
    }
}

pub fn private_location_get(
    config: &RuntimeConfig,
    args: &FarmPrivateLocationKeyArgs,
) -> Result<FarmPrivateLocationView, CliSdkAdapterError> {
    match private_location_target(config, args.farm_d_tag.as_deref())? {
        Some(_) => Err(RuntimeError::Config(
            "private farm location reads require a host-owned private artifact adapter".to_owned(),
        )
        .into()),
        None => Ok(private_location_unconfigured_view()),
    }
}

pub fn private_location_clear(
    config: &RuntimeConfig,
    args: &FarmPrivateLocationKeyArgs,
) -> Result<FarmPrivateLocationView, CliSdkAdapterError> {
    match private_location_target(config, args.farm_d_tag.as_deref())? {
        Some(_) => Err(RuntimeError::Config(
            "private farm location deletion requires a host-owned private artifact adapter"
                .to_owned(),
        )
        .into()),
        None => Ok(private_location_unconfigured_view()),
    }
}

pub fn status(
    config: &RuntimeConfig,
    args: &FarmScopedArgs,
) -> Result<FarmStatusView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmStatusView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            scope: resolved_scope.as_str().to_owned(),
            path: path.display().to_string(),
            config_present: false,
            config_valid: false,
            account_state: "not_checked".to_owned(),
            listing_defaults_state: "missing".to_owned(),
            transport_profile: config.transport.profile.as_str().to_owned(),
            publish_state: "not_checked".to_owned(),
            publish_executable: false,
            publish_reason: None,
            config: None,
            missing: vec!["Farm draft".to_owned()],
            reason: Some(format!("no farm config found at {}", path.display())),
            actions: vec!["radroots farm create".to_owned()],
        });
    };

    let account = configured_account(config, &resolved.document.selection.account)?;
    let draft_missing = farm_config::missing_fields(&resolved.document);
    let account_state = if account.is_some() {
        "ready"
    } else {
        "missing"
    };
    let listing_defaults_state = if missing_blocks_listing_defaults(draft_missing.as_slice()) {
        "missing"
    } else {
        "ready"
    };
    let publish = account
        .as_ref()
        .filter(|_| draft_missing.is_empty())
        .map(|account| farm_publish_readiness(config, account))
        .unwrap_or_else(FarmPublishReadiness::not_checked);
    let state = if account.is_some() && draft_missing.is_empty() && publish.executable {
        "ready"
    } else {
        "unconfigured"
    };
    let reason = if account.is_none() {
        Some(format!(
            "farm config account `{}` is not present in the local account store",
            resolved.document.selection.account
        ))
    } else if !draft_missing.is_empty() {
        Some("farm draft is missing required fields".to_owned())
    } else {
        publish.reason.clone()
    };
    let mut actions = Vec::new();
    if account.is_none() {
        actions.extend(farm_bound_seller_recovery_actions("<selector>"));
    } else if draft_missing.is_empty() {
        actions.extend(publish.actions.clone());
    } else {
        actions.extend(missing_field_actions(draft_missing.as_slice()));
    }
    let account_public_key_hex = account
        .as_ref()
        .map(|account| account.record.public_identity().public_key().to_hex());
    let account_pubkey = account_public_key_hex.as_deref();

    Ok(FarmStatusView {
        state: state.to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        config_valid: true,
        account_state: account_state.to_owned(),
        listing_defaults_state: listing_defaults_state.to_owned(),
        transport_profile: config.transport.profile.as_str().to_owned(),
        publish_state: publish.state.to_owned(),
        publish_executable: publish.executable,
        publish_reason: publish.reason,
        config: Some(summary_view(
            resolved.scope,
            resolved.path.display().to_string(),
            &resolved.document,
            account_pubkey,
        )),
        missing: if account.is_none() {
            vec!["Farm-bound seller account".to_owned()]
        } else {
            let mut missing = missing_field_labels(draft_missing.as_slice());
            missing.extend(publish.missing);
            missing
        },
        reason,
        actions,
    })
}

pub fn get(config: &RuntimeConfig, args: &FarmScopedArgs) -> Result<FarmGetView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmGetView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            scope: resolved_scope.as_str().to_owned(),
            path: path.display().to_string(),
            config_present: false,
            document: None,
            reason: Some(format!("no farm config found at {}", path.display())),
            actions: vec!["radroots farm create".to_owned()],
        });
    };

    Ok(FarmGetView {
        state: "ready".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        document: Some(document_view(&resolved.document)),
        reason: None,
        actions: Vec::new(),
    })
}

#[derive(Debug, Clone)]
struct FarmPublishReadiness {
    state: &'static str,
    executable: bool,
    reason: Option<String>,
    missing: Vec<String>,
    actions: Vec<String>,
}

impl FarmPublishReadiness {
    fn not_checked() -> Self {
        Self {
            state: "not_checked",
            executable: false,
            reason: None,
            missing: Vec::new(),
            actions: Vec::new(),
        }
    }
}

fn farm_publish_readiness(
    config: &RuntimeConfig,
    account: &AccountRecordView,
) -> FarmPublishReadiness {
    transport_farm_publish_readiness(config, account)
}

fn transport_farm_publish_readiness(
    config: &RuntimeConfig,
    account: &AccountRecordView,
) -> FarmPublishReadiness {
    if matches!(
        config.transport.profile,
        TransportProfileKind::Nostr | TransportProfileKind::MultiTarget
    ) && config.transport.nostr_relay_urls.is_empty()
    {
        return FarmPublishReadiness {
            state: "unconfigured",
            executable: false,
            reason: Some("farm publish requires a configured Nostr transport profile".to_owned()),
            missing: vec!["Configured Nostr transport profile".to_owned()],
            actions: vec![
                "radroots transport config update --kind nostr --nostr-relay wss://relay.example.com"
                    .to_owned(),
            ],
        };
    }

    if matches!(config.signer.backend, SignerBackend::Myc) {
        if let Err(error) = validate_configured_signer_for_actor(
            config,
            Some(account.record.id().to_hex().as_str()),
            account
                .record
                .public_identity()
                .public_key()
                .to_hex()
                .as_str(),
            "farm seller",
        ) {
            return FarmPublishReadiness {
                state: "unconfigured",
                executable: false,
                reason: Some(error.to_string()),
                missing: vec!["Remote signer binding".to_owned()],
                actions: vec!["radroots signer status".to_owned()],
            };
        }
        return FarmPublishReadiness {
            state: "ready",
            executable: true,
            reason: None,
            missing: Vec::new(),
            actions: Vec::new(),
        };
    }

    if !account.write_capable {
        return FarmPublishReadiness {
            state: "unconfigured",
            executable: false,
            reason: Some(
                account::AccountRuntimeFailure::watch_only(&account.record.id()).to_string(),
            ),
            missing: vec!["Write-capable farm-bound seller account".to_owned()],
            actions: vec!["radroots account create".to_owned()],
        };
    }

    FarmPublishReadiness {
        state: "ready",
        executable: true,
        reason: None,
        missing: Vec::new(),
        actions: vec!["radroots farm publish".to_owned()],
    }
}

pub fn publish(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
) -> Result<FarmPublishView, CliSdkAdapterError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(missing_publish_view(
            config,
            resolved_scope,
            path.display().to_string(),
            args,
            format!("no farm config found at {}", path.display()),
            vec!["Farm draft".to_owned()],
            vec!["radroots farm create".to_owned()],
            config.output.dry_run,
            false,
            String::new(),
            String::new(),
            String::new(),
        ));
    };

    let Some(account) = configured_account(config, &resolved.document.selection.account)? else {
        return Ok(missing_publish_view(
            config,
            resolved.scope,
            resolved.path.display().to_string(),
            args,
            format!(
                "farm config account `{}` is not present in the local account store",
                resolved.document.selection.account
            ),
            vec!["Farm-bound seller account".to_owned()],
            farm_bound_seller_recovery_actions("<selector>"),
            config.output.dry_run,
            true,
            resolved.document.selection.account.clone(),
            String::new(),
            resolved.document.selection.farm_d_tag.clone(),
        ));
    };
    let draft_missing = farm_config::missing_fields(&resolved.document);
    if !draft_missing.is_empty() {
        return Ok(missing_publish_view(
            config,
            resolved.scope,
            resolved.path.display().to_string(),
            args,
            "farm draft is missing required fields".to_owned(),
            missing_field_labels(draft_missing.as_slice()),
            missing_field_actions(draft_missing.as_slice()),
            config.output.dry_run,
            true,
            resolved.document.selection.account.clone(),
            account.record.public_identity().public_key().to_hex(),
            resolved.document.selection.farm_d_tag.clone(),
        ));
    }
    let account_pubkey = account.record.public_identity().public_key().to_hex();
    let previews = build_publish_previews(&resolved.document, account_pubkey.as_str())?;
    let profile_idempotency_key = component_idempotency_key(args, "profile")?;
    let farm_idempotency_key = component_idempotency_key(args, "farm")?;

    publish_via_sdk(
        config,
        args,
        resolved,
        account_pubkey,
        previews,
        profile_idempotency_key,
        farm_idempotency_key,
    )
}

fn publish_via_sdk(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    mut resolved: ResolvedFarmConfig,
    account_pubkey: String,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
) -> Result<FarmPublishView, CliSdkAdapterError> {
    let input = sdk_farm_publish_input(&resolved, account_pubkey.as_str())?;
    if config.output.dry_run {
        if let Err(error) = validate_configured_signer_for_actor(
            config,
            Some(resolved.document.selection.account.as_str()),
            account_pubkey.as_str(),
            "farm seller",
        ) {
            let binding_error = ActorWriteBindingError::from_runtime(error);
            return match binding_error {
                ActorWriteBindingError::Account(failure) => Err(RuntimeError::from(failure).into()),
                error => Ok(binding_error_publish_view(
                    config,
                    args,
                    &resolved,
                    &account_pubkey,
                    previews,
                    profile_idempotency_key,
                    farm_idempotency_key,
                    error,
                )),
            };
        }

        let plan = sdk_farm::prepare(sdk_farm::PrepareRequest::new(
            input.actor,
            input.farm,
            now_unix(),
        ))
        .map_err(|error| RuntimeError::Config(format!("invalid SDK farm plan: {error}")))?;
        return Ok(sdk_prepared_publish_view(
            config,
            args,
            &resolved,
            account_pubkey.as_str(),
            previews,
            profile_idempotency_key,
            farm_idempotency_key,
            plan,
        ));
    }

    Err(RuntimeError::Config(
        "farm commit is unavailable until the shared sync engine is configured".to_owned(),
    )
    .into())
}

#[derive(Debug, Clone)]
struct SdkFarmPublishInput {
    actor: Actor,
    farm: Farm,
}

#[derive(Debug, Clone)]
struct PrivateFarmLocationTarget {
    actor: Actor,
    farm_addr: AddressableCoordinate,
    farm_d_tag: String,
    seller_account_id: String,
    seller_pubkey: String,
}

#[derive(Debug, Clone)]
struct FarmPublishPreviews {
    profile: FarmPublishEventDraft,
}

#[derive(Debug, Clone)]
struct FarmPublishEventDraft {
    event: FarmPublishEventView,
}

#[expect(
    clippy::too_many_arguments,
    reason = "publish view construction mirrors the V1 output contract fields"
)]
fn missing_publish_view(
    config: &RuntimeConfig,
    scope: FarmConfigScope,
    path: String,
    args: &FarmPublishArgs,
    reason: String,
    missing: Vec<String>,
    actions: Vec<String>,
    dry_run: bool,
    config_present: bool,
    seller_account_id: String,
    seller_pubkey: String,
    farm_d_tag: String,
) -> FarmPublishView {
    FarmPublishView {
        state: "unconfigured".to_owned(),
        source: farm_write_source(config).to_owned(),
        scope: scope.as_str().to_owned(),
        path,
        config_present,
        dry_run,
        seller_account_id,
        seller_pubkey,
        seller_actor_source: FARM_SELLER_ACTOR_SOURCE.to_owned(),
        farm_d_tag,
        profile: not_submitted_component(
            profile_publish_rpc_method(config),
            KIND_PROFILE,
            args,
            None,
            None,
        ),
        farm: not_submitted_component(farm_publish_rpc_method(config), KIND_FARM, args, None, None),
        local_replica: Vec::new(),
        missing,
        reason: Some(reason),
        actions,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "publish view construction mirrors the V1 output contract fields"
)]
fn base_publish_view(
    state: &str,
    config: &RuntimeConfig,
    _args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    profile: FarmPublishComponentView,
    farm: FarmPublishComponentView,
    reason: Option<String>,
    actions: Vec<String>,
) -> FarmPublishView {
    FarmPublishView {
        state: state.to_owned(),
        source: farm_write_source(config).to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        dry_run: config.output.dry_run,
        seller_account_id: resolved.document.selection.account.clone(),
        seller_pubkey: account_pubkey.to_owned(),
        seller_actor_source: FARM_SELLER_ACTOR_SOURCE.to_owned(),
        farm_d_tag: resolved.document.selection.farm_d_tag.clone(),
        profile,
        farm,
        local_replica: Vec::new(),
        missing: Vec::new(),
        reason,
        actions,
    }
}

fn build_publish_previews(
    document: &FarmConfigDocument,
    account_pubkey: &str,
) -> Result<FarmPublishPreviews, RuntimeError> {
    require_verified_publish_media(&document.profile, &document.farm)?;
    let profile = authored_profile_from_draft(&document.profile)?;
    let profile_parts = authored_profile_to_wire_parts(&profile)
        .map_err(|error| RuntimeError::Config(format!("invalid farm profile: {error}")))?;

    Ok(FarmPublishPreviews {
        profile: FarmPublishEventDraft {
            event: FarmPublishEventView {
                kind: profile_parts.kind,
                author: account_pubkey.to_owned(),
                content: profile_parts.content.clone(),
                tags: profile_parts.tags.clone(),
                event_id: None,
                event_addr: None,
            },
        },
    })
}

fn authored_profile_from_draft(draft: &FarmProfileDraft) -> Result<AuthoredProfile, RuntimeError> {
    let mut profile = AuthoredProfile::new(draft.name.clone())
        .map_err(|error| RuntimeError::Config(format!("invalid farm profile: {error}")))?;
    if let Some(display_name) = draft.display_name.as_deref().and_then(non_empty) {
        profile = profile.with_display_name(display_name);
    }
    if let Some(about) = draft.about.as_deref().and_then(non_empty) {
        profile = profile.with_about(about);
    }
    if let Some(nip05) = draft.nip05.as_deref().and_then(non_empty) {
        let identifier = Nip05Identifier::parse(nip05.as_str()).map_err(|error| {
            RuntimeError::Config(format!("invalid farm profile NIP-05 identifier: {error}"))
        })?;
        profile = profile.with_nip05(identifier);
    }
    if let Some(bot) = draft.bot.as_deref().and_then(non_empty) {
        let bot = match bot.as_str() {
            "true" => true,
            "false" => false,
            _ => {
                return Err(RuntimeError::Config(
                    "farm profile bot must be `true` or `false`".to_owned(),
                ));
            }
        };
        profile = profile.with_bot(bot);
    }
    Ok(profile)
}

fn require_verified_publish_media(
    profile: &FarmProfileDraft,
    farm: &Farm,
) -> Result<(), RuntimeError> {
    let contains_unverified_media = [
        profile.picture.as_deref(),
        profile.banner.as_deref(),
        farm.picture.as_deref(),
        farm.banner.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.trim().is_empty());
    if contains_unverified_media {
        return Err(RuntimeError::Config(
            "farm publish cannot use raw picture or banner URLs; publish media only after byte-verifying the Blossom descriptor and retaining proof of successful BUD-02 upload completion"
                .to_owned(),
        ));
    }
    Ok(())
}

fn component_idempotency_key(
    args: &FarmPublishArgs,
    component: &str,
) -> Result<Option<String>, RuntimeError> {
    args.idempotency_key
        .as_deref()
        .map(|value| {
            required_text(value, "idempotency_key")
                .and_then(|key| derive_component_idempotency_key(key.as_str(), component))
        })
        .transpose()
}

fn derive_component_idempotency_key(key: &str, component: &str) -> Result<String, RuntimeError> {
    if !is_uuid_v7_idempotency_key(key) {
        return Err(RuntimeError::Config(
            "idempotency_key must be a UUIDv7".to_owned(),
        ));
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in key
        .bytes()
        .chain(std::iter::once(b':'))
        .chain(component.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(format!("{}{:012x}", &key[..24], hash & 0xffff_ffff_ffff))
}

fn is_uuid_v7_idempotency_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes[14] == b'7'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
}

fn preview_component(
    rpc_method: &str,
    event_kind: u32,
    idempotency_key: Option<String>,
    args: &FarmPublishArgs,
    event: Option<FarmPublishEventView>,
) -> FarmPublishComponentView {
    FarmPublishComponentView {
        state: if event.is_some() {
            "not_submitted".to_owned()
        } else {
            "unconfigured".to_owned()
        },
        rpc_method: rpc_method.to_owned(),
        event_kind,
        deduplicated: false,
        target_transport_endpoints: Vec::new(),
        attempted_transport_endpoints: Vec::new(),
        accepted_transport_endpoints: Vec::new(),
        failed_transport_targets: Vec::new(),
        job_id: None,
        job_status: None,
        signer_mode: None,
        event_id: None,
        event_addr: event.as_ref().and_then(|event| event.event_addr.clone()),
        idempotency_key: idempotency_key.clone(),
        reason: Some("not submitted".to_owned()),
        job: None,
        event: args.print_event.then_some(event).flatten(),
    }
}

fn not_submitted_component(
    rpc_method: &str,
    event_kind: u32,
    args: &FarmPublishArgs,
    idempotency_key: Option<String>,
    event: Option<FarmPublishEventView>,
) -> FarmPublishComponentView {
    preview_component(rpc_method, event_kind, idempotency_key, args, event)
}

#[expect(
    clippy::too_many_arguments,
    reason = "publish view construction mirrors the V1 output contract fields"
)]
fn binding_error_publish_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
    error: ActorWriteBindingError,
) -> FarmPublishView {
    let reason = error.reason();
    let state = "unconfigured".to_owned();
    let actions = vec!["run radroots signer status".to_owned()];
    base_publish_view(
        state.as_str(),
        config,
        args,
        resolved,
        account_pubkey,
        FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..profile_not_submitted_component(
                profile_idempotency_key,
                args,
                Some(previews.profile.event),
            )
        },
        FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..preview_component(
                farm_publish_rpc_method(config),
                KIND_FARM,
                farm_idempotency_key,
                args,
                None,
            )
        },
        Some(reason),
        actions,
    )
}

fn sdk_farm_publish_input(
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
) -> Result<SdkFarmPublishInput, RuntimeError> {
    let actor = Actor::from_public_key_hex(
        account_pubkey,
        ActorSource::ExplicitPublicKey,
        [AuthorRole::Farmer],
    )
    .map_err(|error| RuntimeError::Config(format!("invalid farm SDK actor: {error}")))?;
    Ok(SdkFarmPublishInput {
        actor,
        farm: resolved.document.farm.clone(),
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "publish view construction mirrors the V1 output contract fields"
)]
fn sdk_prepared_publish_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
    plan: FarmPlan,
) -> FarmPublishView {
    base_publish_view(
        "dry_run",
        config,
        args,
        resolved,
        account_pubkey,
        profile_not_submitted_component(
            profile_idempotency_key,
            args,
            Some(previews.profile.event),
        ),
        FarmPublishComponentView {
            state: "not_submitted".to_owned(),
            reason: Some("dry run requested; SDK enqueue and transport push skipped".to_owned()),
            signer_mode: Some(config.signer.backend.as_str().to_owned()),
            event_id: Some(plan.draft().expected_event_id().to_string()),
            event_addr: Some(plan.coordinate().as_str().to_owned()),
            event: args.print_event.then_some(sdk_plan_event_view(&plan)),
            ..preview_component(
                farm_publish_rpc_method(config),
                KIND_FARM,
                farm_idempotency_key,
                args,
                None,
            )
        },
        Some("dry run requested; SDK enqueue and transport push skipped".to_owned()),
        vec!["radroots farm publish".to_owned()],
    )
}

fn sdk_plan_event_view(plan: &FarmPlan) -> FarmPublishEventView {
    FarmPublishEventView {
        kind: plan.draft().kind_u32(),
        author: plan.draft().expected_pubkey().to_hex(),
        content: plan.draft().content().to_owned(),
        tags: plan.draft().tags_as_vec(),
        event_id: Some(plan.draft().expected_event_id().to_string()),
        event_addr: Some(plan.coordinate().as_str().to_owned()),
    }
}

fn profile_not_submitted_component(
    idempotency_key: Option<String>,
    args: &FarmPublishArgs,
    event: Option<FarmPublishEventView>,
) -> FarmPublishComponentView {
    FarmPublishComponentView {
        reason: Some(SDK_PROFILE_NOT_SUBMITTED_REASON.to_owned()),
        ..preview_component(
            SDK_PROFILE_NOT_SUBMITTED_METHOD,
            KIND_PROFILE,
            idempotency_key,
            args,
            event,
        )
    }
}

fn farm_write_source(config: &RuntimeConfig) -> &'static str {
    let _ = config;
    SDK_FARM_WRITE_SOURCE
}

fn profile_publish_rpc_method(config: &RuntimeConfig) -> &'static str {
    let _ = config;
    SDK_PROFILE_NOT_SUBMITTED_METHOD
}

fn farm_publish_rpc_method(config: &RuntimeConfig) -> &'static str {
    let _ = config;
    SDK_FARM_PUBLISH_METHOD
}

fn selected_account_for_draft(
    config: &RuntimeConfig,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    account::resolve_account(config)
}

fn private_location_target(
    config: &RuntimeConfig,
    farm_d_tag: Option<&str>,
) -> Result<Option<PrivateFarmLocationTarget>, RuntimeError> {
    let resolved_scope = farm_config::resolve_scope(&config.paths, None)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(None);
    };
    let Some(account) = configured_account(config, &resolved.document.selection.account)? else {
        return Ok(None);
    };
    let seller_pubkey = account.record.public_identity().public_key().to_hex();
    let seller_account_id = account.record.id().to_string();
    let farm_d_tag = farm_d_tag
        .map(str::to_owned)
        .unwrap_or_else(|| resolved.document.selection.farm_d_tag.clone());
    let actor = Actor::from_public_key_hex(
        seller_pubkey.as_str(),
        ActorSource::ExplicitPublicKey,
        [AuthorRole::Farmer],
    )
    .map_err(|error| {
        RuntimeError::Config(format!("invalid farm private location actor: {error}"))
    })?;
    let farm_addr =
        AddressableCoordinate::parse(format!("{KIND_FARM}:{seller_pubkey}:{farm_d_tag}"))
            .map_err(|error| RuntimeError::Config(format!("invalid farm address: {error}")))?;
    Ok(Some(PrivateFarmLocationTarget {
        actor,
        farm_addr,
        farm_d_tag,
        seller_account_id,
        seller_pubkey,
    }))
}

fn missing_selected_account_setup_view() -> FarmSetupView {
    FarmSetupView {
        state: "unconfigured".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        config: None,
        reason: Some("choose or create an account before setting up your farm".to_owned()),
        actions: vec!["radroots account create".to_owned()],
    }
}

fn private_location_unconfigured_view() -> FarmPrivateLocationView {
    FarmPrivateLocationView {
        state: "unconfigured".to_owned(),
        source: SDK_FARM_PRIVATE_LOCATION_SOURCE.to_owned(),
        farm_addr: None,
        farm_d_tag: None,
        seller_account_id: None,
        seller_pubkey: None,
        label: None,
        exact_location: None,
        public_locality: None,
        geonames_feature_id: None,
        geonames_country_id: None,
        geonames_database_path: None,
        cleared: None,
        candidates: Vec::new(),
        reason: Some("create and bind a farm before storing private location".to_owned()),
        actions: vec![
            "radroots account create".to_owned(),
            "radroots farm create".to_owned(),
        ],
    }
}

fn init_document(
    scope: FarmConfigScope,
    account: &AccountRecordView,
    existing: Option<&ResolvedFarmConfig>,
    args: &FarmCreateArgs,
) -> Result<FarmConfigDocument, RuntimeError> {
    let existing_document = existing.map(|resolved| &resolved.document);
    if let Some(document) = existing_document
        && document.selection.account != account.record.id().to_string()
    {
        let message = format!(
            "account mismatch: farm config is bound to seller account `{}`; select account `{}` before updating this farm config",
            document.selection.account,
            account.record.id()
        );
        return Err(account::AccountRuntimeFailure::mismatch_with_detail(
            message,
            json!({
                "seller_actor_source": FARM_SELLER_ACTOR_SOURCE,
                "farm_bound_seller_account_id": document.selection.account,
                "attempted_seller_account_id": account.record.id().to_string(),
                "actions": farm_rebind_recovery_actions(account.record.id().to_hex().as_str()),
            }),
        )
        .into());
    }
    let farm_d_tag = match args.farm_d_tag.as_deref() {
        Some(value) => required_d_tag(value, "farm_d_tag")?,
        None => existing_document
            .map(|document| document.farm.d_tag.clone())
            .unwrap_or_else(generate_d_tag),
    };
    let existing_name = existing_name(existing_document);
    let existing_location = existing_location_primary(existing_document);
    let existing_city = existing_city(existing_document);
    let existing_region = existing_region(existing_document);
    let existing_country = existing_country(existing_document);
    let existing_geohash = existing_geohash(existing_document);
    let existing_delivery = existing_delivery_method(existing_document);
    let name = optional_arg_or_existing(args.name.as_ref(), existing_name.as_ref())
        .or_else(|| draft_name_from_account(account))
        .unwrap_or_default();
    let display_name = optional_arg_or_existing(
        args.display_name.as_ref(),
        existing_document.and_then(|document| document.profile.display_name.as_ref()),
    )
    .or_else(|| non_empty(name.as_str()));
    let about = optional_arg_or_existing(
        args.about.as_ref(),
        existing_document.and_then(|document| document.profile.about.as_ref()),
    );
    let website = optional_arg_or_existing(
        args.website.as_ref(),
        existing_document.and_then(|document| document.profile.website.as_ref()),
    );
    let picture = optional_arg_or_existing(
        args.picture.as_ref(),
        existing_document.and_then(|document| document.profile.picture.as_ref()),
    );
    let banner = optional_arg_or_existing(
        args.banner.as_ref(),
        existing_document.and_then(|document| document.profile.banner.as_ref()),
    );
    let location_primary =
        optional_arg_or_existing(args.location.as_ref(), existing_location.as_ref())
            .unwrap_or_default();
    let city = optional_arg_or_existing(args.city.as_ref(), existing_city.as_ref());
    let region = optional_arg_or_existing(args.region.as_ref(), existing_region.as_ref());
    let country = optional_arg_or_existing(args.country.as_ref(), existing_country.as_ref());
    let geohash = optional_arg_or_existing(args.geohash.as_ref(), existing_geohash.as_ref())
        .unwrap_or_default();
    let delivery_method =
        optional_arg_or_existing(args.delivery_method.as_ref(), existing_delivery.as_ref())
            .unwrap_or_default();
    let publication = publication_for_document(existing_document, account, farm_d_tag.as_str());

    Ok(FarmConfigDocument {
        version: SUPPORTED_FARM_CONFIG_VERSION,
        selection: FarmConfigSelection {
            scope,
            account: account.record.id().to_string(),
            farm_d_tag: farm_d_tag.clone(),
        },
        profile: FarmProfileDraft {
            name: name.clone(),
            display_name,
            nip05: None,
            about: about.clone(),
            website: website.clone(),
            picture: picture.clone(),
            banner: banner.clone(),
            lud06: None,
            lud16: None,
            bot: None,
        },
        farm: Farm {
            d_tag: farm_d_tag,
            name,
            about,
            website,
            picture,
            banner,
            location: Some(FarmPublicLocation {
                primary: location_primary.clone(),
                city: city.clone(),
                region: region.clone(),
                country: country.clone(),
                geohash: geohash.clone(),
            }),
            tags: None,
        },
        listing_defaults: FarmListingDefaults {
            delivery_method,
            location: OperationalListingPublicLocation {
                primary: location_primary,
                city,
                region,
                country,
                geohash,
            },
        },
        publication,
    })
}

fn save_draft_view(
    state: &str,
    scope: FarmConfigScope,
    account: &AccountRecordView,
    document: &FarmConfigDocument,
    reason: Option<String>,
    actions: Vec<String>,
    config: &RuntimeConfig,
) -> Result<FarmSetupView, RuntimeError> {
    let written_path = farm_config::write(&config.paths, scope, document)?;
    append_farm_local_work(
        config,
        scope,
        written_path.display().to_string(),
        document,
        Some(
            account
                .record
                .public_identity()
                .public_key()
                .to_hex()
                .as_str(),
        ),
    )?;
    Ok(FarmSetupView {
        state: state.to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        config: Some(summary_view(
            scope,
            written_path.display().to_string(),
            document,
            Some(
                account
                    .record
                    .public_identity()
                    .public_key()
                    .to_hex()
                    .as_str(),
            ),
        )),
        reason,
        actions,
    })
}

fn append_farm_local_work(
    config: &RuntimeConfig,
    scope: FarmConfigScope,
    path: String,
    document: &FarmConfigDocument,
    owner_pubkey: Option<&str>,
) -> Result<(), RuntimeError> {
    let payload = json!({
        "record_kind": "farm_config_v1",
        "scope": scope.as_str(),
        "path": path,
        "document": document,
    });
    let subject = format!("farm:{}", document.selection.farm_d_tag);
    append_local_work(
        config,
        subject.as_str(),
        Some(document.selection.account.clone()),
        owner_pubkey.map(str::to_owned),
        Some(document.selection.farm_d_tag.clone()),
        None,
        payload,
    )?;
    Ok(())
}

fn farm_update_actions(
    config: &RuntimeConfig,
    document: &FarmConfigDocument,
    account: Option<&AccountRecordView>,
) -> Vec<String> {
    farm_setup_actions(config, document, account)
}

fn farm_setup_actions(
    config: &RuntimeConfig,
    document: &FarmConfigDocument,
    account: Option<&AccountRecordView>,
) -> Vec<String> {
    let mut actions = vec!["radroots farm get".to_owned()];
    if account.is_none() {
        actions.extend(farm_bound_seller_recovery_actions("<selector>"));
        return actions;
    }
    if farm_config::missing_fields(document).is_empty()
        && account
            .map(|account| farm_publish_readiness(config, account).executable)
            .unwrap_or(false)
    {
        actions.push("radroots farm publish".to_owned());
    }
    actions
}

fn missing_farm_bound_seller_reason(account_id: &str) -> String {
    format!("farm-bound seller account `{account_id}` is not present in the local account store")
}

pub(crate) fn farm_bound_seller_recovery_actions(selector: &str) -> Vec<String> {
    let mut actions = vec!["radroots account import <path>".to_owned()];
    actions.extend(farm_rebind_recovery_actions(selector));
    actions
}

pub(crate) fn farm_rebind_recovery_actions(selector: &str) -> Vec<String> {
    vec![
        farm_rebind_dry_run_action(selector),
        farm_rebind_live_action(selector),
    ]
}

pub(crate) fn farm_rebind_dry_run_action(selector: &str) -> String {
    format!("radroots --dry-run account select {selector}")
}

pub(crate) fn farm_rebind_live_action(selector: &str) -> String {
    format!("radroots account select {selector}")
}

fn account_recovery_actions() -> Vec<String> {
    vec![
        "radroots account import <path>".to_owned(),
        "radroots account create".to_owned(),
    ]
}

fn missing_blocks_listing_defaults(missing: &[FarmMissingField]) -> bool {
    missing.iter().any(|field| {
        matches!(
            field,
            FarmMissingField::Location
                | FarmMissingField::City
                | FarmMissingField::Delivery
                | FarmMissingField::Geohash
        )
    })
}

fn missing_field_labels(missing: &[FarmMissingField]) -> Vec<String> {
    missing
        .iter()
        .map(|field| field.label().to_owned())
        .collect()
}

fn missing_field_actions(missing: &[FarmMissingField]) -> Vec<String> {
    let mut actions = Vec::new();
    for field in missing {
        match field {
            FarmMissingField::Name => {
                push_action(&mut actions, "radroots farm set name \"La Huerta Farm\"");
            }
            FarmMissingField::Location => {
                push_action(
                    &mut actions,
                    "radroots farm set location \"San Francisco, CA\"",
                );
            }
            FarmMissingField::City => {
                push_action(&mut actions, "radroots farm set city \"San Francisco\"");
            }
            FarmMissingField::Delivery => {
                push_action(&mut actions, "radroots farm set delivery pickup");
            }
            FarmMissingField::Country => {
                push_action(&mut actions, "radroots farm set country US");
            }
            FarmMissingField::Geohash => {
                push_action(&mut actions, "radroots farm set geohash 9q8yy");
            }
        }
    }
    actions
}

fn push_action(actions: &mut Vec<String>, action: &str) {
    if !actions.iter().any(|existing| existing == action) {
        actions.push(action.to_owned());
    }
}

fn terminal_field_name(field: FarmFieldArg) -> &'static str {
    match field {
        FarmFieldArg::Name => "Name",
        FarmFieldArg::DisplayName => "Display name",
        FarmFieldArg::About => "About",
        FarmFieldArg::Website => "Website",
        FarmFieldArg::Picture => "Picture",
        FarmFieldArg::Banner => "Banner",
        FarmFieldArg::Location => "Location",
        FarmFieldArg::City => "City",
        FarmFieldArg::Region => "Region",
        FarmFieldArg::Country => "Country",
        FarmFieldArg::Geohash => "Geohash",
        FarmFieldArg::Delivery => "Delivery",
    }
}

fn terminal_field_value(field: FarmFieldArg, value: &str) -> String {
    match field {
        FarmFieldArg::Delivery => display_delivery_method(value),
        _ => value.to_owned(),
    }
}

fn apply_field_update(
    document: &mut FarmConfigDocument,
    field: FarmFieldArg,
    value: &str,
) -> Result<(), RuntimeError> {
    let value = required_text(value, "farm set value")?;
    match field {
        FarmFieldArg::Name => {
            document.profile.name = value.clone();
            document.farm.name = value;
        }
        FarmFieldArg::DisplayName => {
            document.profile.display_name = Some(value);
        }
        FarmFieldArg::About => {
            document.profile.about = Some(value.clone());
            document.farm.about = Some(value);
        }
        FarmFieldArg::Website => {
            document.profile.website = Some(value.clone());
            document.farm.website = Some(value);
        }
        FarmFieldArg::Picture => {
            document.profile.picture = Some(value.clone());
            document.farm.picture = Some(value);
        }
        FarmFieldArg::Banner => {
            document.profile.banner = Some(value.clone());
            document.farm.banner = Some(value);
        }
        FarmFieldArg::Location => {
            document.listing_defaults.location.primary = value.clone();
            ensure_farm_location(document).primary = value;
        }
        FarmFieldArg::City => {
            document.listing_defaults.location.city = Some(value.clone());
            ensure_farm_location(document).city = Some(value);
        }
        FarmFieldArg::Region => {
            document.listing_defaults.location.region = Some(value.clone());
            ensure_farm_location(document).region = Some(value);
        }
        FarmFieldArg::Country => {
            document.listing_defaults.location.country = Some(value.clone());
            ensure_farm_location(document).country = Some(value);
        }
        FarmFieldArg::Geohash => {
            document.listing_defaults.location.geohash = value.clone();
            ensure_farm_location(document).geohash = value;
        }
        FarmFieldArg::Delivery => {
            document.listing_defaults.delivery_method = value;
        }
    }
    Ok(())
}

fn ensure_farm_location(document: &mut FarmConfigDocument) -> &mut FarmPublicLocation {
    let primary = document.listing_defaults.location.primary.clone();
    let city = document.listing_defaults.location.city.clone();
    let region = document.listing_defaults.location.region.clone();
    let country = document.listing_defaults.location.country.clone();
    let geohash = document.listing_defaults.location.geohash.clone();
    document.farm.location.get_or_insert(FarmPublicLocation {
        primary,
        city,
        region,
        country,
        geohash,
    })
}

fn publication_for_document(
    existing_document: Option<&FarmConfigDocument>,
    account: &AccountRecordView,
    farm_d_tag: &str,
) -> FarmPublicationStatus {
    existing_document
        .filter(|document| {
            document.farm.d_tag == farm_d_tag
                && document.selection.account == account.record.id().to_hex()
        })
        .map(|document| document.publication.clone())
        .unwrap_or_default()
}

fn configured_account(
    config: &RuntimeConfig,
    account_id: &str,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    let snapshot = account::snapshot(config)?;
    Ok(snapshot
        .accounts
        .into_iter()
        .find(|account| account.record.id().to_hex() == account_id))
}

fn summary_view(
    scope: FarmConfigScope,
    path: String,
    document: &FarmConfigDocument,
    account_pubkey: Option<&str>,
) -> FarmConfigSummaryView {
    FarmConfigSummaryView {
        scope: scope.as_str().to_owned(),
        path,
        seller_account_id: document.selection.account.clone(),
        seller_pubkey: account_pubkey.map(str::to_owned),
        seller_actor_source: FARM_SELLER_ACTOR_SOURCE.to_owned(),
        farm_d_tag: document.selection.farm_d_tag.clone(),
        name: resolved_name(document).unwrap_or_default(),
        location_primary: resolved_location_primary(document),
        delivery_method: resolved_delivery_method(document).unwrap_or_default(),
        publication: publication_view(&document.publication),
    }
}

fn document_view(document: &FarmConfigDocument) -> FarmConfigDocumentView {
    FarmConfigDocumentView {
        selection: FarmSelectionView {
            scope: document.selection.scope.as_str().to_owned(),
            seller_account_id: document.selection.account.clone(),
            farm_d_tag: document.selection.farm_d_tag.clone(),
        },
        profile: FarmProfileDraftView {
            name: document.profile.name.clone(),
            display_name: document.profile.display_name.clone(),
            nip05: document.profile.nip05.clone(),
            about: document.profile.about.clone(),
            website: document.profile.website.clone(),
            picture: document.profile.picture.clone(),
            banner: document.profile.banner.clone(),
            lud06: document.profile.lud06.clone(),
            lud16: document.profile.lud16.clone(),
            bot: document.profile.bot.clone(),
        },
        farm: document.farm.clone(),
        listing_defaults: FarmListingDefaultsView {
            delivery_method: document.listing_defaults.delivery_method.clone(),
            location: document.listing_defaults.location.clone(),
        },
        publication: publication_view(&document.publication),
    }
}

fn publication_view(publication: &FarmPublicationStatus) -> FarmPublicationView {
    FarmPublicationView {
        profile_state: publish_state(
            publication.profile_event_id.as_deref(),
            publication.profile_published_at,
        )
        .to_owned(),
        farm_state: publish_state(
            publication.farm_event_id.as_deref(),
            publication.farm_published_at,
        )
        .to_owned(),
        profile_event_id: publication.profile_event_id.clone(),
        farm_event_id: publication.farm_event_id.clone(),
        profile_published_at: publication.profile_published_at,
        farm_published_at: publication.farm_published_at,
    }
}

fn publish_state(event_id: Option<&str>, published_at: Option<u64>) -> &'static str {
    if event_id.is_some_and(|value| !value.trim().is_empty()) || published_at.is_some() {
        "published"
    } else {
        "not_published"
    }
}

fn scope_from_arg(scope: Option<FarmScopeArg>) -> Option<FarmConfigScope> {
    scope.map(|scope| match scope {
        FarmScopeArg::User => FarmConfigScope::User,
        FarmScopeArg::Workspace => FarmConfigScope::Workspace,
    })
}

fn required_d_tag(value: &str, field: &str) -> Result<String, RuntimeError> {
    let value = required_text(value, field)?;
    if !is_d_tag_base64url(value.as_str()) {
        return Err(RuntimeError::Config(format!(
            "{field} must be a 22-character base64url identifier"
        )));
    }
    Ok(value)
}

fn required_text(value: &str, field: &str) -> Result<String, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(format!("{field} must not be empty")));
    }
    Ok(trimmed.to_owned())
}

fn optional_arg_or_existing(arg: Option<&String>, existing: Option<&String>) -> Option<String> {
    arg.and_then(|value| non_empty(value.as_str()))
        .or_else(|| existing.and_then(|value| non_empty(value.as_str())))
}

fn draft_name_from_account(account: &AccountRecordView) -> Option<String> {
    account
        .record
        .label()
        .and_then(non_empty)
        .or_else(|| non_empty(account.record.id().to_hex().as_str()))
}

fn existing_name(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document.and_then(resolved_name)
}

fn existing_location_primary(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document.and_then(resolved_location_primary)
}

fn existing_city(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document
        .and_then(|document| {
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.city.as_ref())
        })
        .and_then(|value| non_empty(value.as_str()))
        .or_else(|| {
            existing_document
                .and_then(|document| document.listing_defaults.location.city.as_ref())
                .and_then(|value| non_empty(value.as_str()))
        })
}

fn existing_region(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document
        .and_then(|document| {
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.region.as_ref())
        })
        .and_then(|value| non_empty(value.as_str()))
        .or_else(|| {
            existing_document
                .and_then(|document| document.listing_defaults.location.region.as_ref())
                .and_then(|value| non_empty(value.as_str()))
        })
}

fn existing_country(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document
        .and_then(|document| {
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.country.as_ref())
        })
        .and_then(|value| non_empty(value.as_str()))
        .or_else(|| {
            existing_document
                .and_then(|document| document.listing_defaults.location.country.as_ref())
                .and_then(|value| non_empty(value.as_str()))
        })
}

fn existing_geohash(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document
        .and_then(|document| {
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| non_empty(location.geohash.as_str()))
        })
        .or_else(|| {
            existing_document
                .and_then(|document| non_empty(document.listing_defaults.location.geohash.as_str()))
        })
}

fn existing_delivery_method(existing_document: Option<&FarmConfigDocument>) -> Option<String> {
    existing_document
        .and_then(|document| non_empty(document.listing_defaults.delivery_method.as_str()))
}

fn resolved_name(document: &FarmConfigDocument) -> Option<String> {
    non_empty(document.profile.name.as_str()).or_else(|| non_empty(document.farm.name.as_str()))
}

fn resolved_location_primary(document: &FarmConfigDocument) -> Option<String> {
    non_empty(document.listing_defaults.location.primary.as_str()).or_else(|| {
        document
            .farm
            .location
            .as_ref()
            .and_then(|location| non_empty(location.primary.as_str()))
    })
}

fn resolved_delivery_method(document: &FarmConfigDocument) -> Option<String> {
    non_empty(document.listing_defaults.delivery_method.as_str())
}

fn display_delivery_method(value: &str) -> String {
    value
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(capitalize_ascii_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_ascii_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut rendered = String::new();
    rendered.push(first.to_ascii_uppercase());
    rendered.push_str(chars.as_str());
    rendered
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn generate_d_tag() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = D_TAG_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    encode_base64url_no_pad((nanos ^ counter).to_be_bytes())
}

fn encode_base64url_no_pad(bytes: [u8; 16]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(22);
    let mut index = 0usize;
    while index + 3 <= bytes.len() {
        let block = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
        output.push(ALPHABET[(block & 0x3f) as usize] as char);
        index += 3;
    }
    let remaining = bytes.len() - index;
    if remaining == 1 {
        let block = (bytes[index] as u32) << 16;
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
    } else if remaining == 2 {
        let block = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{authored_profile_from_draft, generate_d_tag, require_verified_publish_media};
    use crate::runtime::RuntimeError;
    use crate::runtime::farm_config::FarmProfileDraft;
    use radroots_event::farm::Farm;
    use radroots_event_codec::d_tag::is_d_tag_base64url;
    use radroots_event_codec::profile::authored::authored_profile_to_wire_parts;
    use serde_json::json;

    #[test]
    fn generated_farm_d_tag_is_valid_base64url() {
        assert!(is_d_tag_base64url(&generate_d_tag()));
    }

    #[test]
    fn farm_profile_draft_uses_strict_tagless_profile_authoring() {
        let draft = FarmProfileDraft {
            name: "moss street farm".to_owned(),
            display_name: Some("Moss Street Farm".to_owned()),
            nip05: Some("farm@example.com".to_owned()),
            about: Some("Victoria produce".to_owned()),
            website: Some("https://example.com".to_owned()),
            bot: Some("true".to_owned()),
            ..FarmProfileDraft::default()
        };

        let authored = authored_profile_from_draft(&draft).expect("strict authored Profile");
        let wire = authored_profile_to_wire_parts(&authored).expect("Profile wire parts");

        assert!(wire.tags.is_empty());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&wire.content).expect("Profile metadata"),
            json!({
                "name": "moss street farm",
                "display_name": "Moss Street Farm",
                "about": "Victoria produce",
                "nip05": "farm@example.com",
                "bot": true,
            })
        );
    }

    #[test]
    fn farm_profile_draft_rejects_non_boolean_legacy_bot_values() {
        let draft = FarmProfileDraft {
            name: "moss street farm".to_owned(),
            bot: Some("yes".to_owned()),
            ..FarmProfileDraft::default()
        };

        let RuntimeError::Config(message) =
            authored_profile_from_draft(&draft).expect_err("invalid bot must fail closed")
        else {
            panic!("expected config error");
        };
        assert_eq!(message, "farm profile bot must be `true` or `false`");
    }

    #[test]
    fn farm_publish_rejects_raw_media_without_blossom_proofs() {
        let mut draft = FarmProfileDraft {
            name: "moss street farm".to_owned(),
            picture: Some("https://media.example/picture.jpg".to_owned()),
            ..FarmProfileDraft::default()
        };
        let mut farm = sample_farm();

        assert_blossom_proof_error(require_verified_publish_media(&draft, &farm));

        draft.picture = None;
        farm.banner = Some("https://media.example/banner.jpg".to_owned());
        assert_blossom_proof_error(require_verified_publish_media(&draft, &farm));
    }

    fn sample_farm() -> Farm {
        Farm {
            d_tag: "AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            name: "Moss Street Farm".to_owned(),
            about: None,
            website: None,
            picture: None,
            banner: None,
            location: None,
            tags: None,
        }
    }

    fn assert_blossom_proof_error(result: Result<(), RuntimeError>) {
        let RuntimeError::Config(message) = result.expect_err("raw media must fail closed") else {
            panic!("expected config error");
        };
        assert!(message.contains("Blossom descriptor"));
        assert!(message.contains("BUD-02 upload completion"));
    }
}
