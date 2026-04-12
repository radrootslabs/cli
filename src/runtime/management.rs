use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use getrandom::getrandom;
use radroots_runtime_distribution::{RadrootsRuntimeDistributionResolver, RuntimeArtifactRequest};
use radroots_runtime_manager::{
    ManagedRuntimeActionInspection, ManagedRuntimeConfigInspection,
    ManagedRuntimeContext as RuntimeManagementContext, ManagedRuntimeGroup as RuntimeGroup,
    ManagedRuntimeInspection, ManagedRuntimeInspectionAvailability, ManagedRuntimeInstallState,
    ManagedRuntimeInstancePaths, ManagedRuntimeInstanceRecord, ManagedRuntimeLifecycleAction,
    ManagedRuntimeLogsInspection, ManagedRuntimeStatusInspection,
    ManagedRuntimeTarget as RuntimeTarget, extract_binary_archive,
    inspect_runtime_action as inspect_target_action,
    inspect_runtime_config as inspect_target_config, inspect_runtime_logs as inspect_target_logs,
    inspect_runtime_status as inspect_target_status, load_management_context_with_selection,
    parse_contract_str, process_running as managed_process_running, remove_instance,
    remove_instance_artifacts, resolve_runtime_target, save_registry, start_process, stop_process,
    upsert_instance, write_instance_metadata, write_managed_file, write_secret_file,
};
use radroots_runtime_paths::{RadrootsPathResolver, RadrootsRuntimePathSelection};
use serde::{Deserialize, Serialize};

use crate::domain::runtime::{
    RuntimeActionView, RuntimeInstancePathsView, RuntimeInstanceRecordView, RuntimeLogsView,
    RuntimeManagedConfigView, RuntimeStatusView,
};
use crate::runtime::{RuntimeError, config::RuntimeConfig};

