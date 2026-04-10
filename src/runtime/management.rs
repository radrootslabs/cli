use std::path::PathBuf;

use radroots_runtime_manager::{
    BootstrapRuntimeContract, ManagedRuntimeHealthState, ManagedRuntimeInstanceRecord,
    ManagedRuntimeInstanceRegistry, ManagedRuntimeInstallState, ManagementModeContract,
    RadrootsRuntimeManagementContract, parse_contract_str, resolve_instance_paths,
    resolve_shared_paths,
};
use radroots_runtime_paths::{RadrootsPathOverrides, RadrootsPathProfile, RadrootsPathResolver};

use crate::domain::runtime::{
    RuntimeActionView, RuntimeInstancePathsView, RuntimeInstanceRecordView, RuntimeLogsView,
    RuntimeManagedConfigView, RuntimeStatusView,
};
use crate::runtime::{RuntimeError, config::RuntimeConfig};

const MANAGEMENT_CONTRACT_RAW: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../foundation/contracts/runtime/management.toml"
));
const DEFERRED_LIFECYCLE_SLICE: &str = "rpv1-rpi.5";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommandAvailability {
    Success,
    Unconfigured,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct RuntimeInspection<T> {
    pub availability: RuntimeCommandAvailability,
    pub view: T,
}

#[derive(Debug, Clone, Copy)]
pub enum RuntimeLifecycleAction {
    Install,
    Uninstall,
    Start,
    Stop,
    Restart,
    ConfigSet,
}

impl RuntimeLifecycleAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::ConfigSet => "config_set",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeGroup {
    ActiveManagedTarget,
    DefinedManagedTarget,
    BootstrapOnly,
    Unknown,
}

impl RuntimeGroup {
    fn as_str(self) -> &'static str {
        match self {
            Self::ActiveManagedTarget => "active_managed_target",
            Self::DefinedManagedTarget => "defined_managed_target",
            Self::BootstrapOnly => "bootstrap_only",
            Self::Unknown => "unknown",
        }
    }

    fn posture(self) -> &'static str {
        match self {
            Self::ActiveManagedTarget => "active_managed_target",
            Self::DefinedManagedTarget => "defined_future_target",
            Self::BootstrapOnly => "bootstrap_only_direct_binding",
            Self::Unknown => "unknown_runtime",
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeManagementContext {
    contract: RadrootsRuntimeManagementContract,
    shared_paths: radroots_runtime_manager::ManagedRuntimeSharedPaths,
    registry: ManagedRuntimeInstanceRegistry,
}

#[derive(Debug, Clone)]
struct RuntimeTarget {
    runtime_id: String,
    instance_id: String,
    instance_source: String,
    runtime_group: RuntimeGroup,
    management_mode: Option<String>,
    mode_contract: Option<ManagementModeContract>,
    bootstrap: Option<BootstrapRuntimeContract>,
    instance_record: Option<ManagedRuntimeInstanceRecord>,
    predicted_paths: Option<radroots_runtime_manager::ManagedRuntimeInstancePaths>,
    registry_path: PathBuf,
}

pub fn inspect_status(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
) -> Result<RuntimeInspection<RuntimeStatusView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let availability = if target.runtime_group == RuntimeGroup::Unknown {
        RuntimeCommandAvailability::Unconfigured
    } else {
        RuntimeCommandAvailability::Success
    };
    Ok(RuntimeInspection {
        availability,
        view: status_view(&target, &context.contract.lifecycle.actions),
    })
}

pub fn inspect_logs(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
) -> Result<RuntimeInspection<RuntimeLogsView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let (availability, view) = logs_view(&target);
    Ok(RuntimeInspection { availability, view })
}

pub fn inspect_config_show(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
) -> Result<RuntimeInspection<RuntimeManagedConfigView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let (availability, view) = config_show_view(&target);
    Ok(RuntimeInspection { availability, view })
}

pub fn inspect_action(
    config: &RuntimeConfig,
    runtime_id: &str,
    instance_id: Option<&str>,
    action: RuntimeLifecycleAction,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(&context, runtime_id, instance_id);
    let (availability, view) = action_view(&target, action, None);
    Ok(RuntimeInspection { availability, view })
}

