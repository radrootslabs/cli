use crate::domain::runtime::{
    AccountRuntimeView, AccountSecretRuntimeView, CapabilityBindingRuntimeView,
    CommandOutput, CommandView, ConfigFilesRuntimeView, ConfigShowView, HyfProviderRuntimeView,
    HyfRuntimeView, LegacyPathRuntimeView, LocalRuntimeView, LoggingRuntimeView,
    MigrationRuntimeView, MycRuntimeView, OutputRuntimeView, PathsRuntimeView, RelayRuntimeView,
    ResolvedProviderRuntimeView, RpcRuntimeView, SignerRuntimeView, WorkflowRuntimeView,
    WritePlaneRuntimeView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;
use crate::runtime::management::{
    RuntimeCommandAvailability, RuntimeConfigMutationRequest, RuntimeLifecycleAction,
    inspect_action, inspect_config_set, inspect_config_show, inspect_logs, inspect_status,
};
use crate::runtime::provider::{
    resolve_capability_providers, resolve_hyf_provider, resolve_workflow_provider,
    resolve_write_plane_provider,
};
use crate::cli::{RuntimeConfigSetArgs, RuntimeTargetArgs};

pub fn show(
    config: &RuntimeConfig,
    logging: &LoggingState,
) -> Result<ConfigShowView, RuntimeError> {
    let secret_backend = crate::runtime::accounts::secret_backend_status(config);
    let write_plane = resolve_write_plane_provider(config);
    let workflow = resolve_workflow_provider(config);
    let hyf_provider = resolve_hyf_provider(config);
    let resolved_providers = resolve_capability_providers(config);
    Ok(ConfigShowView {
        source: "local runtime state".to_owned(),
        output: OutputRuntimeView {
            format: config.output.format.as_str().to_owned(),
            verbosity: config.output.verbosity.as_str().to_owned(),
            color: config.output.color,
            dry_run: config.output.dry_run,
        },
        config_files: ConfigFilesRuntimeView {
            user_present: config.paths.app_config_path.exists(),
            workspace_present: config.paths.workspace_config_path.exists(),
        },
        paths: PathsRuntimeView {
            profile: config.paths.profile.clone(),
            profile_source: config.paths.profile_source.clone(),
            allowed_profiles: config.paths.allowed_profiles.clone(),
            root_source: config.paths.root_source.clone(),
            repo_local_root: config
                .paths
                .repo_local_root
                .as_ref()
                .map(|path| path.display().to_string()),
            repo_local_root_source: config.paths.repo_local_root_source.clone(),
            subordinate_path_override_source: config.paths.subordinate_path_override_source.clone(),
            app_namespace: config.paths.app_namespace.clone(),
            shared_accounts_namespace: config.paths.shared_accounts_namespace.clone(),
            shared_identities_namespace: config.paths.shared_identities_namespace.clone(),
            app_config_path: config.paths.app_config_path.display().to_string(),
            workspace_config_path: config.paths.workspace_config_path.display().to_string(),
            app_data_root: config.paths.app_data_root.display().to_string(),
            app_logs_root: config.paths.app_logs_root.display().to_string(),
            shared_accounts_data_root: config.paths.shared_accounts_data_root.display().to_string(),
            shared_accounts_secrets_root: config
                .paths
                .shared_accounts_secrets_root
                .display()
                .to_string(),
            default_identity_path: config.paths.default_identity_path.display().to_string(),
        },
        migration: migration_runtime_view(config),
        logging: LoggingRuntimeView {
            initialized: logging.initialized,
            filter: config.logging.filter.clone(),
            stdout: config.logging.stdout,
            directory: config
                .logging
                .directory
                .as_ref()
                .map(|path| path.display().to_string()),
            current_file: logging
                .current_file
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        account: AccountRuntimeView {
            selector: config.account.selector.clone(),
            store_path: config.account.store_path.display().to_string(),
            secrets_dir: config.account.secrets_dir.display().to_string(),
            identity_path: config.identity.path.display().to_string(),
            secret_backend: AccountSecretRuntimeView {
                contract_default_backend: config.account_secret_contract.default_backend.clone(),
                contract_default_fallback: config.account_secret_contract.default_fallback.clone(),
                allowed_backends: config.account_secret_contract.allowed_backends.clone(),
                host_vault_policy: config.account_secret_contract.host_vault_policy.clone(),
                uses_protected_store: config.account_secret_contract.uses_protected_store,
                configured_primary: secret_backend.configured_primary,
                configured_fallback: secret_backend.configured_fallback,
                state: secret_backend.state,
                active_backend: secret_backend.active_backend,
                used_fallback: secret_backend.used_fallback,
                reason: secret_backend.reason,
            },
        },
        signer: SignerRuntimeView {
            mode: config.signer.backend.as_str().to_owned(),
        },
        relay: RelayRuntimeView {
            count: config.relay.urls.len(),
            urls: config.relay.urls.clone(),
            publish_policy: config.relay.publish_policy.as_str().to_owned(),
            source: config.relay.source.as_str().to_owned(),
        },
        local: LocalRuntimeView {
            root: config.local.root.display().to_string(),
            replica_db_path: config.local.replica_db_path.display().to_string(),
            backups_dir: config.local.backups_dir.display().to_string(),
            exports_dir: config.local.exports_dir.display().to_string(),
        },
        myc: MycRuntimeView {
            executable: config.myc.executable.display().to_string(),
        },
        write_plane: WritePlaneRuntimeView {
            provider_runtime_id: write_plane.provider_runtime_id,
            binding_model: write_plane.binding_model,
            state: write_plane.state,
            provenance: write_plane.provenance,
            source: write_plane.source,
            target_kind: write_plane.target_kind,
            target: write_plane.target,
            detail: write_plane.detail,
            bridge_auth_configured: write_plane.bridge_auth_configured,
        },
        workflow: WorkflowRuntimeView {
            provider_runtime_id: workflow.provider_runtime_id,
            binding_model: workflow.binding_model,
            state: workflow.state,
            provenance: workflow.provenance,
            source: workflow.source,
            target_kind: workflow.target_kind,
            target: workflow.target,
            hyf_helper_state: workflow.hyf_helper_state,
            hyf_helper_detail: workflow.hyf_helper_detail,
        },
        hyf_provider: HyfProviderRuntimeView {
            provider_runtime_id: hyf_provider.provider_runtime_id,
            binding_model: hyf_provider.binding_model,
            state: hyf_provider.state,
            provenance: hyf_provider.provenance,
            source: hyf_provider.source,
            target_kind: hyf_provider.target_kind,
            target: hyf_provider.target,
            executable: hyf_provider.executable,
            reason: hyf_provider.reason,
            protocol_version: hyf_provider.protocol_version,
            deterministic_available: hyf_provider.deterministic_available,
        },
        hyf: HyfRuntimeView {
            enabled: config.hyf.enabled,
            executable: config.hyf.executable.display().to_string(),
        },
        rpc: RpcRuntimeView {
            url: config.rpc.url.clone(),
            bridge_auth_configured: config.rpc.bridge_bearer_token.is_some(),
        },
        resolved_providers: resolved_providers
            .into_iter()
            .map(|provider| ResolvedProviderRuntimeView {
                capability_id: provider.capability_id,
                provider_runtime_id: provider.provider_runtime_id,
                binding_model: provider.binding_model,
                state: provider.state,
                provenance: provider.provenance,
                source: provider.source,
                target_kind: provider.target_kind,
                target: provider.target,
            })
            .collect(),
        capability_bindings: config
            .inspect_capability_bindings()
            .into_iter()
            .map(|binding| CapabilityBindingRuntimeView {
                capability_id: binding.capability_id,
                provider_runtime_id: binding.provider_runtime_id,
                binding_model: binding.binding_model,
                state: binding.state.as_str().to_owned(),
                source: binding.source,
                target_kind: binding.target_kind,
                target: binding.target,
                managed_account_ref: binding.managed_account_ref,
                signer_session_ref: binding.signer_session_ref,
            })
            .collect(),
    })
}

pub fn status(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_status(config, args.runtime.as_str(), args.instance.as_deref())?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeStatus(inspection.view),
    ))
}