const MANAGEMENT_CONTRACT_RAW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../foundation/contracts/runtime/management.toml"
));
const DISTRIBUTION_CONTRACT_RAW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../foundation/contracts/runtime/distribution.toml"
));
const RADROOTSD_RUNTIME_ID: &str = "radrootsd";
const RADROOTSD_BINARY_NAME: &str = "radrootsd";
const RADROOTSD_ARTIFACT_CHANNEL: &str = "stable";
const RADROOTSD_DEFAULT_RPC_ADDR: &str = "127.0.0.1:7070";
const RADROOTSD_DEFAULT_METADATA_NAME: &str = "radrootsd";
const RADROOTSD_BRIDGE_TOKEN_FILE: &str = "bridge-bearer-token.txt";
const RADROOTSD_IDENTITY_FILE: &str = "identity.secret.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRadrootsdSettingsFile {
    metadata: ManagedRadrootsdMetadata,
    config: ManagedRadrootsdConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRadrootsdMetadata {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRadrootsdConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relays: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logs_dir: Option<String>,
    rpc: ManagedRadrootsdRpc,
    bridge: ManagedRadrootsdBridge,
    #[serde(default)]
    nip46: ManagedRadrootsdNip46,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRadrootsdRpc {
    addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRadrootsdBridge {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    bearer_token: Option<String>,
    delivery_policy: String,
    publish_max_attempts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedRadrootsdNip46 {
    public_jsonrpc_enabled: bool,
    session_ttl_secs: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    perms: Vec<String>,
}

impl Default for ManagedRadrootsdNip46 {
    fn default() -> Self {
        Self {
            public_jsonrpc_enabled: false,
            session_ttl_secs: 900,
            perms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfigMutationRequest {
    pub runtime_id: String,
    pub instance_id: Option<String>,
    pub key: String,
    pub value: String,
}

pub type RuntimeCommandAvailability = ManagedRuntimeInspectionAvailability;
pub type RuntimeLifecycleAction = ManagedRuntimeLifecycleAction;

type RuntimeInspection<T> = ManagedRuntimeInspection<T>;

pub fn inspect_status(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
) -> Result<RuntimeInspection<RuntimeStatusView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let inspection = inspect_target_status(&target, &context.contract.lifecycle.actions);
    Ok(RuntimeInspection {
        availability: inspection.availability,
        view: runtime_status_view(inspection.view),
    })
}

pub fn inspect_logs(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
) -> Result<RuntimeInspection<RuntimeLogsView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let inspection = inspect_target_logs(&target);
    Ok(RuntimeInspection {
        availability: inspection.availability,
        view: runtime_logs_view(inspection.view),
    })
}

pub fn inspect_config_show(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
) -> Result<RuntimeInspection<RuntimeManagedConfigView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let inspection = inspect_target_config(&target);
    Ok(RuntimeInspection {
        availability: inspection.availability,
        view: runtime_config_view(inspection.view),
    })
}

pub fn inspect_action(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
    action: RuntimeLifecycleAction,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let mut context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    if target.runtime_group == RuntimeGroup::ActiveManagedTarget {
        return execute_action(config, &mut context, target, action);
    }
    let inspection = inspect_target_action(&target, action, None);
    Ok(RuntimeInspection {
        availability: inspection.availability,
        view: runtime_action_view(inspection.view),
    })
}

pub fn inspect_config_set(
    config: &RuntimeConfig,
    request: &RuntimeConfigMutationRequest,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let mut context = load_management_context(config)?;
    let target = resolve_runtime_target(
        &context,
        request.runtime_id.as_str(),
        request.instance_id.as_deref(),
    );
    if target.runtime_group == RuntimeGroup::ActiveManagedTarget {
        return execute_config_set(config, &mut context, target, request);
    }
    let detail = Some(format!(
        "requested managed config mutation {}={} for runtime `{}` instance `{}`; runtime `{}` is not an active managed target in this wave",
        request.key, request.value, target.runtime_id, target.instance_id, target.runtime_id
    ));
    let inspection = inspect_target_action(&target, RuntimeLifecycleAction::ConfigSet, detail);
    Ok(RuntimeInspection {
        availability: inspection.availability,
        view: runtime_action_view(inspection.view),
    })
}

fn load_management_context(
    config: &RuntimeConfig,
) -> Result<RuntimeManagementContext, RuntimeError> {
    let contract = parse_contract_str(MANAGEMENT_CONTRACT_RAW)?;
    let selection = RadrootsRuntimePathSelection::from_profile_value(
        config.paths.profile.as_str(),
        config.paths.repo_local_root.clone(),
    )
    .map_err(|error| RuntimeError::Config(error.to_string()))?;
    let resolver = RadrootsPathResolver::current();
    load_management_context_with_selection(contract, &resolver, &selection)
        .map_err(RuntimeError::from)
}

fn runtime_status_view(view: ManagedRuntimeStatusInspection) -> RuntimeStatusView {
    RuntimeStatusView {
        runtime_id: view.runtime_id,
        instance_id: view.instance_id,
        instance_source: view.instance_source,
        runtime_group: view.runtime_group,
        management_posture: view.management_posture,
        state: view.state,
        source: view.source,
        detail: view.detail,
        management_mode: view.management_mode,
        service_manager_integration: view.service_manager_integration,
        uses_absolute_binary_paths: view.uses_absolute_binary_paths,
        preferred_cli_binding: view.preferred_cli_binding,
        install_state: view.install_state,
        health_state: view.health_state,
        health_source: view.health_source,
        registry_path: view.registry_path.display().to_string(),
        lifecycle_actions: view.lifecycle_actions,
        instance_paths: view.instance_paths.map(runtime_instance_paths_view),
        instance_record: view.instance_record.map(runtime_instance_record_view),
    }
}

fn runtime_logs_view(view: ManagedRuntimeLogsInspection) -> RuntimeLogsView {
    RuntimeLogsView {
        runtime_id: view.runtime_id,
        instance_id: view.instance_id,
        instance_source: view.instance_source,
        runtime_group: view.runtime_group,
        state: view.state,
        source: view.source,
        detail: view.detail,
        stdout_log_path: view.stdout_log_path.map(|path| path.display().to_string()),
        stderr_log_path: view.stderr_log_path.map(|path| path.display().to_string()),
        stdout_log_present: view.stdout_log_present,
        stderr_log_present: view.stderr_log_present,
    }
}

fn runtime_config_view(view: ManagedRuntimeConfigInspection) -> RuntimeManagedConfigView {
    RuntimeManagedConfigView {
        runtime_id: view.runtime_id,
        instance_id: view.instance_id,
        instance_source: view.instance_source,
        runtime_group: view.runtime_group,
        state: view.state,
        source: view.source,
        detail: view.detail,
        config_format: view.config_format,
        config_path: view.config_path.map(|path| path.display().to_string()),
        config_present: view.config_present,
        requires_bootstrap_secret: view.requires_bootstrap_secret,
        requires_config_bootstrap: view.requires_config_bootstrap,
        requires_signer_provider: view.requires_signer_provider,
    }
}

fn runtime_action_view(view: ManagedRuntimeActionInspection) -> RuntimeActionView {
    RuntimeActionView {
        action: view.action,
        runtime_id: view.runtime_id,
        instance_id: view.instance_id,
        instance_source: view.instance_source,
        runtime_group: view.runtime_group,
        state: view.state,
        source: view.source,
        detail: view.detail,
        mutates_bindings: view.mutates_bindings,
        next_step: view.next_step,
    }
}

fn execute_action(
    config: &RuntimeConfig,
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
    action: RuntimeLifecycleAction,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    if target.runtime_id != RADROOTSD_RUNTIME_ID {
        let inspection = inspect_target_action(
            &target,
            action,
            Some(format!(
                "runtime `{}` is not admitted as an active managed implementation in this wave",
                target.runtime_id
            )),
        );
        return Ok(RuntimeInspection {
            availability: inspection.availability,
            view: runtime_action_view(inspection.view),
        });
    }

    match action {
        RuntimeLifecycleAction::Install => install_managed_radrootsd(config, context, target),
        RuntimeLifecycleAction::Start => start_managed_radrootsd(config, context, target),
        RuntimeLifecycleAction::Stop => stop_managed_radrootsd(context, target),
        RuntimeLifecycleAction::Restart => restart_managed_radrootsd(config, context, target),
        RuntimeLifecycleAction::Uninstall => uninstall_managed_radrootsd(context, target),
        RuntimeLifecycleAction::ConfigSet => unreachable!("config set is handled separately"),
    }
}

fn execute_config_set(
    _config: &RuntimeConfig,
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
    request: &RuntimeConfigMutationRequest,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    if target.runtime_id != RADROOTSD_RUNTIME_ID {
        let inspection = inspect_target_action(
            &target,
            RuntimeLifecycleAction::ConfigSet,
            Some(format!(
                "runtime `{}` is not admitted as an active managed implementation in this wave",
                target.runtime_id
            )),
        );
        return Ok(RuntimeInspection {
            availability: inspection.availability,
            view: runtime_action_view(inspection.view),
        });
    }

    let Some(predicted_paths) = target.predicted_paths.as_ref() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::ConfigSet,
            "active managed runtime is missing predicted instance paths".to_owned(),
        ));
    };
    let Some(mut record) = target.instance_record.clone() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::ConfigSet,
            format!(
                "managed runtime `{}` instance `{}` is not installed; run `radroots runtime install {}` first",
                target.runtime_id, target.instance_id, target.runtime_id
            ),
        ));
    };

    let mut settings = load_managed_radrootsd_settings(&record.config_path)?;
    let token_path = managed_radrootsd_token_path(predicted_paths);
    let identity_path = managed_radrootsd_identity_path(predicted_paths);
    apply_managed_radrootsd_config_mutation(
        &mut settings,
        &mut record,
        predicted_paths,
        request.key.as_str(),
        request.value.as_str(),
        token_path.as_path(),
    )?;
    write_secret_material_state(&settings, &mut record, token_path.as_path())?;
    save_managed_radrootsd_settings(record.config_path.as_path(), &settings)?;
    write_instance_metadata(predicted_paths, &record)?;
    upsert_instance(&mut context.registry, record.clone());
    save_registry(
        &context.shared_paths.instance_registry_path,
        &context.registry,
    )?;

    Ok(RuntimeInspection {
        availability: RuntimeCommandAvailability::Success,
        view: RuntimeActionView {
            action: RuntimeLifecycleAction::ConfigSet.as_str().to_owned(),
            runtime_id: target.runtime_id,
            instance_id: target.instance_id,
            instance_source: target.instance_source,
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: "configured".to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail: format!(
                "updated managed {} instance `{}` config key `{}`; config path = {}, identity path = {}",
                RADROOTSD_RUNTIME_ID,
                record.instance_id,
                request.key,
                record.config_path.display(),
                identity_path.display()
            ),
            mutates_bindings: false,
            next_step: None,
        },
    })
}