pub fn inspect_config_set(
    config: &RuntimeConfig,
    request: &RuntimeConfigMutationRequest,
) -> Result<RuntimeInspection<RuntimeActionView>, RuntimeError> {
    let context = load_management_context(config)?;
    let target = resolve_runtime_target(
        &context,
        request.runtime_id.as_str(),
        request.instance_id.as_deref(),
    );
    let detail = Some(format!(
        "requested managed config mutation {}={} for runtime `{}` instance `{}`; generic config mutation lands in {}",
        request.key, request.value, target.runtime_id, target.instance_id, DEFERRED_LIFECYCLE_SLICE
    ));
    let (availability, view) = action_view(&target, RuntimeLifecycleAction::ConfigSet, detail);
    Ok(RuntimeInspection { availability, view })
}

fn load_management_context(config: &RuntimeConfig) -> Result<RuntimeManagementContext, RuntimeError> {
    let contract = parse_contract_str(MANAGEMENT_CONTRACT_RAW)?;
    let profile = cli_path_profile(config)?;
    let overrides = cli_path_overrides(config)?;
    let resolver = RadrootsPathResolver::current();
    let mode_id = active_management_mode_for_profile(&contract, profile)?;
    let shared_paths = resolve_shared_paths(&contract, &resolver, profile, &overrides, mode_id)?;
    let registry = radroots_runtime_manager::load_registry(&shared_paths.instance_registry_path)?;
    Ok(RuntimeManagementContext {
        contract,
        shared_paths,
        registry,
    })
}

fn cli_path_profile(config: &RuntimeConfig) -> Result<RadrootsPathProfile, RuntimeError> {
    match config.paths.profile.as_str() {
        "interactive_user" => Ok(RadrootsPathProfile::InteractiveUser),
        "repo_local" => Ok(RadrootsPathProfile::RepoLocal),
        other => Err(RuntimeError::Config(format!(
            "runtime management only supports cli path profiles `interactive_user` and `repo_local`, got `{other}`"
        ))),
    }
}

fn cli_path_overrides(config: &RuntimeConfig) -> Result<RadrootsPathOverrides, RuntimeError> {
    match config.paths.profile.as_str() {
        "interactive_user" => Ok(RadrootsPathOverrides::default()),
        "repo_local" => {
            let Some(repo_local_root) = &config.paths.repo_local_root else {
                return Err(RuntimeError::Config(
                    "repo_local runtime management requires a repo-local root override".to_owned(),
                ));
            };
            Ok(RadrootsPathOverrides::repo_local(repo_local_root))
        }
        other => Err(RuntimeError::Config(format!(
            "runtime management only supports cli path profiles `interactive_user` and `repo_local`, got `{other}`"
        ))),
    }
}

fn active_management_mode_for_profile<'a>(
    contract: &'a RadrootsRuntimeManagementContract,
    profile: RadrootsPathProfile,
) -> Result<&'a str, RuntimeError> {
    let profile_id = profile.to_string();
    contract
        .mode
        .iter()
        .find(|(_, mode)| {
            mode.contract_state == "active"
                && mode.supported_profiles.iter().any(|entry| entry == &profile_id)
        })
        .map(|(mode_id, _)| mode_id.as_str())
        .ok_or_else(|| {
            RuntimeError::Config(format!(
                "no active runtime-management mode supports cli profile `{profile_id}`"
            ))
        })
}

