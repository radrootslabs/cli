use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_events::farm::{RadrootsFarm, RadrootsFarmLocation};
use radroots_events::kinds::{KIND_FARM, KIND_PROFILE};
use radroots_events::listing::RadrootsListingLocation;
use radroots_events::profile::{RadrootsProfile, RadrootsProfileType};
use radroots_events_codec::d_tag::is_d_tag_base64url;
use radroots_events_codec::farm::encode::to_wire_parts_with_kind;
use radroots_events_codec::profile::encode::to_wire_parts_with_profile_type;
use radroots_events_codec::wire::WireEventParts;
use radroots_nostr::prelude::radroots_event_from_nostr;
use radroots_replica_db::migrations;
use radroots_replica_sync::{RadrootsReplicaIngestOutcome, radroots_replica_ingest_event};
use radroots_sql_core::SqliteExecutor;
use serde_json::json;

use crate::cli::global::{
    FarmCreateArgs, FarmFieldArg, FarmPublishArgs, FarmRebindArgs, FarmScopeArg, FarmScopedArgs,
    FarmUpdateArgs,
};
use crate::runtime::RuntimeError;
use crate::runtime::account::{self, AccountRecordView};
use crate::runtime::config::{
    PublishMode, RADROOTSD_PUBLISH_DEFERRED_REASON, RuntimeConfig, SignerBackend,
};
use crate::runtime::direct_relay::{
    DirectRelayFailure, DirectRelayPublishError, DirectRelayPublishReceipt,
    publish_signed_event_with_identity, sign_parts_with_identity,
};
use crate::runtime::farm_config::{
    self, FarmConfigDocument, FarmConfigScope, FarmConfigSelection, FarmListingDefaults,
    FarmMissingField, FarmPublicationStatus, ResolvedFarmConfig, SUPPORTED_FARM_CONFIG_VERSION,
};
use crate::runtime::local_events::{
    append_local_work, append_signed_event, mark_signed_event_acknowledged,
    mark_signed_event_failed_for_publish_error,
};
use crate::runtime::signer::{ActorWriteBindingError, resolve_actor_write_authority};
use crate::view::runtime::{
    FarmConfigDocumentView, FarmConfigSummaryView, FarmGetView, FarmListingDefaultsView,
    FarmPublicationView, FarmPublishComponentView, FarmPublishEventView,
    FarmPublishLocalReplicaView, FarmPublishView, FarmRebindView, FarmSelectionView, FarmSetView,
    FarmSetupView, FarmStatusView, RelayFailureView,
};