fn install_managed_radrootsd(
    _config: &RuntimeConfig,
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let Some(predicted_paths) = target.predicted_paths.as_ref() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Install,
            "active managed runtime is missing predicted instance paths".to_owned(),
        ));
    };

    let artifact = resolve_radrootsd_artifact(&context.shared_paths)?;
    let binary_path = extract_binary_archive(
        artifact.archive_path.as_path(),
        artifact.archive_format.as_str(),
        predicted_paths,
        artifact.binary_name.as_str(),
    )?;

    let rpc_addr = RADROOTSD_DEFAULT_RPC_ADDR.to_owned();
    let health_endpoint = rpc_addr_to_http_url(rpc_addr.as_str())?;
    let token_path = managed_radrootsd_token_path(predicted_paths);
    let bridge_token = generate_bridge_token()?;
    let config_path = predicted_paths.state_dir.join("config.toml");
    let settings = bootstrap_managed_radrootsd_settings(
        predicted_paths,
        rpc_addr.as_str(),
        bridge_token.as_str(),
    );
    write_secret_file(token_path.as_path(), bridge_token.as_str())?;
    save_managed_radrootsd_settings(config_path.as_path(), &settings)?;

    let record = ManagedRuntimeInstanceRecord {
        runtime_id: target.runtime_id.clone(),
        instance_id: target.instance_id.clone(),
        management_mode: target
            .management_mode
            .clone()
            .unwrap_or_else(|| "interactive_user_managed".to_owned()),
        install_state: ManagedRuntimeInstallState::Configured,
        binary_path: binary_path.clone(),
        config_path: config_path.clone(),
        logs_path: predicted_paths.logs_dir.clone(),
        run_path: predicted_paths.run_dir.clone(),
        installed_version: artifact.version.clone(),
        health_endpoint: Some(health_endpoint.clone()),
        secret_material_ref: Some(token_path.display().to_string()),
        last_started_at: None,
        last_stopped_at: None,
        notes: Some(format!(
            "installed from artifact cache {}",
            artifact.archive_path.display()
        )),
    };
    write_instance_metadata(predicted_paths, &record)?;
    upsert_instance(&mut context.registry, record.clone());
    save_registry(
        &context.shared_paths.instance_registry_path,
        &context.registry,
    )?;

    let identity_path = managed_radrootsd_identity_path(predicted_paths);
    Ok(RuntimeInspection {
        availability: RuntimeCommandAvailability::Success,
        view: RuntimeActionView {
            action: RuntimeLifecycleAction::Install.as_str().to_owned(),
            runtime_id: target.runtime_id,
            instance_id: target.instance_id,
            instance_source: target.instance_source,
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: "configured".to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail: format!(
                "installed managed {RADROOTSD_RUNTIME_ID} instance `{}` from artifact {} to {}; config = {}; identity bootstrap path = {}; health endpoint = {}",
                record.instance_id,
                artifact.archive_path.display(),
                binary_path.display(),
                config_path.display(),
                identity_path.display(),
                health_endpoint
            ),
            mutates_bindings: false,
            next_step: None,
        },
    })
}