fn resolve_runtime_target(
    context: &RuntimeManagementContext,
    runtime_id: &str,
    requested_instance_id: Option<&str>,
) -> RuntimeTarget {
    let runtime_group = runtime_group(&context.contract, runtime_id);
    let bootstrap = context.contract.bootstrap.get(runtime_id).cloned();
    let instance_id = requested_instance_id
        .map(ToOwned::to_owned)
        .or_else(|| bootstrap.as_ref().map(|entry| entry.default_instance_id.clone()))
        .unwrap_or_else(|| "default".to_owned());
    let instance_source = if requested_instance_id.is_some() {
        "command_arg".to_owned()
    } else if bootstrap.is_some() {
        "bootstrap_default".to_owned()
    } else {
        "implicit_default".to_owned()
    };
    let management_mode = bootstrap.as_ref().map(|entry| entry.management_mode.clone());
    let mode_contract = management_mode
        .as_ref()
        .and_then(|mode_id| context.contract.mode.get(mode_id).cloned());
    let instance_record = context
        .registry
        .instances
        .iter()
        .find(|record| record.runtime_id == runtime_id && record.instance_id == instance_id)
        .cloned();
    let predicted_paths = if runtime_group == RuntimeGroup::ActiveManagedTarget {
        Some(resolve_instance_paths(
            &context.shared_paths,
            runtime_id,
            instance_id.as_str(),
        ))
    } else {
        None
    };

    RuntimeTarget {
        runtime_id: runtime_id.to_owned(),
        instance_id,
        instance_source,
        runtime_group,
        management_mode,
        mode_contract,
        bootstrap,
        instance_record,
        predicted_paths,
        registry_path: context.shared_paths.instance_registry_path.clone(),
    }
}

fn runtime_group(contract: &RadrootsRuntimeManagementContract, runtime_id: &str) -> RuntimeGroup {
    if contract
        .managed_runtime_targets
        .active
        .iter()
        .any(|entry| entry == runtime_id)
    {
        RuntimeGroup::ActiveManagedTarget
    } else if contract
        .managed_runtime_targets
        .defined
        .iter()
        .any(|entry| entry == runtime_id)
    {
        RuntimeGroup::DefinedManagedTarget
    } else if contract
        .managed_runtime_targets
        .bootstrap_only
        .iter()
        .any(|entry| entry == runtime_id)
    {
        RuntimeGroup::BootstrapOnly
    } else {
        RuntimeGroup::Unknown
    }
}

fn status_view(target: &RuntimeTarget, lifecycle_actions: &[String]) -> RuntimeStatusView {
    let install_state = target
        .instance_record
        .as_ref()
        .map(|record| install_state_label(record.install_state))
        .unwrap_or_else(|| install_state_label(ManagedRuntimeInstallState::NotInstalled));
    let (health_state, health_source) = infer_health_state(target);

    RuntimeStatusView {
        runtime_id: target.runtime_id.clone(),
        instance_id: target.instance_id.clone(),
        instance_source: target.instance_source.clone(),
        runtime_group: target.runtime_group.as_str().to_owned(),
        management_posture: target.runtime_group.posture().to_owned(),
        state: status_state(target).to_owned(),
        source: "runtime management contract + shared instance registry".to_owned(),
        detail: status_detail(target),
        management_mode: target.management_mode.clone(),
        service_manager_integration: target
            .mode_contract
            .as_ref()
            .map(|mode| mode.service_manager_integration),
        uses_absolute_binary_paths: target
            .mode_contract
            .as_ref()
            .map(|mode| mode.uses_absolute_binary_paths),
        preferred_cli_binding: target.bootstrap.as_ref().map(|entry| entry.preferred_cli_binding),
        install_state: install_state.to_owned(),
        health_state: health_state.to_owned(),
        health_source: health_source.to_owned(),
        registry_path: target.registry_path.display().to_string(),
        lifecycle_actions: if target.runtime_group == RuntimeGroup::ActiveManagedTarget {
            lifecycle_actions.to_vec()
        } else {
            Vec::new()
        },
        instance_paths: target.predicted_paths.as_ref().map(instance_paths_view),
        instance_record: target.instance_record.as_ref().map(instance_record_view),
    }
}