const FARM_CONFIG_SOURCE: &str = "farm config · local first";
const FARM_SELLER_ACTOR_SOURCE: &str = "farm_config";
const RELAY_FARM_WRITE_SOURCE: &str = "direct Nostr relay publish · local key";
const RADROOTSD_FARM_WRITE_SOURCE: &str = "radrootsd publish transport · deferred";
const RADROOTSD_BRIDGE_PROFILE_PUBLISH_METHOD: &str = "bridge.profile.publish";
const RADROOTSD_BRIDGE_FARM_PUBLISH_METHOD: &str = "bridge.farm.publish";

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
                    .public_identity
                    .public_key_hex
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
        .map(|account| account.record.public_identity.public_key_hex.clone());
    let target_account = account::resolve_account_selector(config, args.selector.as_str())
        .map_err(|error| farm_rebind_selector_error(args.selector.as_str(), error))?;
    let to_seller_pubkey = target_account.record.public_identity.public_key_hex.clone();
    let seller_pubkey_changed = from_seller_pubkey
        .as_deref()
        .is_none_or(|pubkey| !pubkey.eq_ignore_ascii_case(to_seller_pubkey.as_str()));
    let publication_state_action = if seller_pubkey_changed {
        "cleared"
    } else {
        "preserved"
    };
    let mut document = resolved.document.clone();
    document.selection.account = target_account.record.account_id.to_string();
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
        to_seller_account_id: Some(target_account.record.account_id.to_string()),
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
            vec![format!(
                "radroots --approval-token approve farm rebind {}",
                args.selector
            )]
        } else {
            vec!["radroots farm readiness check".to_owned()]
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
            field: human_field_name(args.field).to_owned(),
            value: human_field_value(args.field, args.value.join(" ").trim()).to_owned(),
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
    let account_pubkey = configured_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.as_str());
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
        field: human_field_name(args.field).to_owned(),
        value: human_field_value(args.field, field_value.as_str()).to_owned(),
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
            field: human_field_name(args.field).to_owned(),
            value: human_field_value(args.field, args.value.join(" ").trim()).to_owned(),
            config: None,
            reason: Some(format!("no farm draft found at {}", path.display())),
            actions: vec!["radroots farm create".to_owned()],
        });
    };

    let raw_value = args.value.join(" ");
    let field_value = required_text(raw_value.as_str(), "farm set value")?;
    apply_field_update(&mut resolved.document, args.field, field_value.as_str())?;
    let configured_account = configured_account(config, &resolved.document.selection.account)?;
    let account_pubkey = configured_account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.as_str());
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
        field: human_field_name(args.field).to_owned(),
        value: human_field_value(args.field, field_value.as_str()).to_owned(),
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
            publish_mode: config.publish.mode.as_str().to_owned(),
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
        actions.push("radroots account import <path>".to_owned());
        actions.push("radroots farm rebind <selector>".to_owned());
    } else if draft_missing.is_empty() {
        actions.extend(publish.actions.clone());
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
        publish_mode: config.publish.mode.as_str().to_owned(),
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
    match config.publish.mode {
        PublishMode::NostrRelay => relay_farm_publish_readiness(config, account),
        PublishMode::Radrootsd => radrootsd_farm_publish_readiness(config),
    }
}

fn relay_farm_publish_readiness(
    config: &RuntimeConfig,
    account: &AccountRecordView,
) -> FarmPublishReadiness {
    if config.relay.urls.is_empty() {
        return FarmPublishReadiness {
            state: "unconfigured",
            executable: false,
            reason: Some(
                "nostr_relay farm publish requires at least one configured relay".to_owned(),
            ),
            missing: vec!["Configured relay".to_owned()],
            actions: vec!["radroots --relay wss://relay.example.com farm publish".to_owned()],
        };
    }

    if matches!(config.signer.backend, SignerBackend::Myc) {
        return FarmPublishReadiness {
            state: "unavailable",
            executable: false,
            reason: Some(
                "nostr_relay farm publish requires signer mode `local`; signer mode `myc` is deferred"
                    .to_owned(),
            ),
            missing: vec!["Local signer mode".to_owned()],
            actions: vec!["radroots signer status get".to_owned()],
        };
    }

    if !account.write_capable {
        return FarmPublishReadiness {
            state: "unconfigured",
            executable: false,
            reason: Some(
                account::AccountRuntimeFailure::watch_only(&account.record.account_id).to_string(),
            ),
            missing: vec!["Write-capable farm-bound seller account".to_owned()],
            actions: vec![format!(
                "radroots account attach-secret {} <path>",
                account.record.account_id
            )],
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

fn radrootsd_farm_publish_readiness(_config: &RuntimeConfig) -> FarmPublishReadiness {
    FarmPublishReadiness {
        state: "unavailable",
        executable: false,
        reason: Some(RADROOTSD_PUBLISH_DEFERRED_REASON.to_owned()),
        missing: vec!["Active direct relay publish mode".to_owned()],
        actions: vec![
            "radroots --publish-mode nostr_relay --relay wss://relay.example.com farm publish"
                .to_owned(),
        ],
    }
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
            vec![
                "radroots account import <path>".to_owned(),
                "radroots farm rebind <selector>".to_owned(),
            ],
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
            account.record.public_identity.public_key_hex.clone(),
            resolved.document.selection.farm_d_tag.clone(),
        ));
    }
    let account_pubkey = account.record.public_identity.public_key_hex.clone();
    let previews = build_publish_previews(&resolved.document, account_pubkey.as_str())?;
    let profile_idempotency_key = component_idempotency_key(args, "profile")?;
    let farm_idempotency_key = component_idempotency_key(args, "farm")?;

    if config.output.dry_run {
        return dry_run_publish_view(
            config,
            args,
            &resolved,
            &account_pubkey,
            previews,
            profile_idempotency_key,
            farm_idempotency_key,
        );
    }

    match config.publish.mode {
        PublishMode::NostrRelay => publish_via_direct_relay(
            config,
            args,
            resolved,
            account_pubkey,
            previews,
            profile_idempotency_key,
            farm_idempotency_key,
        ),
        PublishMode::Radrootsd => Ok(radrootsd_preflight_publish_view(
            config,
            args,
            &resolved,
            &account_pubkey,
            previews,
            profile_idempotency_key,
            farm_idempotency_key,
            "unavailable",
            RADROOTSD_PUBLISH_DEFERRED_REASON,
        )),
    }
}