fn start_managed_radrootsd(
    config: &RuntimeConfig,
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let Some(predicted_paths) = target.predicted_paths.as_ref() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Start,
            "active managed runtime is missing predicted instance paths".to_owned(),
        ));
    };
    let Some(mut record) = target.instance_record.clone() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Start,
            format!(
                "managed runtime `{}` instance `{}` is not installed; run `radroots runtime install {}` first",
                target.runtime_id, target.instance_id, target.runtime_id
            ),
        ));
    };

    if managed_process_running(predicted_paths)? {
        return Ok(RuntimeInspection {
            availability: RuntimeCommandAvailability::Success,
            view: RuntimeActionView {
                action: RuntimeLifecycleAction::Start.as_str().to_owned(),
                runtime_id: target.runtime_id,
                instance_id: target.instance_id,
                instance_source: target.instance_source,
                runtime_group: target.runtime_group.as_str().to_owned(),
                state: "running".to_owned(),
                source: "generic runtime-management command family".to_owned(),
                detail: format!(
                    "managed {} instance `{}` is already running from {}",
                    RADROOTSD_RUNTIME_ID,
                    record.instance_id,
                    record.binary_path.display()
                ),
                mutates_bindings: false,
                next_step: None,
            },
        });
    }

    let args = vec![
        "--config".to_owned(),
        record.config_path.display().to_string(),
        "--identity".to_owned(),
        managed_radrootsd_identity_path(predicted_paths)
            .display()
            .to_string(),
        "--allow-generate-identity".to_owned(),
    ];
    let envs = managed_radrootsd_start_envs(config);
    let pid = start_process(record.binary_path.as_path(), &args, &envs, predicted_paths)?;
    record.last_started_at = Some(Utc::now().to_rfc3339());
    write_instance_metadata(predicted_paths, &record)?;
    upsert_instance(&mut context.registry, record.clone());
    save_registry(
        &context.shared_paths.instance_registry_path,
        &context.registry,
    )?;

    Ok(RuntimeInspection {
        availability: RuntimeCommandAvailability::Success,
        view: RuntimeActionView {
            action: RuntimeLifecycleAction::Start.as_str().to_owned(),
            runtime_id: target.runtime_id,
            instance_id: target.instance_id,
            instance_source: target.instance_source,
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: "running".to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail: format!(
                "started managed {} instance `{}` with pid {} using config {}",
                RADROOTSD_RUNTIME_ID,
                record.instance_id,
                pid,
                record.config_path.display()
            ),
            mutates_bindings: false,
            next_step: None,
        },
    })
}

