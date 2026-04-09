use std::path::{Path, PathBuf};

use radroots_runtime_paths::{
    DEFAULT_CONFIG_FILE_NAME, RadrootsPathOverrides, RadrootsPathProfile, RadrootsRuntimeNamespace,
    default_shared_identity_path,
};

use crate::runtime::{
    RuntimeError,
    config::{EnvFileValues, Environment},
};

const DEFAULT_WORKSPACE_CONFIG_PATH: &str = ".radroots/config.toml";
const CLI_DEFAULT_PROFILE: &str = "interactive_user";
const CLI_REPO_LOCAL_PROFILE: &str = "repo_local";
const CLI_APP_NAMESPACE_VALUE: &str = "cli";
const SHARED_ACCOUNTS_NAMESPACE_VALUE: &str = "accounts";
const SHARED_IDENTITIES_NAMESPACE_VALUE: &str = "identities";

pub(crate) const CLI_ALLOWED_PROFILES: &[&str] = &[CLI_DEFAULT_PROFILE, CLI_REPO_LOCAL_PROFILE];
pub(crate) const ENV_CLI_PATHS_PROFILE: &str = "RADROOTS_CLI_PATHS_PROFILE";
pub(crate) const ENV_CLI_PATHS_REPO_LOCAL_ROOT: &str = "RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathsConfig {
    pub profile: String,
    pub allowed_profiles: Vec<String>,
    pub app_namespace: String,
    pub shared_accounts_namespace: String,
    pub shared_identities_namespace: String,
    pub app_config_path: PathBuf,
    pub workspace_config_path: PathBuf,
    pub app_data_root: PathBuf,
    pub app_logs_root: PathBuf,
    pub shared_accounts_data_root: PathBuf,
    pub shared_accounts_secrets_root: PathBuf,
    pub default_identity_path: PathBuf,
}

pub(crate) fn resolve_paths(
    env: &dyn Environment,
    env_file: &EnvFileValues,
) -> Result<PathsConfig, RuntimeError> {
    let current_dir = env.current_dir()?;
    let resolver = env.path_resolver();
    let (profile, profile_label) = resolve_cli_path_profile(env, env_file)?;
    let overrides = resolve_cli_path_overrides(current_dir.as_path(), env, env_file, profile)?;
    let resolved = resolver
        .resolve(profile, &overrides)
        .map_err(|err| RuntimeError::Config(format!("resolve Radroots path roots: {err}")))?;
    let app_namespace = RadrootsRuntimeNamespace::app(CLI_APP_NAMESPACE_VALUE)
        .map_err(|err| RuntimeError::Config(format!("resolve cli namespace: {err}")))?;
    let shared_accounts_namespace =
        RadrootsRuntimeNamespace::shared(SHARED_ACCOUNTS_NAMESPACE_VALUE).map_err(|err| {
            RuntimeError::Config(format!("resolve shared accounts namespace: {err}"))
        })?;
    let shared_identity_namespace =
        RadrootsRuntimeNamespace::shared(SHARED_IDENTITIES_NAMESPACE_VALUE).map_err(|err| {
            RuntimeError::Config(format!("resolve shared identities namespace: {err}"))
        })?;
    let app_paths = resolved.namespaced(&app_namespace);
    let shared_accounts_paths = resolved.namespaced(&shared_accounts_namespace);
    let default_identity_path = default_shared_identity_path(&resolver, profile, &overrides)
        .map_err(|err| RuntimeError::Config(format!("resolve shared identity path: {err}")))?;

    Ok(PathsConfig {
        profile: profile_label.to_owned(),
        allowed_profiles: CLI_ALLOWED_PROFILES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        app_namespace: app_namespace.relative_path().display().to_string(),
        shared_accounts_namespace: shared_accounts_namespace
            .relative_path()
            .display()
            .to_string(),
        shared_identities_namespace: shared_identity_namespace
            .relative_path()
            .display()
            .to_string(),
        app_config_path: app_paths.config.join(DEFAULT_CONFIG_FILE_NAME),
        workspace_config_path: current_dir.join(DEFAULT_WORKSPACE_CONFIG_PATH),
        app_data_root: app_paths.data,
        app_logs_root: app_paths.logs,
        shared_accounts_data_root: shared_accounts_paths.data,
        shared_accounts_secrets_root: shared_accounts_paths.secrets,
        default_identity_path,
    })
}

fn resolve_cli_path_profile(
    env: &dyn Environment,
    env_file: &EnvFileValues,
) -> Result<(RadrootsPathProfile, &'static str), RuntimeError> {
    match path_env_value_entry(env, env_file, &[ENV_CLI_PATHS_PROFILE]) {
        Some((key, value)) => parse_cli_path_profile(key.as_str(), value.as_str()),
        None => Ok((RadrootsPathProfile::InteractiveUser, CLI_DEFAULT_PROFILE)),
    }
}

fn parse_cli_path_profile(
    key: &str,
    value: &str,
) -> Result<(RadrootsPathProfile, &'static str), RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        CLI_DEFAULT_PROFILE => Ok((RadrootsPathProfile::InteractiveUser, CLI_DEFAULT_PROFILE)),
        CLI_REPO_LOCAL_PROFILE => Ok((RadrootsPathProfile::RepoLocal, CLI_REPO_LOCAL_PROFILE)),
        other => Err(RuntimeError::Config(format!(
            "{key} must be `interactive_user` or `repo_local`, got `{other}`"
        ))),
    }
}

fn resolve_cli_path_overrides(
    current_dir: &Path,
    env: &dyn Environment,
    env_file: &EnvFileValues,
    profile: RadrootsPathProfile,
) -> Result<RadrootsPathOverrides, RuntimeError> {
    match profile {
        RadrootsPathProfile::InteractiveUser => Ok(RadrootsPathOverrides::default()),
        RadrootsPathProfile::RepoLocal => {
            let Some((key, value)) =
                path_env_value_entry(env, env_file, &[ENV_CLI_PATHS_REPO_LOCAL_ROOT])
            else {
                return Err(RuntimeError::Config(format!(
                    "{ENV_CLI_PATHS_REPO_LOCAL_ROOT} must be set when {ENV_CLI_PATHS_PROFILE}=repo_local"
                )));
            };
            if value.trim().is_empty() {
                return Err(RuntimeError::Config(format!(
                    "{key} must not be empty when {ENV_CLI_PATHS_PROFILE}=repo_local"
                )));
            }
            Ok(RadrootsPathOverrides::repo_local(
                normalize_explicit_path_root(current_dir, value.as_str()),
            ))
        }
        _ => Err(RuntimeError::Config(
            "cli only supports interactive_user and repo_local path profiles".to_owned(),
        )),
    }
}

fn normalize_explicit_path_root(current_dir: &Path, value: &str) -> PathBuf {
    let root = PathBuf::from(value.trim());
    if root.is_absolute() {
        root
    } else {
        current_dir.join(root)
    }
}

fn path_env_value_entry(
    env: &dyn Environment,
    env_file: &EnvFileValues,
    keys: &[&str],
) -> Option<(String, String)> {
    keys.iter()
        .find_map(|key| env.var(key).map(|value| ((*key).to_owned(), value)))
        .or_else(|| {
            keys.iter().find_map(|key| {
                env_file
                    .get(key)
                    .map(|value| ((*key).to_owned(), value.to_owned()))
            })
        })
}