fn dry_run_publish_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
) -> Result<FarmPublishView, RuntimeError> {
    match config.publish.mode {
        PublishMode::NostrRelay => {
            if let Err(error) = resolve_farm_signing_identity(
                config,
                resolved.document.selection.account.as_str(),
                account_pubkey,
            ) {
                return match error {
                    ActorWriteBindingError::Account(failure) => Err(failure.into()),
                    error => Ok(binding_error_publish_view(
                        config,
                        args,
                        resolved,
                        account_pubkey,
                        previews,
                        profile_idempotency_key,
                        farm_idempotency_key,
                        error,
                    )),
                };
            }

            Ok(base_publish_view(
                "dry_run",
                config,
                args,
                resolved,
                account_pubkey,
                preview_component(
                    "relay.profile.publish",
                    KIND_PROFILE,
                    profile_idempotency_key,
                    args,
                    Some(previews.profile.event),
                ),
                preview_component(
                    "relay.farm.publish",
                    KIND_FARM,
                    farm_idempotency_key,
                    args,
                    Some(previews.farm.event),
                ),
                Some("dry run requested; relay publish skipped".to_owned()),
                vec!["radroots farm publish".to_owned()],
            ))
        }
        PublishMode::Radrootsd => Ok(radrootsd_preflight_publish_view(
            config,
            args,
            resolved,
            account_pubkey,
            previews,
            profile_idempotency_key,
            farm_idempotency_key,
            "unavailable",
            RADROOTSD_PUBLISH_DEFERRED_REASON,
        )),
    }
}

