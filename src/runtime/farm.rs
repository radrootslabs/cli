use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_events::farm::{RadrootsFarm, RadrootsFarmLocation};
use radroots_events::kinds::{KIND_FARM, KIND_PROFILE};
use radroots_events::listing::RadrootsListingLocation;
use radroots_events::profile::{RadrootsProfile, RadrootsProfileType};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::farm::encode::to_wire_parts_with_kind;
use radroots_events_codec::profile::encode::to_wire_parts_with_profile_type;

use crate::cli::{
    FarmFieldArg, FarmInitArgs, FarmPublishArgs, FarmScopeArg, FarmScopedArgs, FarmSetArgs,
    FarmSetupArgs,
};
use crate::domain::runtime::{
    FarmConfigDocumentView, FarmConfigSummaryView, FarmGetView, FarmListingDefaultsView,
    FarmPublicationView, FarmPublishComponentView, FarmPublishEventView, FarmPublishJobView,
    FarmPublishView, FarmSelectionView, FarmSetView, FarmSetupView, FarmStatusView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts::{self, AccountRecordView};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::daemon::{self, BridgeEventPublishResult, DaemonRpcError};
use crate::runtime::farm_config::{
    self, FarmConfigDocument, FarmConfigScope, FarmConfigSelection, FarmListingDefaults,
    FarmMissingField, FarmPublicationStatus, ResolvedFarmConfig, SUPPORTED_FARM_CONFIG_VERSION,
};
use crate::runtime::signer::{ActorWriteBindingError, resolve_actor_write_authority};

const FARM_CONFIG_SOURCE: &str = "farm config · local first";

static D_TAG_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn init(config: &RuntimeConfig, args: &FarmInitArgs) -> Result<FarmSetupView, RuntimeError> {
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
        farm_setup_actions(&document),
        config,
    )
}

pub fn setup(config: &RuntimeConfig, args: &FarmSetupArgs) -> Result<FarmSetupView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let Some(selected_account) = selected_account_for_draft(config)? else {
        return Ok(missing_selected_account_setup_view());
    };
    let existing = farm_config::load(config, Some(resolved_scope))?;
    let document = setup_document(args, resolved_scope, &selected_account, existing.as_ref())?;
    save_draft_view(
        "configured",
        resolved_scope,
        &selected_account,
        &document,
        None,
        farm_setup_actions(&document),
        config,
    )
}