fn stop_managed_radrootsd(
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let Some(predicted_paths) = target.predicted_paths.as_ref() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Stop,
            "active managed runtime is missing predicted instance paths".to_owned(),
        ));
    };
    let Some(mut record) = target.instance_record.clone() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Stop,
            format!(
                "managed runtime `{}` instance `{}` is not installed",
                target.runtime_id, target.instance_id
            ),
        ));
    };

    let stopped = stop_process(predicted_paths)?;
    record.last_stopped_at = Some(Utc::now().to_rfc3339());
    write_instance_metadata(predicted_paths, &record)?;
    upsert_instance(&mut context.registry, record.clone());
    save_registry(
        &context.shared_paths.instance_registry_path,
        &context.registry,
    )?;

    Ok(RuntimeInspection {
        availability: RuntimeCommandAvailability::Success,
        view: RuntimeActionView {
            action: RuntimeLifecycleAction::Stop.as_str().to_owned(),
            runtime_id: target.runtime_id,
            instance_id: target.instance_id,
            instance_source: target.instance_source,
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: "stopped".to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail: if stopped {
                format!(
                    "stopped managed {} instance `{}`",
                    RADROOTSD_RUNTIME_ID, record.instance_id
                )
            } else {
                format!(
                    "managed {} instance `{}` was already stopped",
                    RADROOTSD_RUNTIME_ID, record.instance_id
                )
            },
            mutates_bindings: false,
            next_step: None,
        },
    })
}

fn restart_managed_radrootsd(
    config: &RuntimeConfig,
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let stop_result = stop_managed_radrootsd(context, target.clone())?;
    if stop_result.availability != RuntimeCommandAvailability::Success {
        return Ok(stop_result);
    }
    let refreshed_target = resolve_runtime_target(
        context,
        RADROOTSD_RUNTIME_ID,
        Some(target.instance_id.as_str()),
    );
    let start_result = start_managed_radrootsd(config, context, refreshed_target)?;
    Ok(RuntimeInspection {
        availability: start_result.availability,
        view: RuntimeActionView {
            action: RuntimeLifecycleAction::Restart.as_str().to_owned(),
            runtime_id: start_result.view.runtime_id,
            instance_id: start_result.view.instance_id,
            instance_source: start_result.view.instance_source,
            runtime_group: start_result.view.runtime_group,
            state: start_result.view.state,
            source: start_result.view.source,
            detail: format!(
                "restarted managed {} instance `{}`",
                RADROOTSD_RUNTIME_ID, target.instance_id
            ),
            mutates_bindings: false,
            next_step: None,
        },
    })
}

fn uninstall_managed_radrootsd(
    context: &mut RuntimeManagementContext,
    target: RuntimeTarget,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let Some(predicted_paths) = target.predicted_paths.as_ref() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Uninstall,
            "active managed runtime is missing predicted instance paths".to_owned(),
        ));
    };
    let Some(record) = target.instance_record.clone() else {
        return Ok(runtime_action_unconfigured(
            &target,
            RuntimeLifecycleAction::Uninstall,
            format!(
                "managed runtime `{}` instance `{}` is not installed",
                target.runtime_id, target.instance_id
            ),
        ));
    };

    let _ = stop_process(predicted_paths);
    remove_instance_artifacts(predicted_paths)?;
    remove_instance(
        &mut context.registry,
        record.runtime_id.as_str(),
        record.instance_id.as_str(),
    );
    save_registry(
        &context.shared_paths.instance_registry_path,
        &context.registry,
    )?;

    Ok(RuntimeInspection {
        availability: RuntimeCommandAvailability::Success,
        view: RuntimeActionView {
            action: RuntimeLifecycleAction::Uninstall.as_str().to_owned(),
            runtime_id: target.runtime_id,
            instance_id: target.instance_id,
            instance_source: target.instance_source,
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: "uninstalled".to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail: format!(
                "uninstalled managed {} instance `{}` and removed {}",
                RADROOTSD_RUNTIME_ID,
                record.instance_id,
                predicted_paths
                    .install_dir
                    .parent()
                    .unwrap_or(predicted_paths.install_dir.as_path())
                    .display()
            ),
            mutates_bindings: false,
            next_step: None,
        },
    })
}