fn publish_via_direct_relay(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    mut resolved: ResolvedFarmConfig,
    account_pubkey: String,
    mut previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
) -> Result<FarmPublishView, RuntimeError> {
    let signing = match resolve_farm_signing_identity(
        config,
        resolved.document.selection.account.as_str(),
        account_pubkey.as_str(),
    ) {
        Ok(signing) => signing,
        Err(ActorWriteBindingError::Account(failure)) => return Err(failure.into()),
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

    if config.relay.urls.is_empty() {
        return Err(RuntimeError::Network(
            DirectRelayPublishError::MissingRelays.to_string(),
        ));
    }

    let profile_event = sign_parts_with_identity(&signing.identity, previews.profile.parts.clone())
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    previews.profile.event.event_id = Some(profile_event.id.to_hex());
    let profile_record = append_signed_event(
        config,
        format!("farm_profile:{}", resolved.document.selection.farm_d_tag).as_str(),
        Some(resolved.document.selection.account.clone()),
        Some(account_pubkey.clone()),
        Some(resolved.document.selection.farm_d_tag.clone()),
        None,
        &profile_event,
    )?;
    let profile_receipt = match publish_signed_event_with_identity(
        &signing.identity,
        &config.relay.urls,
        profile_event,
    ) {
        Ok(receipt) => {
            mark_signed_event_acknowledged(
                config,
                profile_record.record_id.as_str(),
                receipt.target_relays.clone(),
                receipt.connected_relays.clone(),
                receipt.acknowledged_relays.clone(),
                receipt.failed_relays.clone(),
            )?;
            receipt
        }
        Err(error) => {
            mark_signed_event_failed_for_publish_error(
                config,
                profile_record.record_id.as_str(),
                &error,
            )?;
            return Err(RuntimeError::Network(error.to_string()));
        }
    };
    let profile_local_replica =
        farm_local_replica_ingest_view(config, "profile", &profile_receipt, None);
    persist_profile_publication(config, &mut resolved, profile_receipt.event_id.clone())?;

    let farm_event = sign_parts_with_identity(&signing.identity, previews.farm.parts.clone())
        .map_err(|error| RuntimeError::Network(error.to_string()))?;
    previews.farm.event.event_id = Some(farm_event.id.to_hex());
    let farm_record = append_signed_event(
        config,
        format!("farm:{}", resolved.document.selection.farm_d_tag).as_str(),
        Some(resolved.document.selection.account.clone()),
        Some(account_pubkey.clone()),
        Some(resolved.document.selection.farm_d_tag.clone()),
        None,
        &farm_event,
    )?;
    let farm_receipt =
        match publish_signed_event_with_identity(&signing.identity, &config.relay.urls, farm_event)
        {
            Ok(receipt) => {
                mark_signed_event_acknowledged(
                    config,
                    farm_record.record_id.as_str(),
                    receipt.target_relays.clone(),
                    receipt.connected_relays.clone(),
                    receipt.acknowledged_relays.clone(),
                    receipt.failed_relays.clone(),
                )?;
                receipt
            }
            Err(error) => {
                mark_signed_event_failed_for_publish_error(
                    config,
                    farm_record.record_id.as_str(),
                    &error,
                )?;
                return Ok(partial_publish_view(
                    config,
                    args,
                    &resolved,
                    &account_pubkey,
                    previews,
                    profile_idempotency_key,
                    farm_idempotency_key,
                    profile_receipt,
                    profile_local_replica,
                    error,
                ));
            }
        };
    let farm_local_replica = farm_local_replica_ingest_view(
        config,
        "farm",
        &farm_receipt,
        previews.farm.event.event_addr.clone(),
    );
    persist_farm_publication(config, &mut resolved, farm_receipt.event_id.clone())?;

    let mut view = base_publish_view(
        "published",
        config,
        args,
        &resolved,
        &account_pubkey,
        published_component(
            "relay.profile.publish",
            KIND_PROFILE,
            profile_idempotency_key,
            args,
            previews.profile.event,
            profile_receipt,
        ),
        published_component(
            "relay.farm.publish",
            KIND_FARM,
            farm_idempotency_key,
            args,
            previews.farm.event,
            farm_receipt,
        ),
        None,
        Vec::new(),
    );
    view.local_replica = vec![profile_local_replica, farm_local_replica];
    Ok(view)
}

#[derive(Debug, Clone)]
struct FarmPublishPreviews {
    profile: FarmPublishEventDraft,
    farm: FarmPublishEventDraft,
}

#[derive(Debug, Clone)]
struct FarmPublishEventDraft {
    event: FarmPublishEventView,
    parts: WireEventParts,
}

impl FarmPublishView {
    fn with_requested_signer_session_id(mut self, signer_session_id: Option<String>) -> Self {
        self.requested_signer_session_id = signer_session_id;
        self
    }
}

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
        requested_signer_session_id: args.signer_session_id.clone(),
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

fn resolve_farm_signing_identity(
    config: &RuntimeConfig,
    account_id: &str,
    account_pubkey: &str,
) -> Result<account::AccountSigningIdentity, ActorWriteBindingError> {
    if !matches!(
        config.signer.backend,
        crate::runtime::config::SignerBackend::Local
    ) {
        return resolve_actor_write_authority(config, "farm", account_pubkey).and_then(|_| {
            Err(ActorWriteBindingError::Unconfigured(
                "farm publish requires signer mode `local`".to_owned(),
            ))
        });
    }
    let signing = account::resolve_local_signing_identity_for_account(config, account_id)
        .map_err(ActorWriteBindingError::from_runtime)?;
    let selected_pubkey = signing
        .account
        .record
        .public_identity
        .public_key_hex
        .as_str();
    if !selected_pubkey.eq_ignore_ascii_case(account_pubkey) {
        return Err(ActorWriteBindingError::Account(
            account::AccountRuntimeFailure::mismatch(format!(
                "account mismatch: resolved account pubkey `{selected_pubkey}` cannot sign farm-bound seller pubkey `{account_pubkey}`"
            )),
        ));
    }
    Ok(signing)
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
        source: farm_write_source(config).to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        dry_run: config.output.dry_run,
        seller_account_id: resolved.document.selection.account.clone(),
        seller_pubkey: account_pubkey.to_owned(),
        seller_actor_source: FARM_SELLER_ACTOR_SOURCE.to_owned(),
        farm_d_tag: resolved.document.selection.farm_d_tag.clone(),
        requested_signer_session_id: args.signer_session_id.clone(),
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
        profile: FarmPublishEventDraft {
            event: FarmPublishEventView {
                kind: profile_parts.kind,
                author: account_pubkey.to_owned(),
                content: profile_parts.content.clone(),
                tags: profile_parts.tags.clone(),
                event_id: None,
                event_addr: None,
            },
            parts: profile_parts,
        },
        farm: FarmPublishEventDraft {
            event: FarmPublishEventView {
                kind: farm_parts.kind,
                author: account_pubkey.to_owned(),
                content: farm_parts.content.clone(),
                tags: farm_parts.tags.clone(),
                event_id: None,
                event_addr: Some(farm_addr),
            },
            parts: farm_parts,
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
        target_relays: Vec::new(),
        connected_relays: Vec::new(),
        acknowledged_relays: Vec::new(),
        failed_relays: Vec::new(),
        job_id: None,
        job_status: None,
        signer_mode: None,
        signer_session_id: None,
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
    let actions = vec!["run radroots signer status get".to_owned()];
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
                "relay.profile.publish",
                KIND_PROFILE,
                profile_idempotency_key,
                args,
                Some(previews.profile.event),
            )
        },
        FarmPublishComponentView {
            state: state.clone(),
            reason: Some(reason.clone()),
            ..preview_component(
                "relay.farm.publish",
                KIND_FARM,
                farm_idempotency_key,
                args,
                Some(previews.farm.event),
            )
        },
        Some(reason),
        actions,
    )
}