fn logs_view(target: &RuntimeTarget) -> (RuntimeCommandAvailability, RuntimeLogsView) {
    let stdout_log_path = target
        .predicted_paths
        .as_ref()
        .map(|paths| paths.stdout_log_path.display().to_string());
    let stderr_log_path = target
        .predicted_paths
        .as_ref()
        .map(|paths| paths.stderr_log_path.display().to_string());
    let availability = match target.runtime_group {
        RuntimeGroup::Unknown => RuntimeCommandAvailability::Unconfigured,
        RuntimeGroup::ActiveManagedTarget => RuntimeCommandAvailability::Success,
        RuntimeGroup::DefinedManagedTarget | RuntimeGroup::BootstrapOnly => {
            if target.instance_record.is_some() {
                RuntimeCommandAvailability::Success
            } else {
                RuntimeCommandAvailability::Unsupported
            }
        }
    };
    let detail = match target.runtime_group {
        RuntimeGroup::ActiveManagedTarget => {
            "runtime logs report the managed stdout/stderr locations; lifecycle execution lands in rpv1-rpi.5"
                .to_owned()
        }
        RuntimeGroup::DefinedManagedTarget => format!(
            "runtime `{}` is only a defined future managed target; no active generic logs surface exists without a registered instance",
            target.runtime_id
        ),
        RuntimeGroup::BootstrapOnly => format!(
            "runtime `{}` remains bootstrap_only and direct-bindable in this wave; generic managed logs are not admitted",
            target.runtime_id
        ),
        RuntimeGroup::Unknown => unknown_runtime_detail(target),
    };

    (
        availability,
        RuntimeLogsView {
            runtime_id: target.runtime_id.clone(),
            instance_id: target.instance_id.clone(),
            instance_source: target.instance_source.clone(),
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: match availability {
                RuntimeCommandAvailability::Success => "ready".to_owned(),
                RuntimeCommandAvailability::Unconfigured => "unknown_runtime".to_owned(),
                RuntimeCommandAvailability::Unsupported => "unsupported".to_owned(),
            },
            source: "runtime management contract + shared instance registry".to_owned(),
            detail,
            stdout_log_path: stdout_log_path.clone().or_else(|| {
                target
                    .instance_record
                    .as_ref()
                    .map(|record| record.logs_path.join("stdout.log").display().to_string())
            }),
            stderr_log_path: stderr_log_path.clone().or_else(|| {
                target
                    .instance_record
                    .as_ref()
                    .map(|record| record.logs_path.join("stderr.log").display().to_string())
            }),
            stdout_log_present: path_present(stdout_log_path.as_deref()).unwrap_or_else(|| {
                target
                    .instance_record
                    .as_ref()
                    .is_some_and(|record| record.logs_path.join("stdout.log").exists())
            }),
            stderr_log_present: path_present(stderr_log_path.as_deref()).unwrap_or_else(|| {
                target
                    .instance_record
                    .as_ref()
                    .is_some_and(|record| record.logs_path.join("stderr.log").exists())
            }),
        },
    )
}

fn config_show_view(
    target: &RuntimeTarget,
) -> (RuntimeCommandAvailability, RuntimeManagedConfigView) {
    let availability = match target.runtime_group {
        RuntimeGroup::Unknown => RuntimeCommandAvailability::Unconfigured,
        RuntimeGroup::ActiveManagedTarget => RuntimeCommandAvailability::Success,
        RuntimeGroup::DefinedManagedTarget | RuntimeGroup::BootstrapOnly => {
            if target.instance_record.is_some() {
                RuntimeCommandAvailability::Success
            } else {
                RuntimeCommandAvailability::Unsupported
            }
        }
    };
    let config_path = target
        .instance_record
        .as_ref()
        .map(|record| record.config_path.display().to_string());
    let detail = match target.runtime_group {
        RuntimeGroup::ActiveManagedTarget => {
            if config_path.is_some() {
                "runtime config show reports the managed config location without mutating bindings or lifecycle state"
                    .to_owned()
            } else {
                format!(
                    "managed runtime `{}` has no registered instance config yet; config bootstrap lands in {}",
                    target.runtime_id, DEFERRED_LIFECYCLE_SLICE
                )
            }
        }
        RuntimeGroup::DefinedManagedTarget => format!(
            "runtime `{}` is only a defined future managed target; generic config surfaces are not admitted without a registered instance",
            target.runtime_id
        ),
        RuntimeGroup::BootstrapOnly => format!(
            "runtime `{}` remains bootstrap_only and direct-bindable in this wave; generic managed config is not admitted",
            target.runtime_id
        ),
        RuntimeGroup::Unknown => unknown_runtime_detail(target),
    };

    (
        availability,
        RuntimeManagedConfigView {
            runtime_id: target.runtime_id.clone(),
            instance_id: target.instance_id.clone(),
            instance_source: target.instance_source.clone(),
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: match availability {
                RuntimeCommandAvailability::Success => {
                    if config_path.is_some() {
                        "ready".to_owned()
                    } else {
                        "not_installed".to_owned()
                    }
                }
                RuntimeCommandAvailability::Unconfigured => "unknown_runtime".to_owned(),
                RuntimeCommandAvailability::Unsupported => "unsupported".to_owned(),
            },
            source: "runtime management contract + shared instance registry".to_owned(),
            detail,
            config_format: target.bootstrap.as_ref().map(|entry| entry.config_format.clone()),
            config_path: config_path.clone(),
            config_present: config_path
                .as_deref()
                .is_some_and(|path| PathBuf::from(path).exists()),
            requires_bootstrap_secret: target
                .bootstrap
                .as_ref()
                .map(|entry| entry.requires_bootstrap_secret),
            requires_config_bootstrap: target
                .bootstrap
                .as_ref()
                .map(|entry| entry.requires_config_bootstrap),
            requires_signer_provider: target
                .bootstrap
                .as_ref()
                .map(|entry| entry.requires_signer_provider),
        },
    )
}