pub fn set(config: &RuntimeConfig, args: &FarmSetArgs) -> Result<FarmSetView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(mut resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmSetView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            field: human_field_name(args.field).to_owned(),
            value: human_field_value(args.field, args.value.join(" ").trim()).to_owned(),
            config: None,
            reason: Some(format!("no farm draft found at {}", path.display())),
            actions: vec!["radroots farm init".to_owned()],
        });
    };

    let raw_value = args.value.join(" ");
    let field_value = required_text(raw_value.as_str(), "farm set value")?;
    apply_field_update(&mut resolved.document, args.field, field_value.as_str())?;
    let written_path = farm_config::write(&config.paths, resolved.scope, &resolved.document)?;
    let configured_account = configured_account(config, &resolved.document.selection.account)?;
    let account_pubkey = configured_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.as_str());

    Ok(FarmSetView {
        state: "updated".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        field: human_field_name(args.field).to_owned(),
        value: human_field_value(args.field, field_value.as_str()).to_owned(),
        config: Some(summary_view(
            resolved.scope,
            written_path.display().to_string(),
            &resolved.document,
            account_pubkey,
        )),
        reason: None,
        actions: vec!["radroots farm check".to_owned()],
    })
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
            config: None,
            missing: vec!["Farm draft".to_owned()],
            reason: Some(format!("no farm config found at {}", path.display())),
            actions: vec!["radroots farm init".to_owned()],
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
    let state = if account.is_some() && draft_missing.is_empty() {
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
        None
    };
    let mut actions = Vec::new();
    if account.is_none() {
        actions.push("radroots account create".to_owned());
    } else if draft_missing.is_empty() {
        actions.push("radroots farm publish".to_owned());
    } else {
        actions.extend(missing_field_actions(draft_missing.as_slice()));
    }
    let account_pubkey = account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.as_str());

    Ok(FarmStatusView {
        state: state.to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        config_valid: true,
        account_state: account_state.to_owned(),
        listing_defaults_state: listing_defaults_state.to_owned(),
        config: Some(summary_view(
            resolved.scope,
            resolved.path.display().to_string(),
            &resolved.document,
            account_pubkey,
        )),
        missing: if account.is_none() {
            vec!["Selected account".to_owned()]
        } else {
            missing_field_labels(draft_missing.as_slice())
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
            actions: vec!["radroots farm init".to_owned()],
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

pub fn publish(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
) -> Result<FarmPublishView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(missing_publish_view(
            resolved_scope,
            path.display().to_string(),
            args,
            format!("no farm config found at {}", path.display()),
            vec!["Farm draft".to_owned()],
            vec!["radroots farm init".to_owned()],
            false,
            String::new(),
            String::new(),
            String::new(),
        ));
    };

    let Some(account) = configured_account(config, &resolved.document.selection.account)? else {
        return Ok(missing_publish_view(
            resolved.scope,
            resolved.path.display().to_string(),
            args,
            format!(
                "farm config account `{}` is not present in the local account store",
                resolved.document.selection.account
            ),
            vec!["Selected account".to_owned()],
            vec!["radroots account create".to_owned()],
            true,
            resolved.document.selection.account.clone(),
            String::new(),
            resolved.document.selection.farm_d_tag.clone(),
        ));
    };
    let draft_missing = farm_config::missing_fields(&resolved.document);
    if !draft_missing.is_empty() {
        return Ok(missing_publish_view(
            resolved.scope,
            resolved.path.display().to_string(),
            args,
            "farm draft is missing required fields".to_owned(),
            missing_field_labels(draft_missing.as_slice()),
            missing_field_actions(draft_missing.as_slice()),
            true,
            resolved.document.selection.account.clone(),
            account.record.public_identity.public_key_hex.clone(),
            resolved.document.selection.farm_d_tag.clone(),
        ));
    }
    let account_pubkey = account.record.public_identity.public_key_hex.clone();
    let previews = build_publish_previews(&resolved.document, account_pubkey.as_str())?;
    let profile_idempotency_key = component_idempotency_key(args, "profile")?;
    let farm_idempotency_key = component_idempotency_key(args, "farm")?;

    if config.output.dry_run {
        return Ok(base_publish_view(
            "dry_run",
            config,
            args,
            &resolved,
            &account_pubkey,
            preview_component(
                "bridge.profile.publish",
                KIND_PROFILE,
                profile_idempotency_key,
                args,
                Some(previews.profile),
            ),
            preview_component(
                "bridge.farm.publish",
                KIND_FARM,
                farm_idempotency_key,
                args,
                Some(previews.farm),
            ),
            Some("dry run requested; daemon farm publish skipped".to_owned()),
            vec![format!(
                "radroots farm publish --scope {}",
                resolved.scope.as_str()
            )],
        ));
    }

    let signer_authority =
        match resolve_actor_write_authority(config, "farm", account_pubkey.as_str()) {
            Ok(authority) => authority,
            Err(error) => {
                return Ok(binding_error_publish_view(
                    config,
                    args,
                    &resolved,
                    &account_pubkey,
                    previews,
                    profile_idempotency_key,
                    farm_idempotency_key,
                    error,
                ));
            }
        };
    let profile_signer_session_id = match daemon::resolve_signer_session_id(
        config,
        "farm profile",
        account_pubkey.as_str(),
        KIND_PROFILE,
        args.signer_session_id.as_deref(),
        signer_authority.as_ref(),
    ) {
        Ok(session_id) => session_id,
        Err(error) => {
            return Ok(daemon_error_publish_view(
                config,
                args,
                &resolved,
                &account_pubkey,
                previews,
                profile_idempotency_key,
                farm_idempotency_key,
                error,
                FarmPublishFailureStage::Profile,
            ));
        }
    };
    let farm_signer_session_id = match daemon::resolve_signer_session_id(
        config,
        "farm",
        account_pubkey.as_str(),
        KIND_FARM,
        Some(profile_signer_session_id.as_str()),
        signer_authority.as_ref(),
    ) {
        Ok(session_id) => session_id,
        Err(error) => {
            return Ok(daemon_error_publish_view(
                config,
                args,
                &resolved,
                &account_pubkey,
                previews,
                profile_idempotency_key,
                farm_idempotency_key,
                error,
                FarmPublishFailureStage::Farm,
            ));
        }
    };

    let profile_result = match daemon::bridge_profile_publish(
        config,
        &resolved.document.profile,
        Some(RadrootsProfileType::Farm),
        profile_idempotency_key.as_deref(),
        Some(profile_signer_session_id.as_str()),
        signer_authority.as_ref(),
    ) {
        Ok(result) => result_component(
            "bridge.profile.publish",
            KIND_PROFILE,
            result,
            args,
            Some(previews.profile.clone()),
        ),
        Err(error) => {
            return Ok(daemon_error_publish_view(
                config,
                args,
                &resolved,
                &account_pubkey,
                previews,
                profile_idempotency_key,
                farm_idempotency_key,
                error,
                FarmPublishFailureStage::Profile,
            ));
        }
    };

    if component_failed(&profile_result) {
        return Ok(persist_publication_and_view(
            config,
            args,
            resolved,
            &account_pubkey,
            profile_result,
            preview_component(
                "bridge.farm.publish",
                KIND_FARM,
                farm_idempotency_key,
                args,
                Some(previews.farm),
            ),
        )?);
    }

    let farm_result = match daemon::bridge_farm_publish(
        config,
        &resolved.document.farm,
        farm_idempotency_key.as_deref(),
        Some(farm_signer_session_id.as_str()),
        signer_authority.as_ref(),
    ) {
        Ok(result) => result_component(
            "bridge.farm.publish",
            KIND_FARM,
            result,
            args,
            Some(previews.farm),
        ),
        Err(error) => {
            let profile_for_error = profile_result.clone();
            let farm_for_error = daemon_error_component(
                "bridge.farm.publish",
                KIND_FARM,
                args,
                farm_idempotency_key,
                error,
                Some(previews.farm),
            );
            return Ok(persist_publication_and_view(
                config,
                args,
                resolved,
                &account_pubkey,
                profile_for_error,
                farm_for_error,
            )?);
        }
    };

    persist_publication_and_view(
        config,
        args,
        resolved,
        &account_pubkey,
        profile_result,
        farm_result,
    )
}

#[derive(Debug, Clone)]
struct FarmPublishPreviews {
    profile: FarmPublishEventView,
    farm: FarmPublishEventView,
}

#[derive(Debug, Clone, Copy)]
enum FarmPublishFailureStage {
    Profile,
    Farm,
}

fn missing_publish_view(
    scope: FarmConfigScope,
    path: String,
    args: &FarmPublishArgs,
    reason: String,
    missing: Vec<String>,
    actions: Vec<String>,
    config_present: bool,
    selected_account_id: String,
    selected_account_pubkey: String,
    farm_d_tag: String,
) -> FarmPublishView {
    FarmPublishView {
        state: "unconfigured".to_owned(),
        source: daemon::bridge_source().to_owned(),
        scope: scope.as_str().to_owned(),
        path,
        config_present,
        dry_run: false,
        selected_account_id,
        selected_account_pubkey,
        farm_d_tag,
        requested_signer_session_id: args.signer_session_id.clone(),
        profile: not_submitted_component("bridge.profile.publish", KIND_PROFILE, args, None, None),
        farm: not_submitted_component("bridge.farm.publish", KIND_FARM, args, None, None),
        missing,
        reason: Some(reason),
        actions,
    }
}

fn base_publish_view(
    state: &str,
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    profile: FarmPublishComponentView,
    farm: FarmPublishComponentView,
    reason: Option<String>,
    actions: Vec<String>,
) -> FarmPublishView {
    FarmPublishView {
        state: state.to_owned(),
        source: daemon::bridge_source().to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        dry_run: config.output.dry_run,
        selected_account_id: resolved.document.selection.account.clone(),
        selected_account_pubkey: account_pubkey.to_owned(),
        farm_d_tag: resolved.document.selection.farm_d_tag.clone(),
        requested_signer_session_id: args.signer_session_id.clone(),
        profile,
        farm,
        missing: Vec::new(),
        reason,
        actions,
    }
}

fn build_publish_previews(
    document: &FarmConfigDocument,
    account_pubkey: &str,
) -> Result<FarmPublishPreviews, RuntimeError> {
    let profile_parts =
        to_wire_parts_with_profile_type(&document.profile, Some(RadrootsProfileType::Farm))
            .map_err(|error| RuntimeError::Config(format!("invalid farm profile: {error}")))?;
    let farm_parts = to_wire_parts_with_kind(&document.farm, KIND_FARM)
        .map_err(|error| RuntimeError::Config(format!("invalid farm contract: {error}")))?;
    let farm_addr = format!(
        "{}:{}:{}",
        farm_parts.kind, account_pubkey, document.farm.d_tag
    );

    Ok(FarmPublishPreviews {
        profile: FarmPublishEventView {
            kind: profile_parts.kind,
            author: account_pubkey.to_owned(),
            content: profile_parts.content,
            tags: profile_parts.tags,
            event_id: None,
            event_addr: None,
        },
        farm: FarmPublishEventView {
            kind: farm_parts.kind,
            author: account_pubkey.to_owned(),
            content: farm_parts.content,
            tags: farm_parts.tags,
            event_id: None,
            event_addr: Some(farm_addr),
        },
    })
}

fn component_idempotency_key(
    args: &FarmPublishArgs,
    component: &str,
) -> Result<Option<String>, RuntimeError> {
    args.idempotency_key
        .as_deref()
        .map(|value| {
            required_text(value, "idempotency_key").map(|key| format!("{key}:{component}"))
        })
        .transpose()
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
        job_id: None,
        job_status: None,
        signer_mode: None,
        signer_session_id: None,
        event_id: None,
        event_addr: event.as_ref().and_then(|event| event.event_addr.clone()),
        idempotency_key: idempotency_key.clone(),
        reason: Some("not submitted".to_owned()),
        job: args.print_job.then(|| FarmPublishJobView {
            rpc_method: rpc_method.to_owned(),
            state: "not_submitted".to_owned(),
            job_id: None,
            idempotency_key,
            requested_signer_session_id: args.signer_session_id.clone(),
            signer_mode: None,
            signer_session_id: None,
        }),
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

fn result_component(
    rpc_method: &str,
    fallback_event_kind: u32,
    result: BridgeEventPublishResult,
    args: &FarmPublishArgs,
    preview: Option<FarmPublishEventView>,
) -> FarmPublishComponentView {
    let state = if result.status == "failed" {
        "failed".to_owned()
    } else if result.deduplicated {
        "deduplicated".to_owned()
    } else {
        result.status.clone()
    };
    let event_kind = result.event_kind.unwrap_or(fallback_event_kind);
    let event_addr = result
        .event_addr
        .clone()
        .or_else(|| preview.as_ref().and_then(|event| event.event_addr.clone()));
    let event = args.print_event.then(|| FarmPublishEventView {
        event_id: result.event_id.clone(),
        event_addr: event_addr.clone(),
        ..preview.unwrap_or_else(|| FarmPublishEventView {
            kind: event_kind,
            author: String::new(),
            content: String::new(),
            tags: Vec::new(),
            event_id: None,
            event_addr: None,
        })
    });
    FarmPublishComponentView {
        state: state.clone(),
        rpc_method: rpc_method.to_owned(),
        event_kind,
        deduplicated: result.deduplicated,
        job_id: Some(result.job_id.clone()),
        job_status: Some(result.status.clone()),
        signer_mode: Some(result.signer_mode.clone()),
        signer_session_id: result.signer_session_id.clone(),
        event_id: result.event_id.clone(),
        event_addr,
        idempotency_key: result.idempotency_key.clone(),
        reason: (result.status == "failed")
            .then(|| "daemon publish job failed before relay delivery completed".to_owned()),
        job: args.print_job.then(|| FarmPublishJobView {
            rpc_method: rpc_method.to_owned(),
            state,
            job_id: Some(result.job_id),
            idempotency_key: result.idempotency_key,
            requested_signer_session_id: args.signer_session_id.clone(),
            signer_mode: Some(result.signer_mode),
            signer_session_id: result.signer_session_id,
        }),
        event,
    }
}

fn daemon_error_component(
    rpc_method: &str,
    event_kind: u32,
    args: &FarmPublishArgs,
    idempotency_key: Option<String>,
    error: DaemonRpcError,
    preview: Option<FarmPublishEventView>,
) -> FarmPublishComponentView {
    let (state, reason) = daemon_error_state_reason(error);
    FarmPublishComponentView {
        state: state.clone(),
        rpc_method: rpc_method.to_owned(),
        event_kind,
        deduplicated: false,
        job_id: None,
        job_status: None,
        signer_mode: None,
        signer_session_id: None,
        event_id: None,
        event_addr: preview.as_ref().and_then(|event| event.event_addr.clone()),
        idempotency_key: idempotency_key.clone(),
        reason: Some(reason),
        job: args.print_job.then(|| FarmPublishJobView {
            rpc_method: rpc_method.to_owned(),
            state,
            job_id: None,
            idempotency_key,
            requested_signer_session_id: args.signer_session_id.clone(),
            signer_mode: None,
            signer_session_id: None,
        }),
        event: args.print_event.then_some(preview).flatten(),
    }
}

fn daemon_error_state_reason(error: DaemonRpcError) -> (String, String) {
    match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => ("unconfigured".to_owned(), reason),
        DaemonRpcError::External(reason) => ("unavailable".to_owned(), reason),
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => ("error".to_owned(), reason),
    }
}

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
    let (state, reason, actions) = match error {
        ActorWriteBindingError::Unconfigured(reason) => (
            "unconfigured".to_owned(),
            reason,
            vec!["run radroots signer status get".to_owned()],
        ),
        ActorWriteBindingError::Unavailable(reason) => (
            "unavailable".to_owned(),
            reason,
            vec!["run radroots signer status get".to_owned()],
        ),
    };
    base_publish_view(
        state.as_str(),
        config,
        args,
        resolved,
        account_pubkey,
        FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..preview_component(
                "bridge.profile.publish",
                KIND_PROFILE,
                profile_idempotency_key,
                args,
                Some(previews.profile),
            )
        },
        FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..preview_component(
                "bridge.farm.publish",
                KIND_FARM,
                farm_idempotency_key,
                args,
                Some(previews.farm),
            )
        },
        Some(reason),
        actions,
    )
}