fn partial_publish_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
    profile_receipt: DirectRelayPublishReceipt,
    profile_local_replica: FarmPublishLocalReplicaView,
    farm_error: DirectRelayPublishError,
) -> FarmPublishView {
    let reason = format!("farm publish failed after profile publish: {farm_error}");
    let mut view = base_publish_view(
        "partial",
        config,
        args,
        resolved,
        account_pubkey,
        published_component(
            "relay.profile.publish",
            KIND_PROFILE,
            profile_idempotency_key,
            args,
            previews.profile.event,
            profile_receipt,
        ),
        failed_component(
            "relay.farm.publish",
            KIND_FARM,
            farm_idempotency_key,
            args,
            previews.farm.event,
            &config.relay.urls,
            farm_error,
        ),
        Some(reason),
        vec!["radroots farm publish".to_owned()],
    );
    view.local_replica = vec![profile_local_replica];
    view
}

fn published_component(
    rpc_method: &str,
    event_kind: u32,
    idempotency_key: Option<String>,
    args: &FarmPublishArgs,
    mut event: FarmPublishEventView,
    receipt: DirectRelayPublishReceipt,
) -> FarmPublishComponentView {
    event.event_id = Some(receipt.event_id.clone());
    FarmPublishComponentView {
        state: "published".to_owned(),
        rpc_method: rpc_method.to_owned(),
        event_kind,
        deduplicated: false,
        target_relays: receipt.target_relays,
        connected_relays: receipt.connected_relays,
        acknowledged_relays: receipt.acknowledged_relays,
        failed_relays: relay_failures(receipt.failed_relays),
        job_id: None,
        job_status: None,
        signer_mode: Some("local".to_owned()),
        signer_session_id: None,
        event_id: Some(receipt.event_id),
        event_addr: event.event_addr.clone(),
        idempotency_key,
        reason: None,
        job: None,
        event: args.print_event.then_some(event),
    }
}