fn action_view(
    target: &RuntimeTarget,
    action: RuntimeLifecycleAction,
    detail_override: Option<String>,
) -> (RuntimeCommandAvailability, RuntimeActionView) {
    let (availability, state, detail, next_step) = match target.runtime_group {
        RuntimeGroup::ActiveManagedTarget => (
            RuntimeCommandAvailability::Unsupported,
            "deferred",
            detail_override.unwrap_or_else(|| {
                format!(
                    "runtime {} `{}` is reserved for {} so lifecycle execution can land after the generic command family is stable",
                    action.as_str().replace('_', " "),
                    target.runtime_id,
                    DEFERRED_LIFECYCLE_SLICE
                )
            }),
            Some(DEFERRED_LIFECYCLE_SLICE.to_owned()),
        ),
        RuntimeGroup::DefinedManagedTarget => (
            RuntimeCommandAvailability::Unsupported,
            "unsupported",
            detail_override.unwrap_or_else(|| {
                format!(
                    "runtime `{}` is only a defined future managed target; `{}` is not admitted in the current wave",
                    target.runtime_id,
                    action.as_str().replace('_', " ")
                )
            }),
            None,
        ),
        RuntimeGroup::BootstrapOnly => (
            RuntimeCommandAvailability::Unsupported,
            "unsupported",
            detail_override.unwrap_or_else(|| {
                format!(
                    "runtime `{}` remains bootstrap_only and direct-bindable in this wave; generic managed `{}` is not admitted",
                    target.runtime_id,
                    action.as_str().replace('_', " ")
                )
            }),
            None,
        ),
        RuntimeGroup::Unknown => (
            RuntimeCommandAvailability::Unconfigured,
            "unknown_runtime",
            detail_override.unwrap_or_else(|| unknown_runtime_detail(target)),
            None,
        ),
    };

    (
        availability,
        RuntimeActionView {
            action: action.as_str().to_owned(),
            runtime_id: target.runtime_id.clone(),
            instance_id: target.instance_id.clone(),
            instance_source: target.instance_source.clone(),
            runtime_group: target.runtime_group.as_str().to_owned(),
            state: state.to_owned(),
            source: "generic runtime-management command family".to_owned(),
            detail,
            mutates_bindings: false,
            next_step,
        },
    )
}

fn status_state(target: &RuntimeTarget) -> &'static str {
    match target.runtime_group {
        RuntimeGroup::ActiveManagedTarget => match target.instance_record.as_ref() {
            Some(record) => install_state_label(record.install_state),
            None => "not_installed",
        },
        RuntimeGroup::DefinedManagedTarget => "defined_not_active",
        RuntimeGroup::BootstrapOnly => "bootstrap_only",
        RuntimeGroup::Unknown => "unknown_runtime",
    }
}