fn runtime_action_unconfigured(
    target: &RuntimeTarget,
    action: RuntimeLifecycleAction,
    detail: String,
) -> RuntimeInspection<RuntimeActionView> {
    RuntimeInspection {
        availability: RuntimeCommandAvailability::Unconfigured,
        view: RuntimeActionView {
            action: action.as_str().to_owned(),
            runtime_id: target.runtime_id.clone(),
            instance_id: target.instance_id.clone(),
            instance_source: target.instance_source.clone(),
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: "not_installed".to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail,
            mutates_bindings: false,
            next_step: None,
        },
    }
}

#[derive(Debug, Clone)]
struct ResolvedManagedArtifact {
    archive_path: PathBuf,
    archive_format: String,
    binary_name: String,
    version: String,
}

fn resolve_radrootsd_artifact(
    shared_paths: &radroots_runtime_manager::ManagedRuntimeSharedPaths,
) -> Result<ResolvedManagedArtifact, RuntimeError> {
    let resolver = RadrootsRuntimeDistributionResolver::parse_str(DISTRIBUTION_CONTRACT_RAW)?;
    let request = RuntimeArtifactRequest {
        runtime_id: RADROOTSD_RUNTIME_ID,
        os: current_distribution_os(),
        arch: current_distribution_arch(),
        version: "0.0.0",
        channel: Some(RADROOTSD_ARTIFACT_CHANNEL),
    };
    let artifact = resolver.resolve_artifact(&request)?;
    let search_root = shared_paths.artifact_cache_dir.join(RADROOTSD_RUNTIME_ID);
    let matches = find_cached_artifacts(
        search_root.as_path(),
        RADROOTSD_RUNTIME_ID,
        artifact.target_id.as_str(),
        artifact.archive_extension.as_str(),
    )?;
    match matches.as_slice() {
        [] => Err(RuntimeError::Config(format!(
            "no cached {RADROOTSD_RUNTIME_ID} artifact found under {} for target {}{}",
            search_root.display(),
            artifact.target_id,
            artifact.archive_extension
        ))),
        [found] => Ok(found.clone()),
        _ => Err(RuntimeError::Config(format!(
            "multiple cached {RADROOTSD_RUNTIME_ID} artifacts found under {}; keep exactly one matching target {}{}",
            search_root.display(),
            artifact.target_id,
            artifact.archive_extension
        ))),
    }
}

fn find_cached_artifacts(
    root: &Path,
    runtime_id: &str,
    target_id: &str,
    extension: &str,
) -> Result<Vec<ResolvedManagedArtifact>, RuntimeError> {
    let mut matches = Vec::new();
    if !root.exists() {
        return Ok(matches);
    }
    collect_cached_artifacts(root, runtime_id, target_id, extension, &mut matches)?;
    Ok(matches)
}

fn collect_cached_artifacts(
    root: &Path,
    runtime_id: &str,
    target_id: &str,
    extension: &str,
    matches: &mut Vec<ResolvedManagedArtifact>,
) -> Result<(), RuntimeError> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_cached_artifacts(path.as_path(), runtime_id, target_id, extension, matches)?;
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let prefix = format!("{runtime_id}-");
        let suffix = format!("-{target_id}{extension}");
        if !file_name.starts_with(prefix.as_str()) || !file_name.ends_with(suffix.as_str()) {
            continue;
        }
        let version = file_name
            .strip_prefix(prefix.as_str())
            .and_then(|value| value.strip_suffix(suffix.as_str()))
            .ok_or_else(|| {
                RuntimeError::Config(format!(
                    "invalid cached artifact name `{file_name}` under {}",
                    root.display()
                ))
            })?;
        matches.push(ResolvedManagedArtifact {
            archive_path: path.clone(),
            archive_format: archive_format_from_extension(extension).to_owned(),
            binary_name: RADROOTSD_BINARY_NAME.to_owned(),
            version: version.to_owned(),
        });
    }
    Ok(())
}

fn archive_format_from_extension(extension: &str) -> &str {
    match extension {
        ".tar.gz" => "tar.gz",
        other => other.trim_start_matches('.'),
    }
}