fn farm_local_replica_ingest_view(
    config: &RuntimeConfig,
    component: &str,
    receipt: &DirectRelayPublishReceipt,
    event_addr: Option<String>,
) -> FarmPublishLocalReplicaView {
    if !config.local.replica_db_path.exists() {
        return FarmPublishLocalReplicaView {
            component: component.to_owned(),
            state: "unconfigured".to_owned(),
            store_state: "missing".to_owned(),
            ingest_outcome: None,
            event_id: Some(receipt.event_id.clone()),
            event_addr,
            reason: Some("local replica database is not initialized".to_owned()),
            actions: vec!["radroots store init".to_owned()],
        };
    }

    let executor = match SqliteExecutor::open(&config.local.replica_db_path) {
        Ok(executor) => executor,
        Err(error) => {
            return farm_local_replica_failed_view(
                component,
                receipt.event_id.clone(),
                event_addr,
                format!("failed to open local replica database: {error}"),
            );
        }
    };
    if let Err(error) = migrations::run_all_up(&executor) {
        return farm_local_replica_failed_view(
            component,
            receipt.event_id.clone(),
            event_addr,
            format!("failed to migrate local replica database: {error}"),
        );
    }

    let event = radroots_event_from_nostr(&receipt.event);
    match radroots_replica_ingest_event(&executor, &event) {
        Ok(RadrootsReplicaIngestOutcome::Applied) => FarmPublishLocalReplicaView {
            component: component.to_owned(),
            state: "applied".to_owned(),
            store_state: "ready".to_owned(),
            ingest_outcome: Some("applied".to_owned()),
            event_id: Some(receipt.event_id.clone()),
            event_addr,
            reason: None,
            actions: Vec::new(),
        },
        Ok(RadrootsReplicaIngestOutcome::Skipped) => FarmPublishLocalReplicaView {
            component: component.to_owned(),
            state: "skipped".to_owned(),
            store_state: "ready".to_owned(),
            ingest_outcome: Some("skipped".to_owned()),
            event_id: Some(receipt.event_id.clone()),
            event_addr,
            reason: Some("shared replica ingest skipped the event".to_owned()),
            actions: Vec::new(),
        },
        Err(error) => farm_local_replica_failed_view(
            component,
            receipt.event_id.clone(),
            event_addr,
            format!("failed to ingest farm publish event into local replica: {error}"),
        ),
    }
}

fn farm_local_replica_failed_view(
    component: &str,
    event_id: String,
    event_addr: Option<String>,
    reason: String,
) -> FarmPublishLocalReplicaView {
    FarmPublishLocalReplicaView {
        component: component.to_owned(),
        state: "failed".to_owned(),
        store_state: "unavailable".to_owned(),
        ingest_outcome: None,
        event_id: Some(event_id),
        event_addr,
        reason: Some(reason),
        actions: vec!["radroots store status get".to_owned()],
    }
}

fn failed_component(
    rpc_method: &str,
    event_kind: u32,
    idempotency_key: Option<String>,
    args: &FarmPublishArgs,
    event: FarmPublishEventView,
    relay_urls: &[String],
    error: DirectRelayPublishError,
) -> FarmPublishComponentView {
    let reason = error.to_string();
    let failure = publish_failure_details(&error, relay_urls);
    let event_id = failure.event_id.or_else(|| event.event_id.clone());
    FarmPublishComponentView {
        state: "failed".to_owned(),
        rpc_method: rpc_method.to_owned(),
        event_kind,
        deduplicated: false,
        target_relays: failure.target_relays,
        connected_relays: failure.connected_relays,
        acknowledged_relays: Vec::new(),
        failed_relays: failure.failed_relays,
        job_id: None,
        job_status: None,
        signer_mode: Some("local".to_owned()),
        signer_session_id: None,
        event_id,
        event_addr: event.event_addr.clone(),
        idempotency_key,
        reason: Some(reason),
        job: None,
        event: args.print_event.then_some(event),
    }
}