fn daemon_error_publish_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
    error: DaemonRpcError,
    stage: FarmPublishFailureStage,
) -> FarmPublishView {
    let (state, reason) = daemon_error_state_reason(error);
    let profile = match stage {
        FarmPublishFailureStage::Profile => FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..preview_component(
                "bridge.profile.publish",
                KIND_PROFILE,
                profile_idempotency_key,
                args,
                Some(previews.profile),
            )
        },
        FarmPublishFailureStage::Farm => preview_component(
            "bridge.profile.publish",
            KIND_PROFILE,
            profile_idempotency_key,
            args,
            Some(previews.profile),
        ),
    };
    let farm = match stage {
        FarmPublishFailureStage::Profile => preview_component(
            "bridge.farm.publish",
            KIND_FARM,
            farm_idempotency_key,
            args,
            Some(previews.farm),
        ),
        FarmPublishFailureStage::Farm => FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..preview_component(
                "bridge.farm.publish",
                KIND_FARM,
                farm_idempotency_key,
                args,
                Some(previews.farm),
            )
        },
    };
    base_publish_view(
        state.as_str(),
        config,
        args,
        resolved,
        account_pubkey,
        profile,
        farm,
        Some(reason),
        daemon_error_actions(state.as_str()),
    )
}

fn persist_publication_and_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    mut resolved: ResolvedFarmConfig,
    account_pubkey: &str,
    profile: FarmPublishComponentView,
    farm: FarmPublishComponentView,
) -> Result<FarmPublishView, RuntimeError> {
    let now = unix_timestamp_now();
    if component_published(&profile) {
        if let Some(event_id) = &profile.event_id {
            resolved.document.publication.profile_event_id = Some(event_id.clone());
        }
        resolved.document.publication.profile_published_at = Some(now);
    }
    if component_published(&farm) {
        if let Some(event_id) = &farm.event_id {
            resolved.document.publication.farm_event_id = Some(event_id.clone());
        }
        resolved.document.publication.farm_published_at = Some(now);
    }
    if component_published(&profile) || component_published(&farm) {
        farm_config::write(&config.paths, resolved.scope, &resolved.document)?;
    }

    let state = publish_view_state(&profile, &farm);
    let mut actions = Vec::new();
    if let Some(job_id) = &profile.job_id {
        actions.push(format!("radroots job get {job_id}"));
    }
    if let Some(job_id) = &farm.job_id {
        actions.push(format!("radroots job get {job_id}"));
        actions.push(format!("radroots job watch {job_id}"));
    }
    if actions.is_empty() {
        actions.push("radroots rpc status".to_owned());
    }
    let reason = (state == "partial" || state == "unavailable" || state == "error")
        .then(|| "farm publish did not complete for both profile and farm record".to_owned());

    Ok(base_publish_view(
        state,
        config,
        args,
        &resolved,
        account_pubkey,
        profile,
        farm,
        reason,
        actions,
    ))
}