pub fn install(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_action(
        config,
        args.runtime.as_str(),
        args.instance.as_deref(),
        RuntimeLifecycleAction::Install,
    )?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeAction(inspection.view),
    ))
}

pub fn uninstall(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_action(
        config,
        args.runtime.as_str(),
        args.instance.as_deref(),
        RuntimeLifecycleAction::Uninstall,
    )?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeAction(inspection.view),
    ))
}

pub fn start(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_action(
        config,
        args.runtime.as_str(),
        args.instance.as_deref(),
        RuntimeLifecycleAction::Start,
    )?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeAction(inspection.view),
    ))
}

pub fn stop(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_action(
        config,
        args.runtime.as_str(),
        args.instance.as_deref(),
        RuntimeLifecycleAction::Stop,
    )?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeAction(inspection.view),
    ))
}

pub fn restart(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_action(
        config,
        args.runtime.as_str(),
        args.instance.as_deref(),
        RuntimeLifecycleAction::Restart,
    )?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeAction(inspection.view),
    ))
}

pub fn logs(
    config: &RuntimeConfig,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_logs(config, args.runtime.as_str(), args.instance.as_deref())?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeLogs(inspection.view),
    ))
}

pub fn config_show(
    config: &RuntimeConfig,
    _logging: &LoggingState,
    args: &RuntimeTargetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection =
        inspect_config_show(config, args.runtime.as_str(), args.instance.as_deref())?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeConfigShow(inspection.view),
    ))
}