fn radrootsd_preflight_publish_view(
    config: &RuntimeConfig,
    args: &FarmPublishArgs,
    resolved: &ResolvedFarmConfig,
    account_pubkey: &str,
    previews: FarmPublishPreviews,
    profile_idempotency_key: Option<String>,
    farm_idempotency_key: Option<String>,
    state: &str,
    reason: &str,
) -> FarmPublishView {
    let requested_signer_session_id = args.signer_session_id.clone();
    base_publish_view(
        state,
        config,
        args,
        resolved,
        account_pubkey,
        FarmPublishComponentView {
            state: state.to_owned(),
            signer_mode: Some("deferred".to_owned()),
            signer_session_id: None,
            reason: Some(reason.to_owned()),
            ..radrootsd_preview_component(
                RADROOTSD_BRIDGE_PROFILE_PUBLISH_METHOD,
                KIND_PROFILE,
                profile_idempotency_key,
                args,
                Some(previews.profile.event),
            )
        },
        FarmPublishComponentView {
            state: state.to_owned(),
            signer_mode: Some("deferred".to_owned()),
            signer_session_id: None,
            reason: Some(reason.to_owned()),
            ..radrootsd_preview_component(
                RADROOTSD_BRIDGE_FARM_PUBLISH_METHOD,
                KIND_FARM,
                farm_idempotency_key,
                args,
                Some(previews.farm.event),
            )
        },
        Some(reason.to_owned()),
        vec![
            "radroots --publish-mode nostr_relay --relay wss://relay.example.com farm publish"
                .to_owned(),
        ],
    )
    .with_requested_signer_session_id(requested_signer_session_id)
}

fn radrootsd_preview_component(
    rpc_method: &str,
    event_kind: u32,
    idempotency_key: Option<String>,
    args: &FarmPublishArgs,
    event: Option<FarmPublishEventView>,
) -> FarmPublishComponentView {
    FarmPublishComponentView {
        signer_mode: Some("deferred".to_owned()),
        ..preview_component(rpc_method, event_kind, idempotency_key, args, event)
    }
}

fn persist_profile_publication(
    config: &RuntimeConfig,
    resolved: &mut ResolvedFarmConfig,
    event_id: String,
) -> Result<(), RuntimeError> {
    persist_publication(config, resolved, Some(event_id), None)
}

fn persist_farm_publication(
    config: &RuntimeConfig,
    resolved: &mut ResolvedFarmConfig,
    event_id: String,
) -> Result<(), RuntimeError> {
    persist_publication(config, resolved, None, Some(event_id))
}

fn persist_publication(
    config: &RuntimeConfig,
    resolved: &mut ResolvedFarmConfig,
    profile_event_id: Option<String>,
    farm_event_id: Option<String>,
) -> Result<(), RuntimeError> {
    let published_at = now_unix();
    if let Some(event_id) = profile_event_id.and_then(|value| non_empty(value.as_str())) {
        resolved.document.publication.profile_event_id = Some(event_id);
        resolved.document.publication.profile_published_at = Some(published_at);
    }
    if let Some(event_id) = farm_event_id.and_then(|value| non_empty(value.as_str())) {
        resolved.document.publication.farm_event_id = Some(event_id);
        resolved.document.publication.farm_published_at = Some(published_at);
    }
    farm_config::write(&config.paths, resolved.scope, &resolved.document)?;
    Ok(())
}

fn farm_write_source(config: &RuntimeConfig) -> &'static str {
    match config.publish.mode {
        PublishMode::NostrRelay => RELAY_FARM_WRITE_SOURCE,
        PublishMode::Radrootsd => RADROOTSD_FARM_WRITE_SOURCE,
    }
}

fn profile_publish_rpc_method(config: &RuntimeConfig) -> &'static str {
    match config.publish.mode {
        PublishMode::NostrRelay => "relay.profile.publish",
        PublishMode::Radrootsd => RADROOTSD_BRIDGE_PROFILE_PUBLISH_METHOD,
    }
}

fn farm_publish_rpc_method(config: &RuntimeConfig) -> &'static str {
    match config.publish.mode {
        PublishMode::NostrRelay => "relay.farm.publish",
        PublishMode::Radrootsd => RADROOTSD_BRIDGE_FARM_PUBLISH_METHOD,
    }
}