fn publish_view_state(
    profile: &FarmPublishComponentView,
    farm: &FarmPublishComponentView,
) -> &'static str {
    if component_error(profile) || component_error(farm) {
        return "error";
    }
    if component_unconfigured(profile) || component_unconfigured(farm) {
        return "unconfigured";
    }
    if component_failed(profile) || component_failed(farm) {
        return if component_published(profile) || component_published(farm) {
            "partial"
        } else {
            "unavailable"
        };
    }
    if profile.state == "deduplicated" || farm.state == "deduplicated" {
        return "deduplicated";
    }
    "published"
}

fn component_published(component: &FarmPublishComponentView) -> bool {
    matches!(component.state.as_str(), "published" | "deduplicated")
        || component
            .job_status
            .as_deref()
            .is_some_and(|status| status == "published")
}

fn component_failed(component: &FarmPublishComponentView) -> bool {
    matches!(component.state.as_str(), "failed" | "unavailable")
}

fn component_unconfigured(component: &FarmPublishComponentView) -> bool {
    component.state == "unconfigured"
}

fn component_error(component: &FarmPublishComponentView) -> bool {
    component.state == "error"
}

fn daemon_error_actions(state: &str) -> Vec<String> {
    match state {
        "unconfigured" => vec![
            "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
            "start radrootsd with bridge ingress enabled".to_owned(),
        ],
        "unavailable" => vec!["start radrootsd and verify the rpc url".to_owned()],
        _ => vec!["inspect the daemon rpc response contract".to_owned()],
    }
}