fn bootstrap_managed_radrootsd_settings(
    predicted_paths: &radroots_runtime_manager::ManagedRuntimeInstancePaths,
    rpc_addr: &str,
    bridge_token: &str,
) -> ManagedRadrootsdSettingsFile {
    ManagedRadrootsdSettingsFile {
        metadata: ManagedRadrootsdMetadata {
            name: RADROOTSD_DEFAULT_METADATA_NAME.to_owned(),
        },
        config: ManagedRadrootsdConfig {
            relays: Vec::new(),
            logs_dir: Some(predicted_paths.logs_dir.display().to_string()),
            rpc: ManagedRadrootsdRpc {
                addr: rpc_addr.to_owned(),
            },
            bridge: ManagedRadrootsdBridge {
                enabled: true,
                bearer_token: Some(bridge_token.to_owned()),
                delivery_policy: "any".to_owned(),
                publish_max_attempts: 2,
                state_path: Some(
                    predicted_paths
                        .state_dir
                        .join("bridge/bridge-jobs.json")
                        .display()
                        .to_string(),
                ),
            },
            nip46: ManagedRadrootsdNip46::default(),
        },
    }
}

fn load_managed_radrootsd_settings(
    path: &Path,
) -> Result<ManagedRadrootsdSettingsFile, RuntimeError> {
    let raw = fs::read_to_string(path)?;
    toml::from_str(raw.as_str()).map_err(|err| {
        RuntimeError::Config(format!(
            "parse managed {RADROOTSD_RUNTIME_ID} config {}: {err}",
            path.display()
        ))
    })
}

fn save_managed_radrootsd_settings(
    path: &Path,
    settings: &ManagedRadrootsdSettingsFile,
) -> Result<(), RuntimeError> {
    let raw = toml::to_string_pretty(settings).map_err(|err| {
        RuntimeError::Config(format!(
            "serialize managed {RADROOTSD_RUNTIME_ID} config {}: {err}",
            path.display()
        ))
    })?;
    write_managed_file(path, raw.as_str())?;
    Ok(())
}

fn apply_managed_radrootsd_config_mutation(
    settings: &mut ManagedRadrootsdSettingsFile,
    record: &mut ManagedRuntimeInstanceRecord,
    predicted_paths: &radroots_runtime_manager::ManagedRuntimeInstancePaths,
    key: &str,
    value: &str,
    token_path: &Path,
) -> Result<(), RuntimeError> {
    match key {
        "metadata.name" => {
            settings.metadata.name = non_empty_value(key, value)?;
        }
        "config.logs_dir" => {
            settings.config.logs_dir = Some(non_empty_value(key, value)?);
        }
        "config.rpc.addr" => {
            let rpc_addr = non_empty_value(key, value)?;
            settings.config.rpc.addr = rpc_addr.clone();
            record.health_endpoint = Some(rpc_addr_to_http_url(rpc_addr.as_str())?);
        }
        "config.bridge.enabled" => {
            let enabled = parse_bool(value, key)?;
            settings.config.bridge.enabled = enabled;
            if !enabled {
                settings.config.bridge.bearer_token = None;
                if token_path.exists() {
                    fs::remove_file(token_path)?;
                }
                record.secret_material_ref = None;
            }
        }
        "config.bridge.bearer_token" => {
            let token = non_empty_value(key, value)?;
            settings.config.bridge.enabled = true;
            settings.config.bridge.bearer_token = Some(token.clone());
            write_secret_file(token_path, token.as_str())?;
            record.secret_material_ref = Some(token_path.display().to_string());
        }
        "config.bridge.state_path" => {
            settings.config.bridge.state_path = Some(non_empty_value(key, value)?);
        }
        other => {
            return Err(RuntimeError::Config(format!(
                "unsupported managed {RADROOTSD_RUNTIME_ID} config key `{other}`; supported keys: metadata.name, config.logs_dir, config.rpc.addr, config.bridge.enabled, config.bridge.bearer_token, config.bridge.state_path"
            )));
        }
    }

    if settings.config.logs_dir.is_none() {
        settings.config.logs_dir = Some(predicted_paths.logs_dir.display().to_string());
    }
    if settings.config.bridge.state_path.is_none() {
        settings.config.bridge.state_path = Some(
            predicted_paths
                .state_dir
                .join("bridge/bridge-jobs.json")
                .display()
                .to_string(),
        );
    }
    Ok(())
}