#[derive(Debug, Clone)]
struct FarmPublishFailureDetails {
    event_id: Option<String>,
    target_relays: Vec<String>,
    connected_relays: Vec<String>,
    failed_relays: Vec<RelayFailureView>,
}

fn publish_failure_details(
    error: &DirectRelayPublishError,
    relay_urls: &[String],
) -> FarmPublishFailureDetails {
    match error {
        DirectRelayPublishError::MissingRelays
        | DirectRelayPublishError::Runtime(_)
        | DirectRelayPublishError::Build(_)
        | DirectRelayPublishError::Sign(_) => FarmPublishFailureDetails {
            event_id: None,
            target_relays: relay_urls.to_vec(),
            connected_relays: Vec::new(),
            failed_relays: Vec::new(),
        },
        DirectRelayPublishError::RelayConfig { relay, source } => FarmPublishFailureDetails {
            event_id: None,
            target_relays: relay_urls.to_vec(),
            connected_relays: Vec::new(),
            failed_relays: vec![RelayFailureView {
                relay: relay.clone(),
                reason: source.to_string(),
            }],
        },
        DirectRelayPublishError::Connect {
            target_relays,
            connected_relays,
            failed_relays,
            ..
        } => FarmPublishFailureDetails {
            event_id: None,
            target_relays: target_relays.clone(),
            connected_relays: connected_relays.clone(),
            failed_relays: relay_failures(failed_relays.clone()),
        },
        DirectRelayPublishError::Publish {
            event_id,
            target_relays,
            connected_relays,
            failed_relays,
            ..
        } => FarmPublishFailureDetails {
            event_id: Some(event_id.clone()),
            target_relays: target_relays.clone(),
            connected_relays: connected_relays.clone(),
            failed_relays: relay_failures(failed_relays.clone()),
        },
    }
}

fn relay_failures(failures: Vec<DirectRelayFailure>) -> Vec<RelayFailureView> {
    failures
        .into_iter()
        .map(|failure| RelayFailureView {
            relay: failure.relay,
            reason: failure.reason,
        })
        .collect()
}

fn selected_account_for_draft(
    config: &RuntimeConfig,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    account::resolve_account(config)
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
    args: &FarmCreateArgs,
) -> Result<FarmConfigDocument, RuntimeError> {
    let existing_document = existing.map(|resolved| &resolved.document);
    if let Some(document) = existing_document
        && document.selection.account != account.record.account_id.to_string()
    {
        let message = format!(
            "account mismatch: farm config is bound to seller account `{}`; use `radroots farm rebind {}` to change the farm-bound seller account",
            document.selection.account, account.record.account_id
        );
        return Err(account::AccountRuntimeFailure::mismatch_with_detail(
            message,
            json!({
                "seller_actor_source": FARM_SELLER_ACTOR_SOURCE,
                "farm_bound_seller_account_id": document.selection.account,
                "attempted_seller_account_id": account.record.account_id.to_string(),
                "actions": [format!("radroots farm rebind {}", account.record.account_id)],
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
    append_farm_local_work(
        config,
        scope,
        written_path.display().to_string(),
        document,
        Some(account.record.public_identity.public_key_hex.as_str()),
    )?;
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
    let mut actions = vec!["radroots farm readiness check".to_owned()];
    if account.is_none() {
        actions.extend(farm_bound_seller_recovery_actions());
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

fn farm_bound_seller_recovery_actions() -> Vec<String> {
    vec![
        "radroots account import <path>".to_owned(),
        "radroots farm rebind <selector>".to_owned(),
    ]
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

fn configured_account(
    config: &RuntimeConfig,
    account_id: &str,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    let snapshot = account::snapshot(config)?;
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
    use super::generate_d_tag;
    use radroots_events_codec::d_tag::is_d_tag_base64url;

    #[test]
    fn generated_farm_d_tag_is_valid_base64url() {
        assert!(is_d_tag_base64url(&generate_d_tag()));
    }
}