fn selected_account_for_draft(
    config: &RuntimeConfig,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    accounts::resolve_account(config)
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

fn init_document(
    scope: FarmConfigScope,
    account: &AccountRecordView,
    existing: Option<&ResolvedFarmConfig>,
    args: &FarmInitArgs,
) -> Result<FarmConfigDocument, RuntimeError> {
    let existing_document = existing.map(|resolved| &resolved.document);
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
    let delivery_method =
        optional_arg_or_existing(args.delivery_method.as_ref(), existing_delivery.as_ref())
            .unwrap_or_default();
    let publication = publication_for_document(existing_document, account, farm_d_tag.as_str());

    Ok(FarmConfigDocument {
        version: SUPPORTED_FARM_CONFIG_VERSION,
        selection: FarmConfigSelection {
            scope,
            account: account.record.account_id.to_string(),
            farm_d_tag: farm_d_tag.clone(),
        },
        profile: RadrootsProfile {
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
        farm: RadrootsFarm {
            d_tag: farm_d_tag,
            name,
            about,
            website,
            picture,
            banner,
            location: Some(RadrootsFarmLocation {
                primary: non_empty(location_primary.as_str()),
                city: city.clone(),
                region: region.clone(),
                country: country.clone(),
                gcs: None,
            }),
            tags: None,
        },
        listing_defaults: FarmListingDefaults {
            delivery_method,
            location: RadrootsListingLocation {
                primary: location_primary,
                city,
                region,
                country,
                lat: None,
                lng: None,
                geohash: None,
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
    Ok(FarmSetupView {
        state: state.to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        config: Some(summary_view(
            scope,
            written_path.display().to_string(),
            document,
            Some(account.record.public_identity.public_key_hex.as_str()),
        )),
        reason,
        actions,
    })
}

fn farm_setup_actions(document: &FarmConfigDocument) -> Vec<String> {
    let mut actions = vec!["radroots farm check".to_owned()];
    if farm_config::missing_fields(document).is_empty() {
        actions.push("radroots farm publish".to_owned());
    }
    actions
}

fn missing_blocks_listing_defaults(missing: &[FarmMissingField]) -> bool {
    missing.iter().any(|field| {
        matches!(
            field,
            FarmMissingField::Location | FarmMissingField::Delivery
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
            FarmMissingField::Delivery => {
                push_action(&mut actions, "radroots farm set delivery pickup");
            }
            FarmMissingField::Country => {
                push_action(&mut actions, "radroots farm set country US");
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

fn human_field_name(field: FarmFieldArg) -> &'static str {
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
        FarmFieldArg::Delivery => "Delivery",
    }
}

fn human_field_value(field: FarmFieldArg, value: &str) -> String {
    match field {
        FarmFieldArg::Delivery => humanize_delivery_method(value),
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
            ensure_farm_location(document).primary = Some(value);
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
        FarmFieldArg::Delivery => {
            document.listing_defaults.delivery_method = value;
        }
    }
    Ok(())
}

fn ensure_farm_location(document: &mut FarmConfigDocument) -> &mut RadrootsFarmLocation {
    let primary = non_empty(document.listing_defaults.location.primary.as_str());
    let city = document.listing_defaults.location.city.clone();
    let region = document.listing_defaults.location.region.clone();
    let country = document.listing_defaults.location.country.clone();
    document
        .farm
        .location
        .get_or_insert_with(|| RadrootsFarmLocation {
            primary,
            city,
            region,
            country,
            gcs: None,
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
                && document.selection.account == account.record.account_id.as_str()
        })
        .map(|document| document.publication.clone())
        .unwrap_or_default()
}

fn setup_document(
    args: &FarmSetupArgs,
    scope: FarmConfigScope,
    account: &AccountRecordView,
    existing: Option<&ResolvedFarmConfig>,
) -> Result<FarmConfigDocument, RuntimeError> {
    let existing_document = existing.map(|resolved| &resolved.document);
    let name = required_text(args.name.as_str(), "farm.name")?;
    let location_primary = required_text(args.location.as_str(), "farm.location.primary")?;
    let delivery_method = required_text(
        args.delivery_method.as_str(),
        "listing_defaults.delivery_method",
    )?;
    let farm_d_tag = match args.farm_d_tag.as_deref() {
        Some(value) => required_d_tag(value, "farm_d_tag")?,
        None => existing_document
            .map(|document| document.farm.d_tag.clone())
            .unwrap_or_else(generate_d_tag),
    };
    if !is_d_tag_base64url(farm_d_tag.as_str()) {
        return Err(RuntimeError::Config(
            "farm_d_tag must be a 22-character base64url identifier".to_owned(),
        ));
    }

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
    let display_name = optional_arg_or_existing(
        args.display_name.as_ref(),
        existing_document.and_then(|document| document.profile.display_name.as_ref()),
    )
    .or_else(|| Some(name.clone()));
    let city = optional_arg_or_existing(
        args.city.as_ref(),
        existing_document
            .and_then(|document| document.farm.location.as_ref())
            .and_then(|location| location.city.as_ref()),
    );
    let region = optional_arg_or_existing(
        args.region.as_ref(),
        existing_document
            .and_then(|document| document.farm.location.as_ref())
            .and_then(|location| location.region.as_ref()),
    );
    let country = optional_arg_or_existing(
        args.country.as_ref(),
        existing_document
            .and_then(|document| document.farm.location.as_ref())
            .and_then(|location| location.country.as_ref()),
    );
    let publication = publication_for_document(existing_document, account, farm_d_tag.as_str());

    Ok(FarmConfigDocument {
        version: SUPPORTED_FARM_CONFIG_VERSION,
        selection: FarmConfigSelection {
            scope,
            account: account.record.account_id.to_string(),
            farm_d_tag: farm_d_tag.clone(),
        },
        profile: RadrootsProfile {
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
        farm: RadrootsFarm {
            d_tag: farm_d_tag,
            name,
            about,
            website,
            picture,
            banner,
            location: Some(RadrootsFarmLocation {
                primary: Some(location_primary.clone()),
                city: city.clone(),
                region: region.clone(),
                country: country.clone(),
                gcs: None,
            }),
            tags: None,
        },
        listing_defaults: FarmListingDefaults {
            delivery_method,
            location: RadrootsListingLocation {
                primary: location_primary,
                city,
                region,
                country,
                lat: None,
                lng: None,
                geohash: None,
            },
        },
        publication,
    })
}

fn configured_account(
    config: &RuntimeConfig,
    account_id: &str,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    let snapshot = accounts::snapshot(config)?;
    Ok(snapshot
        .accounts
        .into_iter()
        .find(|account| account.record.account_id.as_str() == account_id))
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
        selected_account_id: document.selection.account.clone(),
        selected_account_pubkey: account_pubkey.map(str::to_owned),
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
            account: document.selection.account.clone(),
            farm_d_tag: document.selection.farm_d_tag.clone(),
        },
        profile: document.profile.clone(),
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
        .label
        .as_deref()
        .and_then(non_empty)
        .or_else(|| non_empty(account.record.account_id.as_str()))
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
            .and_then(|location| location.primary.as_deref())
            .and_then(non_empty)
    })
}

fn resolved_delivery_method(document: &FarmConfigDocument) -> Option<String> {
    non_empty(document.listing_defaults.delivery_method.as_str())
}

fn humanize_delivery_method(value: &str) -> String {
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

fn unix_timestamp_now() -> u64 {
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
    use super::generate_d_tag;
    use radroots_events_codec::d_tag::is_d_tag_base64url;

    #[test]
    fn generated_farm_d_tag_is_valid_base64url() {
        assert!(is_d_tag_base64url(&generate_d_tag()));
    }
}