fn write_secret_material_state(
    settings: &ManagedRadrootsdSettingsFile,
    record: &mut ManagedRuntimeInstanceRecord,
    token_path: &Path,
) -> Result<(), RuntimeError> {
    if settings.config.bridge.enabled {
        let token = settings
            .config
            .bridge
            .bearer_token
            .as_deref()
            .ok_or_else(|| {
                RuntimeError::Config(format!(
                    "managed {RADROOTSD_RUNTIME_ID} bridge is enabled but bearer_token is missing"
                ))
            })?;
        write_secret_file(token_path, token)?;
        record.secret_material_ref = Some(token_path.display().to_string());
    } else {
        record.secret_material_ref = None;
    }
    Ok(())
}

fn managed_radrootsd_start_envs(config: &RuntimeConfig) -> Vec<(String, String)> {
    let mut envs = Vec::new();
    envs.push((
        "RADROOTSD_PATHS_PROFILE".to_owned(),
        config.paths.profile.clone(),
    ));
    if config.paths.profile == "repo_local" {
        if let Some(root) = &config.paths.repo_local_root {
            envs.push((
                "RADROOTSD_PATHS_REPO_LOCAL_ROOT".to_owned(),
                root.display().to_string(),
            ));
        }
    }
    envs
}

fn managed_radrootsd_token_path(
    predicted_paths: &radroots_runtime_manager::ManagedRuntimeInstancePaths,
) -> PathBuf {
    predicted_paths
        .secrets_dir
        .join(RADROOTSD_BRIDGE_TOKEN_FILE)
}

fn managed_radrootsd_identity_path(
    predicted_paths: &radroots_runtime_manager::ManagedRuntimeInstancePaths,
) -> PathBuf {
    predicted_paths.secrets_dir.join(RADROOTSD_IDENTITY_FILE)
}

fn current_distribution_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => other,
    }
}

fn current_distribution_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn non_empty_value(key: &str, value: &str) -> Result<String, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(format!(
            "managed config key `{key}` must not be empty"
        )));
    }
    Ok(trimmed.to_owned())
}

fn parse_bool(value: &str, key: &str) -> Result<bool, RuntimeError> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(RuntimeError::Config(format!(
            "managed config key `{key}` must be `true` or `false`, got `{other}`"
        ))),
    }
}

fn rpc_addr_to_http_url(value: &str) -> Result<String, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(
            "managed rpc addr must not be empty".to_owned(),
        ));
    }
    if trimmed.contains("://") {
        return Ok(trimmed.to_owned());
    }
    Ok(format!("http://{trimmed}"))
}

fn generate_bridge_token() -> Result<String, RuntimeError> {
    let mut bytes = [0_u8; 24];
    getrandom(&mut bytes)
        .map_err(|err| RuntimeError::Config(format!("generate bridge token: {err}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn runtime_instance_paths_view(paths: ManagedRuntimeInstancePaths) -> RuntimeInstancePathsView {
    RuntimeInstancePathsView {
        install_dir: paths.install_dir.display().to_string(),
        state_dir: paths.state_dir.display().to_string(),
        logs_dir: paths.logs_dir.display().to_string(),
        run_dir: paths.run_dir.display().to_string(),
        secrets_dir: paths.secrets_dir.display().to_string(),
        pid_file_path: paths.pid_file_path.display().to_string(),
        stdout_log_path: paths.stdout_log_path.display().to_string(),
        stderr_log_path: paths.stderr_log_path.display().to_string(),
        metadata_path: paths.metadata_path.display().to_string(),
    }
}

fn runtime_instance_record_view(record: ManagedRuntimeInstanceRecord) -> RuntimeInstanceRecordView {
    RuntimeInstanceRecordView {
        management_mode: record.management_mode,
        install_state: match record.install_state {
            ManagedRuntimeInstallState::NotInstalled => "not_installed".to_owned(),
            ManagedRuntimeInstallState::Installed => "installed".to_owned(),
            ManagedRuntimeInstallState::Configured => "configured".to_owned(),
            ManagedRuntimeInstallState::Failed => "failed".to_owned(),
        },
        binary_path: record.binary_path.display().to_string(),
        config_path: record.config_path.display().to_string(),
        logs_path: record.logs_path.display().to_string(),
        run_path: record.run_path.display().to_string(),
        installed_version: record.installed_version,
        health_endpoint: record.health_endpoint,
        secret_material_ref: record.secret_material_ref,
        last_started_at: record.last_started_at,
        last_stopped_at: record.last_stopped_at,
        notes: record.notes,
    }
}