fn status_detail(target: &RuntimeTarget) -> String {
    match target.runtime_group {
        RuntimeGroup::ActiveManagedTarget => match &target.instance_record {
            Some(record) => format!(
                "managed runtime `{}` instance `{}` is registered with config at {}; generic lifecycle execution lands in {}",
                target.runtime_id,
                target.instance_id,
                record.config_path.display(),
                DEFERRED_LIFECYCLE_SLICE
            ),
            None => format!(
                "managed runtime `{}` has no registered instance `{}` in {}; lifecycle bootstrap lands in {}",
                target.runtime_id,
                target.instance_id,
                target.registry_path.display(),
                DEFERRED_LIFECYCLE_SLICE
            ),
        },
        RuntimeGroup::DefinedManagedTarget => format!(
            "runtime `{}` is defined in the management contract but not yet admitted as an active managed target",
            target.runtime_id
        ),
        RuntimeGroup::BootstrapOnly => format!(
            "runtime `{}` is bootstrap_only in the management contract and remains direct-bindable outside managed lifecycle in this wave",
            target.runtime_id
        ),
        RuntimeGroup::Unknown => unknown_runtime_detail(target),
    }
}

fn unknown_runtime_detail(target: &RuntimeTarget) -> String {
    format!(
        "runtime `{}` is not present in the current runtime-management contract",
        target.runtime_id
    )
}

fn infer_health_state(target: &RuntimeTarget) -> (&'static str, &'static str) {
    let Some(record) = &target.instance_record else {
        return (
            health_state_label(ManagedRuntimeHealthState::NotInstalled),
            "registry_absent",
        );
    };
    if record.install_state == ManagedRuntimeInstallState::Failed {
        return (
            health_state_label(ManagedRuntimeHealthState::Failed),
            "registry_install_state",
        );
    }

    let pid_path = target
        .predicted_paths
        .as_ref()
        .map(|paths| paths.pid_file_path.clone())
        .unwrap_or_else(|| record.run_path.join("runtime.pid"));

    if pid_path.exists() {
        return (
            health_state_label(ManagedRuntimeHealthState::Running),
            "pid_file_presence",
        );
    }

    match record.install_state {
        ManagedRuntimeInstallState::NotInstalled => (
            health_state_label(ManagedRuntimeHealthState::NotInstalled),
            "registry_install_state",
        ),
        ManagedRuntimeInstallState::Installed | ManagedRuntimeInstallState::Configured => (
            health_state_label(ManagedRuntimeHealthState::Stopped),
            "pid_file_absent",
        ),
        ManagedRuntimeInstallState::Failed => (
            health_state_label(ManagedRuntimeHealthState::Failed),
            "registry_install_state",
        ),
    }
}

fn install_state_label(state: ManagedRuntimeInstallState) -> &'static str {
    match state {
        ManagedRuntimeInstallState::NotInstalled => "not_installed",
        ManagedRuntimeInstallState::Installed => "installed",
        ManagedRuntimeInstallState::Configured => "configured",
        ManagedRuntimeInstallState::Failed => "failed",
    }
}

fn health_state_label(state: ManagedRuntimeHealthState) -> &'static str {
    match state {
        ManagedRuntimeHealthState::NotInstalled => "not_installed",
        ManagedRuntimeHealthState::Stopped => "stopped",
        ManagedRuntimeHealthState::Starting => "starting",
        ManagedRuntimeHealthState::Running => "running",
        ManagedRuntimeHealthState::Degraded => "degraded",
        ManagedRuntimeHealthState::Failed => "failed",
    }
}

fn instance_paths_view(
    paths: &radroots_runtime_manager::ManagedRuntimeInstancePaths,
) -> RuntimeInstancePathsView {
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

fn instance_record_view(record: &ManagedRuntimeInstanceRecord) -> RuntimeInstanceRecordView {
    RuntimeInstanceRecordView {
        management_mode: record.management_mode.clone(),
        install_state: install_state_label(record.install_state).to_owned(),
        binary_path: record.binary_path.display().to_string(),
        config_path: record.config_path.display().to_string(),
        logs_path: record.logs_path.display().to_string(),
        run_path: record.run_path.display().to_string(),
        installed_version: record.installed_version.clone(),
        health_endpoint: record.health_endpoint.clone(),
        secret_material_ref: record.secret_material_ref.clone(),
        last_started_at: record.last_started_at.clone(),
        last_stopped_at: record.last_stopped_at.clone(),
        notes: record.notes.clone(),
    }
}

fn path_present(path: Option<&str>) -> Option<bool> {
    path.map(|value| PathBuf::from(value).exists())
}