pub fn config_set(
    config: &RuntimeConfig,
    args: &RuntimeConfigSetArgs,
) -> Result<CommandOutput, RuntimeError> {
    let inspection = inspect_config_set(
        config,
        &RuntimeConfigMutationRequest {
            runtime_id: args.target.runtime.clone(),
            instance_id: args.target.instance.clone(),
            key: args.key.clone(),
            value: args.value.clone(),
        },
    )?;
    Ok(command_output(
        inspection.availability,
        CommandView::RuntimeAction(inspection.view),
    ))
}

fn migration_runtime_view(config: &RuntimeConfig) -> MigrationRuntimeView {
    let report = &config.migration.report;
    let detected_legacy_paths = report
        .detected_legacy_paths
        .iter()
        .map(|path| LegacyPathRuntimeView {
            id: path.id.clone(),
            description: path.description.clone(),
            path: path.path.display().to_string(),
            destination: path
                .destination
                .as_ref()
                .map(|destination| destination.display().to_string()),
            import_hint: path.import_hint.clone(),
        })
        .collect::<Vec<_>>();
    let actions = if detected_legacy_paths.is_empty() {
        Vec::new()
    } else {
        vec![
            "inspect detected_legacy_paths before writing new local state".to_owned(),
            "perform an explicit export/import or manual copy; startup did not move legacy data"
                .to_owned(),
        ]
    };
    MigrationRuntimeView {
        posture: report.posture.to_owned(),
        state: report.state.to_owned(),
        silent_startup_relocation: report.silent_startup_relocation,
        compatibility_window: report.compatibility_window.to_owned(),
        detected_legacy_paths,
        actions,
    }
}

fn command_output(availability: RuntimeCommandAvailability, view: CommandView) -> CommandOutput {
    match availability {
        RuntimeCommandAvailability::Success => CommandOutput::success(view),
        RuntimeCommandAvailability::Unconfigured => CommandOutput::unconfigured(view),
        RuntimeCommandAvailability::Unsupported => CommandOutput::unsupported(view),
    }
}
