use std::collections::BTreeMap;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;

use radroots_runtime_paths::{
    RadrootsLegacyPathCandidate, RadrootsMigrationReport, RadrootsPathResolver,
    inspect_legacy_paths,
};
use radroots_secret_vault::{RadrootsHostVaultPolicy, RadrootsSecretBackend};
use serde::Deserialize;
use url::Url;

use crate::cli::CliArgs;
use crate::runtime::RuntimeError;
pub use crate::runtime::paths::PathsConfig;
use crate::runtime::paths::{ENV_CLI_PATHS_PROFILE, ENV_CLI_PATHS_REPO_LOCAL_ROOT, resolve_paths};

const DEFAULT_LOG_FILTER: &str = "info";
const DEFAULT_ENV_PATH: &str = ".env";
const DEFAULT_LOCAL_STATE_DIR: &str = "replica";
const DEFAULT_LOCAL_DB_FILE: &str = "replica.sqlite";
const DEFAULT_LOCAL_BACKUPS_DIR: &str = "backups";
const DEFAULT_LOCAL_EXPORTS_DIR: &str = "exports";
const DEFAULT_SHARED_ACCOUNTS_STORE_FILE: &str = "store.json";
const DEFAULT_HYF_EXECUTABLE: &str = "hyfd";
const DEFAULT_RPC_URL: &str = "http://127.0.0.1:7070";
const CLI_HOST_VAULT_POLICY: &str = "desktop";
const CLI_DEFAULT_SECRET_BACKEND: &str = "host_vault";
const CLI_DEFAULT_SECRET_FALLBACK: &str = "encrypted_file";
const CLI_ALLOWED_SHARED_SECRET_BACKENDS: &[&str] = &["host_vault", "encrypted_file", "memory"];
const CLI_USES_PROTECTED_STORE: bool = true;
const ENV_FILE_PATH: &str = "RADROOTS_ENV_FILE";
const ENV_OUTPUT: &str = "RADROOTS_OUTPUT";
const ENV_CLI_LOG_FILTER: &str = "RADROOTS_CLI_LOGGING_FILTER";
const ENV_CLI_LOG_DIR: &str = "RADROOTS_CLI_LOGGING_OUTPUT_DIR";
const ENV_CLI_LOG_STDOUT: &str = "RADROOTS_CLI_LOGGING_STDOUT";
const ENV_LOG_FILTER: &str = "RADROOTS_LOG_FILTER";
const ENV_LOG_DIR: &str = "RADROOTS_LOG_DIR";
const ENV_LOG_STDOUT: &str = "RADROOTS_LOG_STDOUT";
const ENV_ACCOUNT: &str = "RADROOTS_ACCOUNT";
const ENV_ACCOUNT_SECRET_BACKEND: &str = "RADROOTS_ACCOUNT_SECRET_BACKEND";
const ENV_ACCOUNT_SECRET_FALLBACK: &str = "RADROOTS_ACCOUNT_SECRET_FALLBACK";
const ENV_IDENTITY_PATH: &str = "RADROOTS_IDENTITY_PATH";
const ENV_SIGNER: &str = "RADROOTS_SIGNER";
const ENV_RELAYS: &str = "RADROOTS_RELAYS";
const ENV_MYC_EXECUTABLE: &str = "RADROOTS_MYC_EXECUTABLE";
const ENV_HYF_ENABLED: &str = "RADROOTS_HYF_ENABLED";
const ENV_HYF_EXECUTABLE: &str = "RADROOTS_HYF_EXECUTABLE";
const ENV_RPC_URL: &str = "RADROOTS_RPC_URL";
const ENV_RPC_BEARER_TOKEN: &str = "RADROOTS_RPC_BEARER_TOKEN";
const SUPPORTED_ENV_FILE_KEYS: &[&str] = &[
    ENV_OUTPUT,
    ENV_CLI_LOG_FILTER,
    ENV_CLI_LOG_DIR,
    ENV_CLI_LOG_STDOUT,
    ENV_CLI_PATHS_PROFILE,
    ENV_CLI_PATHS_REPO_LOCAL_ROOT,
    ENV_LOG_FILTER,
    ENV_LOG_DIR,
    ENV_LOG_STDOUT,
    ENV_ACCOUNT,
    ENV_ACCOUNT_SECRET_BACKEND,
    ENV_ACCOUNT_SECRET_FALLBACK,
    ENV_IDENTITY_PATH,
    ENV_SIGNER,
    ENV_RELAYS,
    ENV_MYC_EXECUTABLE,
    ENV_HYF_ENABLED,
    ENV_HYF_EXECUTABLE,
    ENV_RPC_URL,
    ENV_RPC_BEARER_TOKEN,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Ndjson,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Json => "json",
            Self::Ndjson => "ndjson",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
    Trace,
}

impl Verbosity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Normal => "normal",
            Self::Verbose => "verbose",
            Self::Trace => "trace",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub verbosity: Verbosity,
    pub color: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionConfig {
    pub input_enabled: bool,
    pub assume_yes: bool,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub prompts_allowed: bool,
    pub confirmations_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    pub filter: String,
    pub directory: Option<PathBuf>,
    pub stdout: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityConfig {
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountConfig {
    pub selector: Option<String>,
    pub store_path: PathBuf,
    pub secrets_dir: PathBuf,
    pub secret_backend: RadrootsSecretBackend,
    pub secret_fallback: Option<RadrootsSecretBackend>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSecretContractConfig {
    pub default_backend: String,
    pub default_fallback: Option<String>,
    pub allowed_backends: Vec<String>,
    pub host_vault_policy: Option<String>,
    pub uses_protected_store: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignerBackend {
    Local,
    Myc,
}

impl SignerBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Myc => "myc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerConfig {
    pub backend: SignerBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayPublishPolicy {
    Any,
}

impl RelayPublishPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayConfigSource {
    Flags,
    Environment,
    UserConfig,
    WorkspaceConfig,
    Defaults,
}

impl RelayConfigSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flags => "cli flags · local first",
            Self::Environment => "environment · local first",
            Self::UserConfig => "user config · local first",
            Self::WorkspaceConfig => "workspace config · local first",
            Self::Defaults => "defaults · local first",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConfig {
    pub urls: Vec<String>,
    pub publish_policy: RelayPublishPolicy,
    pub source: RelayConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalConfig {
    pub root: PathBuf,
    pub replica_db_path: PathBuf,
    pub backups_dir: PathBuf,
    pub exports_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MycConfig {
    pub executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyfConfig {
    pub enabled: bool,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityBindingTargetKind {
    ManagedInstance,
    ExplicitEndpoint,
}

impl CapabilityBindingTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManagedInstance => "managed_instance",
            Self::ExplicitEndpoint => "explicit_endpoint",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityBindingSource {
    UserConfig,
    WorkspaceConfig,
}

impl CapabilityBindingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserConfig => "user config [[capability_binding]]",
            Self::WorkspaceConfig => "workspace config [[capability_binding]]",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityBindingConfig {
    pub capability_id: String,
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub target_kind: CapabilityBindingTargetKind,
    pub target: String,
    pub managed_account_ref: Option<String>,
    pub signer_session_ref: Option<String>,
    pub source: CapabilityBindingSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityBindingInspectionState {
    Configured,
    NotConfigured,
    Disabled,
}

impl CapabilityBindingInspectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::NotConfigured => "not_configured",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityBindingInspection {
    pub capability_id: String,
    pub provider_runtime_id: String,
    pub binding_model: String,
    pub state: CapabilityBindingInspectionState,
    pub source: String,
    pub target_kind: Option<String>,
    pub target: Option<String>,
    pub managed_account_ref: Option<String>,
    pub signer_session_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcConfig {
    pub url: String,
    pub bridge_bearer_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub output: OutputConfig,
    pub interaction: InteractionConfig,
    pub paths: PathsConfig,
    pub migration: MigrationConfig,
    pub logging: LoggingConfig,
    pub account: AccountConfig,
    pub account_secret_contract: AccountSecretContractConfig,
    pub identity: IdentityConfig,
    pub signer: SignerConfig,
    pub relay: RelayConfig,
    pub local: LocalConfig,
    pub myc: MycConfig,
    pub hyf: HyfConfig,
    pub rpc: RpcConfig,
    pub capability_bindings: Vec<CapabilityBindingConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationConfig {
    pub report: RadrootsMigrationReport,
}

#[derive(Debug, Default)]
pub(crate) struct EnvFileValues(BTreeMap<String, String>);

impl EnvFileValues {
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

#[derive(Debug, Default, Deserialize)]
struct CliConfigFile {
    relay: Option<RelayFileConfig>,
    hyf: Option<HyfFileConfig>,
    rpc: Option<RpcFileConfig>,
    capability_binding: Option<Vec<CapabilityBindingFileConfig>>,
}

#[derive(Debug, Default, Deserialize)]
struct RelayFileConfig {
    urls: Option<Vec<String>>,
    publish_policy: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RpcFileConfig {
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct HyfFileConfig {
    enabled: Option<bool>,
    executable: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct CapabilityBindingFileConfig {
    capability: String,
    provider: String,
    target_kind: String,
    target: String,
    managed_account_ref: Option<String>,
    signer_session_ref: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct CapabilityBindingSpec {
    capability_id: &'static str,
    provider_runtime_id: &'static str,
    binding_model: &'static str,
}

pub(crate) const SIGNER_REMOTE_NIP46_CAPABILITY: &str = "signer.remote_nip46";
pub(crate) const WRITE_PLANE_TRADE_JSONRPC_CAPABILITY: &str = "write_plane.trade_jsonrpc";
pub(crate) const WORKFLOW_TRADE_CAPABILITY: &str = "workflow.trade";
pub(crate) const INFERENCE_HYF_STDIO_CAPABILITY: &str = "inference.hyf_stdio";

const CAPABILITY_BINDING_SPECS: &[CapabilityBindingSpec] = &[
    CapabilityBindingSpec {
        capability_id: SIGNER_REMOTE_NIP46_CAPABILITY,
        provider_runtime_id: "myc",
        binding_model: "session_authorized_remote_signer",
    },
    CapabilityBindingSpec {
        capability_id: WRITE_PLANE_TRADE_JSONRPC_CAPABILITY,
        provider_runtime_id: "radrootsd",
        binding_model: "daemon_backed_jsonrpc",
    },
    CapabilityBindingSpec {
        capability_id: WORKFLOW_TRADE_CAPABILITY,
        provider_runtime_id: "rhi",
        binding_model: "out_of_process_worker",
    },
    CapabilityBindingSpec {
        capability_id: INFERENCE_HYF_STDIO_CAPABILITY,
        provider_runtime_id: "hyf",
        binding_model: "stdio_service",
    },
];

pub(crate) trait Environment {
    fn var(&self, key: &str) -> Option<String>;
    fn current_dir(&self) -> Result<PathBuf, RuntimeError>;
    fn path_resolver(&self) -> RadrootsPathResolver;
    fn stdin_is_tty(&self) -> bool;
    fn stdout_is_tty(&self) -> bool;
}

pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn current_dir(&self) -> Result<PathBuf, RuntimeError> {
        std::env::current_dir().map_err(|err| {
            RuntimeError::Config(format!("failed to resolve current directory: {err}"))
        })
    }

    fn path_resolver(&self) -> RadrootsPathResolver {
        RadrootsPathResolver::current()
    }

    fn stdin_is_tty(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn stdout_is_tty(&self) -> bool {
        std::io::stdout().is_terminal()
    }
}

impl RuntimeConfig {
    pub fn from_system(args: &CliArgs) -> Result<Self, RuntimeError> {
        let system = SystemEnvironment;
        let env_file_path = resolve_env_file_path(args, &system);
        let env_file = load_env_file_values(env_file_path.as_deref())?;
        Self::resolve_with_env_file(args, &system, &env_file)
    }

    fn resolve_with_env_file(
        args: &CliArgs,
        env: &dyn Environment,
        env_file: &EnvFileValues,
    ) -> Result<Self, RuntimeError> {
        let paths = resolve_paths(env, env_file)?;
        let migration = resolve_migration(paths.clone(), env);
        let workspace_config = load_cli_config_file(paths.workspace_config_path.as_path())?;
        let app_config = load_cli_config_file(paths.app_config_path.as_path())?;
        let account_secret_backend = resolve_account_secret_backend(args, env, env_file)?
            .unwrap_or(RadrootsSecretBackend::HostVault(
                RadrootsHostVaultPolicy::desktop(),
            ));
        let account_secret_fallback = resolve_account_secret_fallback(args, env, env_file)?
            .unwrap_or(match account_secret_backend {
                RadrootsSecretBackend::HostVault(_) => Some(RadrootsSecretBackend::EncryptedFile),
                _ => None,
            });
        let output = OutputConfig {
            format: resolve_output_format(args, env, env_file)?,
            verbosity: resolve_verbosity(args)?,
            color: !args.no_color,
            dry_run: args.dry_run,
        };
        let logging = LoggingConfig {
            filter: args
                .log_filter
                .clone()
                .or_else(|| env_value(env, env_file, &[ENV_CLI_LOG_FILTER, ENV_LOG_FILTER]))
                .unwrap_or_else(|| DEFAULT_LOG_FILTER.to_owned()),
            directory: args.log_dir.clone().or_else(|| {
                env_value(env, env_file, &[ENV_CLI_LOG_DIR, ENV_LOG_DIR])
                    .map(PathBuf::from)
                    .or_else(|| Some(paths.app_logs_root.clone()))
            }),
            stdout: resolve_bool_pair(
                args.log_stdout,
                args.no_log_stdout,
                &[ENV_CLI_LOG_STDOUT, ENV_LOG_STDOUT],
                false,
                env,
                env_file,
                "--log-stdout",
                "--no-log-stdout",
            )?,
        };
        validate_logging_output_contract(&output, &logging)?;
        Ok(Self {
            capability_bindings: resolve_capability_bindings(
                app_config.as_ref(),
                workspace_config.as_ref(),
            )?,
            output,
            interaction: resolve_interaction_config(args, env),
            paths: paths.clone(),
            migration,
            logging,
            account: AccountConfig {
                selector: args
                    .account
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_ACCOUNT])),
                store_path: paths
                    .shared_accounts_data_root
                    .join(DEFAULT_SHARED_ACCOUNTS_STORE_FILE),
                secrets_dir: paths.shared_accounts_secrets_root.clone(),
                secret_backend: account_secret_backend,
                secret_fallback: account_secret_fallback,
            },
            account_secret_contract: AccountSecretContractConfig {
                default_backend: CLI_DEFAULT_SECRET_BACKEND.to_owned(),
                default_fallback: Some(CLI_DEFAULT_SECRET_FALLBACK.to_owned()),
                allowed_backends: CLI_ALLOWED_SHARED_SECRET_BACKENDS
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
                host_vault_policy: Some(CLI_HOST_VAULT_POLICY.to_owned()),
                uses_protected_store: CLI_USES_PROTECTED_STORE,
            },
            identity: IdentityConfig {
                path: args
                    .identity_path
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_IDENTITY_PATH]).map(PathBuf::from))
                    .unwrap_or_else(|| paths.default_identity_path.clone()),
            },
            signer: SignerConfig {
                backend: args
                    .signer
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_SIGNER]))
                    .map(parse_signer_mode)
                    .transpose()?
                    .unwrap_or(SignerBackend::Local),
            },
            relay: resolve_relay_config(
                args,
                env,
                env_file,
                app_config.as_ref(),
                workspace_config.as_ref(),
            )?,
            local: LocalConfig {
                root: paths.app_data_root.join(DEFAULT_LOCAL_STATE_DIR),
                replica_db_path: paths
                    .app_data_root
                    .join(DEFAULT_LOCAL_STATE_DIR)
                    .join(DEFAULT_LOCAL_DB_FILE),
                backups_dir: paths
                    .app_data_root
                    .join(DEFAULT_LOCAL_STATE_DIR)
                    .join(DEFAULT_LOCAL_BACKUPS_DIR),
                exports_dir: paths
                    .app_data_root
                    .join(DEFAULT_LOCAL_STATE_DIR)
                    .join(DEFAULT_LOCAL_EXPORTS_DIR),
            },
            myc: MycConfig {
                executable: args
                    .myc_executable
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_MYC_EXECUTABLE]).map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("myc")),
            },
            hyf: HyfConfig {
                enabled: resolve_hyf_enabled(
                    args,
                    env,
                    env_file,
                    app_config.as_ref(),
                    workspace_config.as_ref(),
                )?,
                executable: resolve_hyf_executable(
                    args,
                    env,
                    env_file,
                    app_config.as_ref(),
                    workspace_config.as_ref(),
                ),
            },
            rpc: resolve_rpc_config(
                env,
                env_file,
                app_config.as_ref(),
                workspace_config.as_ref(),
            )?,
        })
    }

    pub fn inspect_capability_bindings(&self) -> Vec<CapabilityBindingInspection> {
        CAPABILITY_BINDING_SPECS
            .iter()
            .map(|spec| {
                if let Some(binding) = self
                    .capability_bindings
                    .iter()
                    .find(|binding| binding.capability_id == spec.capability_id)
                {
                    return CapabilityBindingInspection {
                        capability_id: binding.capability_id.clone(),
                        provider_runtime_id: binding.provider_runtime_id.clone(),
                        binding_model: binding.binding_model.clone(),
                        state: CapabilityBindingInspectionState::Configured,
                        source: binding.source.as_str().to_owned(),
                        target_kind: Some(binding.target_kind.as_str().to_owned()),
                        target: Some(binding.target.clone()),
                        managed_account_ref: binding.managed_account_ref.clone(),
                        signer_session_ref: binding.signer_session_ref.clone(),
                    };
                }

                let (state, source) = match spec.capability_id {
                    SIGNER_REMOTE_NIP46_CAPABILITY
                        if matches!(self.signer.backend, SignerBackend::Local) =>
                    {
                        (
                            CapabilityBindingInspectionState::Disabled,
                            "independent local signer mode".to_owned(),
                        )
                    }
                    INFERENCE_HYF_STDIO_CAPABILITY if !self.hyf.enabled => (
                        CapabilityBindingInspectionState::Disabled,
                        "hyf disabled by config".to_owned(),
                    ),
                    _ => (
                        CapabilityBindingInspectionState::NotConfigured,
                        "no explicit capability binding".to_owned(),
                    ),
                };

                CapabilityBindingInspection {
                    capability_id: spec.capability_id.to_owned(),
                    provider_runtime_id: spec.provider_runtime_id.to_owned(),
                    binding_model: spec.binding_model.to_owned(),
                    state,
                    source,
                    target_kind: None,
                    target: None,
                    managed_account_ref: None,
                    signer_session_ref: None,
                }
            })
            .collect()
    }

    pub fn capability_binding(&self, capability_id: &str) -> Option<&CapabilityBindingConfig> {
        self.capability_bindings
            .iter()
            .find(|binding| binding.capability_id == capability_id)
    }
}

fn resolve_migration(paths: PathsConfig, env: &dyn Environment) -> MigrationConfig {
    MigrationConfig {
        report: inspect_legacy_paths(legacy_path_candidates(&paths, env)),
    }
}

fn legacy_path_candidates(
    paths: &PathsConfig,
    env: &dyn Environment,
) -> Vec<RadrootsLegacyPathCandidate> {
    let Some(home_dir) = env.var("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let old_user_config = home_dir.join(".config/radroots/config.toml");
    let old_user_state_root = home_dir.join(".local/share/radroots");

    vec![
        RadrootsLegacyPathCandidate::new(
            "cli_user_config_v0",
            "legacy cli user config",
            old_user_config,
            Some(paths.app_config_path.clone()),
            "merge this config into the canonical app config path; the cli will not copy it on startup",
        ),
        RadrootsLegacyPathCandidate::new(
            "cli_user_state_root_v0",
            "legacy cli user state root",
            old_user_state_root,
            Some(paths.app_data_root.clone()),
            "export/import the old local state into the canonical app and shared namespaces; the cli will not move it on startup",
        ),
    ]
}

fn load_cli_config_file(path: &Path) -> Result<Option<CliConfigFile>, RuntimeError> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path).map_err(|err| {
        RuntimeError::Config(format!(
            "failed to read config file {}: {err}",
            path.display()
        ))
    })?;

    if raw.trim().is_empty() {
        return Ok(Some(CliConfigFile::default()));
    }

    toml::from_str::<CliConfigFile>(&raw)
        .map(Some)
        .map_err(|err| {
            RuntimeError::Config(format!(
                "failed to parse config file {}: {err}",
                path.display()
            ))
        })
}

fn resolve_rpc_config(
    env: &dyn Environment,
    env_file: &EnvFileValues,
    user_config: Option<&CliConfigFile>,
    workspace_config: Option<&CliConfigFile>,
) -> Result<RpcConfig, RuntimeError> {
    let url = env_value(env, env_file, &[ENV_RPC_URL])
        .or_else(|| {
            user_config
                .and_then(|config| config.rpc.as_ref())
                .and_then(|rpc| rpc.url.clone())
        })
        .or_else(|| {
            workspace_config
                .and_then(|config| config.rpc.as_ref())
                .and_then(|rpc| rpc.url.clone())
        })
        .unwrap_or_else(|| DEFAULT_RPC_URL.to_owned());

    Ok(RpcConfig {
        url: validate_rpc_url(url.as_str())?,
        bridge_bearer_token: env_value(env, env_file, &[ENV_RPC_BEARER_TOKEN]),
    })
}

fn resolve_capability_bindings(
    user_config: Option<&CliConfigFile>,
    workspace_config: Option<&CliConfigFile>,
) -> Result<Vec<CapabilityBindingConfig>, RuntimeError> {
    let workspace = resolve_file_capability_bindings(
        workspace_config.and_then(|config| config.capability_binding.as_deref()),
        CapabilityBindingSource::WorkspaceConfig,
    )?;
    let user = resolve_file_capability_bindings(
        user_config.and_then(|config| config.capability_binding.as_deref()),
        CapabilityBindingSource::UserConfig,
    )?;

    let mut merged = BTreeMap::new();
    for binding in workspace.into_iter().chain(user) {
        merged.insert(binding.capability_id.clone(), binding);
    }

    Ok(CAPABILITY_BINDING_SPECS
        .iter()
        .filter_map(|spec| merged.remove(spec.capability_id))
        .collect())
}

fn resolve_file_capability_bindings(
    bindings: Option<&[CapabilityBindingFileConfig]>,
    source: CapabilityBindingSource,
) -> Result<Vec<CapabilityBindingConfig>, RuntimeError> {
    let Some(bindings) = bindings else {
        return Ok(Vec::new());
    };

    let mut seen = BTreeMap::new();
    let mut resolved = Vec::with_capacity(bindings.len());

    for binding in bindings {
        let capability = binding.capability.trim();
        let provider = binding.provider.trim();
        let Some(spec) = capability_binding_spec(capability) else {
            return Err(RuntimeError::Config(format!(
                "unknown capability_binding capability `{capability}`"
            )));
        };
        if provider != spec.provider_runtime_id {
            return Err(RuntimeError::Config(format!(
                "capability_binding `{capability}` must use provider `{}`, got `{provider}`",
                spec.provider_runtime_id
            )));
        }
        if seen.insert(spec.capability_id.to_owned(), ()).is_some() {
            return Err(RuntimeError::Config(format!(
                "capability_binding `{capability}` is duplicated in one config file"
            )));
        }

        let target = binding.target.trim();
        if target.is_empty() {
            return Err(RuntimeError::Config(format!(
                "capability_binding `{capability}` target must not be empty"
            )));
        }

        let managed_account_ref = normalize_binding_ref(
            binding
                .managed_account_ref
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        );
        let signer_session_ref = normalize_binding_ref(
            binding
                .signer_session_ref
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
        );
        if spec.capability_id != SIGNER_REMOTE_NIP46_CAPABILITY
            && (managed_account_ref.is_some() || signer_session_ref.is_some())
        {
            return Err(RuntimeError::Config(format!(
                "capability_binding `{capability}` may not set managed_account_ref or signer_session_ref"
            )));
        }

        resolved.push(CapabilityBindingConfig {
            capability_id: spec.capability_id.to_owned(),
            provider_runtime_id: spec.provider_runtime_id.to_owned(),
            binding_model: spec.binding_model.to_owned(),
            target_kind: parse_capability_binding_target_kind(
                binding.target_kind.as_str(),
                spec.capability_id,
            )?,
            target: target.to_owned(),
            managed_account_ref,
            signer_session_ref,
            source,
        });
    }

    Ok(resolved)
}

fn capability_binding_spec(capability_id: &str) -> Option<CapabilityBindingSpec> {
    CAPABILITY_BINDING_SPECS
        .iter()
        .copied()
        .find(|spec| spec.capability_id == capability_id)
}

fn parse_capability_binding_target_kind(
    value: &str,
    capability_id: &str,
) -> Result<CapabilityBindingTargetKind, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "managed_instance" => Ok(CapabilityBindingTargetKind::ManagedInstance),
        "explicit_endpoint" => Ok(CapabilityBindingTargetKind::ExplicitEndpoint),
        other => Err(RuntimeError::Config(format!(
            "capability_binding `{capability_id}` target_kind must be `managed_instance` or `explicit_endpoint`, got `{other}`"
        ))),
    }
}

fn normalize_binding_ref(value: Option<&str>) -> Option<String> {
    value.map(ToOwned::to_owned)
}

fn resolve_relay_config(
    args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
    user_config: Option<&CliConfigFile>,
    workspace_config: Option<&CliConfigFile>,
) -> Result<RelayConfig, RuntimeError> {
    let publish_policy = resolve_relay_publish_policy(user_config, workspace_config)?
        .unwrap_or(RelayPublishPolicy::Any);

    if !args.relay.is_empty() {
        return Ok(RelayConfig {
            urls: normalize_relay_urls(args.relay.clone(), "--relay")?,
            publish_policy,
            source: RelayConfigSource::Flags,
        });
    }

    if let Some(value) = env_value(env, env_file, &[ENV_RELAYS]) {
        return Ok(RelayConfig {
            urls: parse_relay_env_value(value.as_str(), ENV_RELAYS)?,
            publish_policy,
            source: RelayConfigSource::Environment,
        });
    }

    if let Some(relay) = user_config.and_then(|config| config.relay.as_ref()) {
        if let Some(urls) = relay.urls.clone() {
            return Ok(RelayConfig {
                urls: normalize_relay_urls(urls, "user config [relay].urls")?,
                publish_policy,
                source: RelayConfigSource::UserConfig,
            });
        }
    }

    if let Some(relay) = workspace_config.and_then(|config| config.relay.as_ref()) {
        if let Some(urls) = relay.urls.clone() {
            return Ok(RelayConfig {
                urls: normalize_relay_urls(urls, "workspace config [relay].urls")?,
                publish_policy,
                source: RelayConfigSource::WorkspaceConfig,
            });
        }
    }

    Ok(RelayConfig {
        urls: Vec::new(),
        publish_policy,
        source: RelayConfigSource::Defaults,
    })
}

fn resolve_hyf_enabled(
    args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
    user_config: Option<&CliConfigFile>,
    workspace_config: Option<&CliConfigFile>,
) -> Result<bool, RuntimeError> {
    match (args.hyf_enabled, args.no_hyf_enabled) {
        (true, true) => {
            return Err(RuntimeError::Config(
                "flags --hyf-enabled and --no-hyf-enabled cannot be used together".to_owned(),
            ));
        }
        (true, false) => return Ok(true),
        (false, true) => return Ok(false),
        (false, false) => {}
    }

    if let Some((key, value)) = env_value_entry(env, env_file, &[ENV_HYF_ENABLED]) {
        return parse_bool_env(key.as_str(), value.as_str());
    }

    if let Some(enabled) = user_config
        .and_then(|config| config.hyf.as_ref())
        .and_then(|hyf| hyf.enabled)
    {
        return Ok(enabled);
    }

    if let Some(enabled) = workspace_config
        .and_then(|config| config.hyf.as_ref())
        .and_then(|hyf| hyf.enabled)
    {
        return Ok(enabled);
    }

    Ok(false)
}

fn resolve_hyf_executable(
    args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
    user_config: Option<&CliConfigFile>,
    workspace_config: Option<&CliConfigFile>,
) -> PathBuf {
    args.hyf_executable
        .clone()
        .or_else(|| env_value(env, env_file, &[ENV_HYF_EXECUTABLE]).map(PathBuf::from))
        .or_else(|| {
            user_config
                .and_then(|config| config.hyf.as_ref())
                .and_then(|hyf| hyf.executable.clone())
        })
        .or_else(|| {
            workspace_config
                .and_then(|config| config.hyf.as_ref())
                .and_then(|hyf| hyf.executable.clone())
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_HYF_EXECUTABLE))
}

fn resolve_relay_publish_policy(
    user_config: Option<&CliConfigFile>,
    workspace_config: Option<&CliConfigFile>,
) -> Result<Option<RelayPublishPolicy>, RuntimeError> {
    if let Some(value) = user_config
        .and_then(|config| config.relay.as_ref())
        .and_then(|relay| relay.publish_policy.as_deref())
    {
        return parse_relay_publish_policy(value).map(Some);
    }

    if let Some(value) = workspace_config
        .and_then(|config| config.relay.as_ref())
        .and_then(|relay| relay.publish_policy.as_deref())
    {
        return parse_relay_publish_policy(value).map(Some);
    }

    Ok(None)
}

fn parse_relay_publish_policy(value: &str) -> Result<RelayPublishPolicy, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "any" => Ok(RelayPublishPolicy::Any),
        other => Err(RuntimeError::Config(format!(
            "[relay].publish_policy must be `any`, got `{other}`"
        ))),
    }
}

fn validate_rpc_url(value: &str) -> Result<String, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config("rpc url must not be empty".to_owned()));
    }
    let parsed = Url::parse(trimmed)
        .map_err(|err| RuntimeError::Config(format!("rpc url `{trimmed}` is invalid: {err}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(RuntimeError::Config(format!(
            "rpc url must use http or https, got `{trimmed}`"
        )));
    }
    Ok(trimmed.to_owned())
}

fn parse_relay_env_value(value: &str, key: &str) -> Result<Vec<String>, RuntimeError> {
    let entries = value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if entries.is_empty() {
        return Err(RuntimeError::Config(format!(
            "{key} must contain at least one websocket relay url"
        )));
    }

    normalize_relay_urls(entries, key)
}

fn normalize_relay_urls(values: Vec<String>, source: &str) -> Result<Vec<String>, RuntimeError> {
    let mut normalized = Vec::new();
    for value in values {
        let relay = validate_relay_url(value.as_str(), source)?;
        if !normalized.iter().any(|existing| existing == &relay) {
            normalized.push(relay);
        }
    }
    Ok(normalized)
}

fn validate_relay_url(value: &str, source: &str) -> Result<String, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(format!(
            "{source} contains an empty relay url"
        )));
    }

    let parsed = Url::parse(trimmed).map_err(|err| {
        RuntimeError::Config(format!(
            "{source} contains invalid relay url `{trimmed}`: {err}"
        ))
    })?;

    if !matches!(parsed.scheme(), "ws" | "wss") || parsed.host_str().is_none() {
        return Err(RuntimeError::Config(format!(
            "{source} must use websocket relay urls, got `{trimmed}`"
        )));
    }

    Ok(trimmed.to_owned())
}

fn resolve_env_file_path(args: &CliArgs, env: &dyn Environment) -> Option<PathBuf> {
    args.env_file
        .clone()
        .or_else(|| env.var(ENV_FILE_PATH).map(PathBuf::from))
        .or_else(|| {
            let default_path = PathBuf::from(DEFAULT_ENV_PATH);
            default_path.exists().then_some(default_path)
        })
}

fn resolve_output_format(
    args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
) -> Result<OutputFormat, RuntimeError> {
    if args.output_format.is_some() && (args.json || args.ndjson) {
        return Err(RuntimeError::Config(
            "flags --output, --json, and --ndjson cannot be used together".to_owned(),
        ));
    }

    match (args.output_format, args.json, args.ndjson) {
        (_, true, true) => {
            return Err(RuntimeError::Config(
                "flags --json and --ndjson cannot be used together".to_owned(),
            ));
        }
        (Some(format), false, false) => return Ok(format.as_output_format()),
        (None, true, false) => return Ok(OutputFormat::Json),
        (None, false, true) => return Ok(OutputFormat::Ndjson),
        (None, false, false) => {}
        (Some(_), true, false) | (Some(_), false, true) => unreachable!(),
    }
    match env_value(env, env_file, &[ENV_OUTPUT]) {
        Some(value) => parse_output_format(value.as_str()),
        None => Ok(OutputFormat::Human),
    }
}

fn resolve_verbosity(args: &CliArgs) -> Result<Verbosity, RuntimeError> {
    let selected = [args.quiet, args.verbose, args.trace]
        .into_iter()
        .filter(|selected| *selected)
        .count();
    if selected > 1 {
        return Err(RuntimeError::Config(
            "flags --quiet, --verbose, and --trace are mutually exclusive".to_owned(),
        ));
    }

    if args.quiet {
        Ok(Verbosity::Quiet)
    } else if args.trace {
        Ok(Verbosity::Trace)
    } else if args.verbose {
        Ok(Verbosity::Verbose)
    } else {
        Ok(Verbosity::Normal)
    }
}

fn resolve_interaction_config(args: &CliArgs, env: &dyn Environment) -> InteractionConfig {
    let stdin_tty = env.stdin_is_tty();
    let stdout_tty = env.stdout_is_tty();
    let input_enabled = !args.no_input;
    let prompts_allowed = input_enabled && stdin_tty && stdout_tty;
    let confirmations_allowed = prompts_allowed && !args.yes;
    InteractionConfig {
        input_enabled,
        assume_yes: args.yes,
        stdin_tty,
        stdout_tty,
        prompts_allowed,
        confirmations_allowed,
    }
}

fn validate_logging_output_contract(
    output: &OutputConfig,
    logging: &LoggingConfig,
) -> Result<(), RuntimeError> {
    if logging.stdout && matches!(output.format, OutputFormat::Json | OutputFormat::Ndjson) {
        return Err(RuntimeError::Config(format!(
            "stdout logging cannot be used with {} output; unset {ENV_CLI_LOG_STDOUT}/{ENV_LOG_STDOUT} or use --no-log-stdout",
            output.format.as_str()
        )));
    }

    Ok(())
}

fn resolve_bool_pair(
    positive_flag: bool,
    negative_flag: bool,
    env_keys: &[&str],
    default: bool,
    env: &dyn Environment,
    env_file: &EnvFileValues,
    positive_label: &str,
    negative_label: &str,
) -> Result<bool, RuntimeError> {
    match (positive_flag, negative_flag) {
        (true, true) => Err(RuntimeError::Config(format!(
            "flags {positive_label} and {negative_label} cannot be used together"
        ))),
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        (false, false) => match env_value_entry(env, env_file, env_keys) {
            Some((key, value)) => parse_bool_env(key.as_str(), value.as_str()),
            None => Ok(default),
        },
    }
}

fn env_value(env: &dyn Environment, env_file: &EnvFileValues, keys: &[&str]) -> Option<String> {
    env_value_entry(env, env_file, keys).map(|(_, value)| value)
}

fn env_value_entry(
    env: &dyn Environment,
    env_file: &EnvFileValues,
    keys: &[&str],
) -> Option<(String, String)> {
    keys.iter()
        .find_map(|key| env.var(key).map(|value| ((*key).to_owned(), value)))
        .or_else(|| {
            keys.iter().find_map(|key| {
                env_file
                    .0
                    .get(*key)
                    .cloned()
                    .map(|value| ((*key).to_owned(), value))
            })
        })
}

fn load_env_file_values(path: Option<&Path>) -> Result<EnvFileValues, RuntimeError> {
    let Some(path) = path else {
        return Ok(EnvFileValues::default());
    };
    let raw = fs::read_to_string(path).map_err(|err| {
        RuntimeError::Config(format!("failed to read env file {}: {err}", path.display()))
    })?;
    parse_env_file_values(&raw, path)
}

fn parse_env_file_values(raw: &str, path: &Path) -> Result<EnvFileValues, RuntimeError> {
    let mut values = BTreeMap::new();

    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(RuntimeError::Config(format!(
                "invalid env file {} line {}: expected KEY=VALUE",
                path.display(),
                index + 1
            )));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(RuntimeError::Config(format!(
                "invalid env file {} line {}: empty key",
                path.display(),
                index + 1
            )));
        }
        if !SUPPORTED_ENV_FILE_KEYS.contains(&key) {
            return Err(RuntimeError::Config(format!(
                "invalid env file {} line {}: unknown environment variable `{key}`",
                path.display(),
                index + 1
            )));
        }
        values.insert(key.to_owned(), normalize_env_value(value.trim()));
    }

    Ok(EnvFileValues(values))
}

fn normalize_env_value(value: &str) -> String {
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

fn parse_output_format(value: &str) -> Result<OutputFormat, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "human" => Ok(OutputFormat::Human),
        "json" => Ok(OutputFormat::Json),
        "ndjson" => Ok(OutputFormat::Ndjson),
        other => Err(RuntimeError::Config(format!(
            "{ENV_OUTPUT} must be `human`, `json`, or `ndjson`, got `{other}`"
        ))),
    }
}

fn parse_signer_mode(value: String) -> Result<SignerBackend, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local" => Ok(SignerBackend::Local),
        "myc" => Ok(SignerBackend::Myc),
        other => Err(RuntimeError::Config(format!(
            "{ENV_SIGNER} or --signer must be `local` or `myc`, got `{other}`"
        ))),
    }
}

fn resolve_account_secret_backend(
    _args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
) -> Result<Option<RadrootsSecretBackend>, RuntimeError> {
    env_value_entry(env, env_file, &[ENV_ACCOUNT_SECRET_BACKEND])
        .map(|(key, value)| parse_account_secret_backend(key.as_str(), value.as_str()))
        .transpose()
}

fn resolve_account_secret_fallback(
    _args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
) -> Result<Option<Option<RadrootsSecretBackend>>, RuntimeError> {
    env_value_entry(env, env_file, &[ENV_ACCOUNT_SECRET_FALLBACK])
        .map(|(key, value)| parse_account_secret_fallback(key.as_str(), value.as_str()))
        .transpose()
}

fn parse_account_secret_fallback(
    key: &str,
    value: &str,
) -> Result<Option<RadrootsSecretBackend>, RuntimeError> {
    if value.trim().eq_ignore_ascii_case("none") {
        return Ok(None);
    }

    parse_account_secret_backend(key, value).map(Some)
}

fn parse_account_secret_backend(
    key: &str,
    value: &str,
) -> Result<RadrootsSecretBackend, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "host_vault" => Ok(RadrootsSecretBackend::HostVault(
            RadrootsHostVaultPolicy::desktop(),
        )),
        "encrypted_file" => Ok(RadrootsSecretBackend::EncryptedFile),
        "memory" => Ok(RadrootsSecretBackend::Memory),
        other => Err(RuntimeError::Config(format!(
            "{key} must be `host_vault`, `encrypted_file`, `memory`, or `none` for fallback, got `{other}`"
        ))),
    }
}

fn parse_bool_env(key: &str, value: &str) -> Result<bool, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(RuntimeError::Config(format!(
            "{key} must be a boolean value, got `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccountConfig, AccountSecretContractConfig, CapabilityBindingConfig,
        CapabilityBindingSource, CapabilityBindingTargetKind, EnvFileValues, Environment,
        HyfConfig, INFERENCE_HYF_STDIO_CAPABILITY, InteractionConfig, OutputConfig, OutputFormat,
        PathsConfig, RelayConfigSource, RelayPublishPolicy, RuntimeConfig, SignerBackend,
        Verbosity, parse_env_file_values,
    };
    use crate::cli::CliArgs;
    use radroots_runtime_paths::{RadrootsHostEnvironment, RadrootsPathResolver, RadrootsPlatform};
    use radroots_secret_vault::{RadrootsHostVaultPolicy, RadrootsSecretBackend};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    struct MapEnvironment {
        values: BTreeMap<String, String>,
        current_dir: PathBuf,
        path_resolver: RadrootsPathResolver,
        stdin_tty: bool,
        stdout_tty: bool,
    }

    impl MapEnvironment {
        fn new(values: BTreeMap<String, String>) -> Self {
            Self {
                values,
                current_dir: PathBuf::from("/workspaces/radroots-cli"),
                path_resolver: RadrootsPathResolver::new(
                    RadrootsPlatform::Linux,
                    RadrootsHostEnvironment {
                        home_dir: Some(PathBuf::from("/home/tester")),
                        ..RadrootsHostEnvironment::default()
                    },
                ),
                stdin_tty: false,
                stdout_tty: false,
            }
        }

        fn with_tty(mut self, stdin_tty: bool, stdout_tty: bool) -> Self {
            self.stdin_tty = stdin_tty;
            self.stdout_tty = stdout_tty;
            self
        }
    }

    impl Environment for MapEnvironment {
        fn var(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn current_dir(&self) -> Result<PathBuf, crate::runtime::RuntimeError> {
            Ok(self.current_dir.clone())
        }

        fn path_resolver(&self) -> RadrootsPathResolver {
            self.path_resolver.clone()
        }

        fn stdin_is_tty(&self) -> bool {
            self.stdin_tty
        }

        fn stdout_is_tty(&self) -> bool {
            self.stdout_tty
        }
    }

    #[test]
    fn flags_override_environment_values() {
        let args = CliArgs::parse_from([
            "radroots",
            "--output",
            "human",
            "--verbose",
            "--dry-run",
            "--no-color",
            "--log-filter",
            "debug",
            "--log-stdout",
            "--identity-path",
            "custom-identity.json",
            "--signer",
            "local",
            "--relay",
            "wss://relay.one",
            "--relay",
            "wss://relay.two",
            "--myc-executable",
            "bin/myc-cli",
            "--hyf-enabled",
            "--hyf-executable",
            "bin/hyfd-cli",
            "config",
            "show",
        ]);
        let env = MapEnvironment::new(BTreeMap::from([
            ("RADROOTS_OUTPUT".to_owned(), "human".to_owned()),
            ("RADROOTS_LOG_FILTER".to_owned(), "trace".to_owned()),
            ("RADROOTS_LOG_STDOUT".to_owned(), "false".to_owned()),
            (
                "RADROOTS_IDENTITY_PATH".to_owned(),
                "env-identity.json".to_owned(),
            ),
            ("RADROOTS_SIGNER".to_owned(), "myc".to_owned()),
            ("RADROOTS_RELAYS".to_owned(), "wss://relay.env".to_owned()),
            ("RADROOTS_MYC_EXECUTABLE".to_owned(), "env-myc".to_owned()),
            ("RADROOTS_HYF_ENABLED".to_owned(), "false".to_owned()),
            ("RADROOTS_HYF_EXECUTABLE".to_owned(), "env-hyfd".to_owned()),
        ]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(
            resolved.output,
            OutputConfig {
                format: OutputFormat::Human,
                verbosity: Verbosity::Verbose,
                color: false,
                dry_run: true,
            }
        );
        assert_eq!(
            resolved.interaction,
            InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: false,
                stdout_tty: false,
                prompts_allowed: false,
                confirmations_allowed: false,
            }
        );
        assert_eq!(
            resolved.paths,
            PathsConfig {
                profile: "interactive_user".to_owned(),
                profile_source: "default".to_owned(),
                allowed_profiles: vec!["interactive_user".to_owned(), "repo_local".to_owned(),],
                root_source: "host_defaults".to_owned(),
                repo_local_root: None,
                repo_local_root_source: None,
                subordinate_path_override_source: "runtime_config".to_owned(),
                app_namespace: "apps/cli".to_owned(),
                shared_accounts_namespace: "shared/accounts".to_owned(),
                shared_identities_namespace: "shared/identities".to_owned(),
                app_config_path: PathBuf::from(
                    "/home/tester/.radroots/config/apps/cli/config.toml"
                ),
                workspace_config_path: PathBuf::from(
                    "/workspaces/radroots-cli/infra/local/runtime/radroots/config.toml"
                ),
                app_data_root: PathBuf::from("/home/tester/.radroots/data/apps/cli"),
                app_logs_root: PathBuf::from("/home/tester/.radroots/logs/apps/cli"),
                shared_accounts_data_root: PathBuf::from(
                    "/home/tester/.radroots/data/shared/accounts"
                ),
                shared_accounts_secrets_root: PathBuf::from(
                    "/home/tester/.radroots/secrets/shared/accounts"
                ),
                default_identity_path: PathBuf::from(
                    "/home/tester/.radroots/secrets/shared/identities/default.json"
                ),
            }
        );
        assert_eq!(resolved.logging.filter, "debug");
        assert!(resolved.logging.stdout);
        assert_eq!(
            resolved.identity.path,
            PathBuf::from("custom-identity.json")
        );
        assert_eq!(
            resolved.account,
            AccountConfig {
                selector: None,
                store_path: PathBuf::from("/home/tester/.radroots/data/shared/accounts/store.json"),
                secrets_dir: PathBuf::from("/home/tester/.radroots/secrets/shared/accounts"),
                secret_backend: RadrootsSecretBackend::HostVault(
                    RadrootsHostVaultPolicy::desktop(),
                ),
                secret_fallback: Some(RadrootsSecretBackend::EncryptedFile),
            }
        );
        assert_eq!(
            resolved.account_secret_contract,
            AccountSecretContractConfig {
                default_backend: "host_vault".to_owned(),
                default_fallback: Some("encrypted_file".to_owned()),
                allowed_backends: vec![
                    "host_vault".to_owned(),
                    "encrypted_file".to_owned(),
                    "memory".to_owned(),
                ],
                host_vault_policy: Some("desktop".to_owned()),
                uses_protected_store: true,
            }
        );
        assert_eq!(resolved.signer.backend, SignerBackend::Local);
        assert_eq!(
            resolved.relay.urls,
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
        assert_eq!(resolved.relay.source, RelayConfigSource::Flags);
        assert_eq!(resolved.relay.publish_policy, RelayPublishPolicy::Any);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc-cli"));
        assert_eq!(
            resolved.hyf,
            HyfConfig {
                enabled: true,
                executable: PathBuf::from("bin/hyfd-cli"),
            }
        );
    }

    #[test]
    fn environment_values_fill_missing_flags() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([
            ("RADROOTS_OUTPUT".to_owned(), "json".to_owned()),
            (
                "RADROOTS_LOG_FILTER".to_owned(),
                "debug,cli=trace".to_owned(),
            ),
            ("RADROOTS_LOG_DIR".to_owned(), "logs/runtime".to_owned()),
            ("RADROOTS_LOG_STDOUT".to_owned(), "false".to_owned()),
            ("RADROOTS_ACCOUNT".to_owned(), "acct_demo".to_owned()),
            (
                "RADROOTS_IDENTITY_PATH".to_owned(),
                "state/identity.json".to_owned(),
            ),
            ("RADROOTS_SIGNER".to_owned(), "myc".to_owned()),
            (
                "RADROOTS_RELAYS".to_owned(),
                "wss://relay.one,wss://relay.two".to_owned(),
            ),
            ("RADROOTS_MYC_EXECUTABLE".to_owned(), "bin/myc".to_owned()),
            ("RADROOTS_HYF_ENABLED".to_owned(), "true".to_owned()),
            ("RADROOTS_HYF_EXECUTABLE".to_owned(), "bin/hyfd".to_owned()),
        ]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(
            resolved.output,
            OutputConfig {
                format: OutputFormat::Json,
                verbosity: Verbosity::Normal,
                color: true,
                dry_run: false,
            }
        );
        assert_eq!(
            resolved.interaction,
            InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: false,
                stdout_tty: false,
                prompts_allowed: false,
                confirmations_allowed: false,
            }
        );
        assert_eq!(resolved.logging.filter, "debug,cli=trace");
        assert_eq!(
            resolved.logging.directory,
            Some(PathBuf::from("logs/runtime"))
        );
        assert!(!resolved.logging.stdout);
        assert_eq!(resolved.account.selector.as_deref(), Some("acct_demo"));
        assert_eq!(
            resolved.account.secret_backend,
            RadrootsSecretBackend::HostVault(RadrootsHostVaultPolicy::desktop())
        );
        assert_eq!(
            resolved.account.secret_fallback,
            Some(RadrootsSecretBackend::EncryptedFile)
        );
        assert_eq!(resolved.identity.path, PathBuf::from("state/identity.json"));
        assert_eq!(resolved.signer.backend, SignerBackend::Myc);
        assert_eq!(
            resolved.relay.urls,
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
        assert_eq!(resolved.relay.source, RelayConfigSource::Environment);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc"));
        assert_eq!(
            resolved.hyf,
            HyfConfig {
                enabled: true,
                executable: PathBuf::from("bin/hyfd"),
            }
        );
    }

    #[test]
    fn conflicting_boolean_flags_fail() {
        let args = CliArgs::parse_from([
            "radroots",
            "--log-stdout",
            "--no-log-stdout",
            "config",
            "show",
        ]);
        let env = MapEnvironment::new(BTreeMap::new());
        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("conflicting flags");
        assert!(error.to_string().contains("cannot be used together"));

        let hyf_args = CliArgs::parse_from([
            "radroots",
            "--hyf-enabled",
            "--no-hyf-enabled",
            "config",
            "show",
        ]);
        let error =
            RuntimeConfig::resolve_with_env_file(&hyf_args, &env, &EnvFileValues::default())
                .expect_err("conflicting hyf flags");
        assert!(error.to_string().contains("--hyf-enabled"));
    }

    #[test]
    fn conflicting_output_and_verbosity_flags_fail() {
        let env = MapEnvironment::new(BTreeMap::new());

        let conflicting_output =
            CliArgs::parse_from(["radroots", "--json", "--ndjson", "config", "show"]);
        let error = RuntimeConfig::resolve_with_env_file(
            &conflicting_output,
            &env,
            &EnvFileValues::default(),
        )
        .expect_err("conflicting output flags");
        assert!(error.to_string().contains("--json and --ndjson"));

        let conflicting_verbosity =
            CliArgs::parse_from(["radroots", "--quiet", "--trace", "config", "show"]);
        let error = RuntimeConfig::resolve_with_env_file(
            &conflicting_verbosity,
            &env,
            &EnvFileValues::default(),
        )
        .expect_err("conflicting verbosity flags");
        assert!(
            error
                .to_string()
                .contains("--quiet, --verbose, and --trace")
        );

        let conflicting_aliases =
            CliArgs::parse_from(["radroots", "--output", "json", "--json", "config", "show"]);
        let error = RuntimeConfig::resolve_with_env_file(
            &conflicting_aliases,
            &env,
            &EnvFileValues::default(),
        )
        .expect_err("conflicting output aliases");
        assert!(error.to_string().contains("--output, --json, and --ndjson"));
    }

    #[test]
    fn machine_output_rejects_stdout_logging_flags() {
        let env = MapEnvironment::new(BTreeMap::new());

        let json_args =
            CliArgs::parse_from(["radroots", "--json", "--log-stdout", "config", "show"]);
        let error =
            RuntimeConfig::resolve_with_env_file(&json_args, &env, &EnvFileValues::default())
                .expect_err("json stdout logging should fail");
        let message = error.to_string();
        assert!(message.contains("stdout logging"));
        assert!(message.contains("json output"));
        assert!(message.contains("--no-log-stdout"));

        let ndjson_args =
            CliArgs::parse_from(["radroots", "--ndjson", "--log-stdout", "find", "eggs"]);
        let error =
            RuntimeConfig::resolve_with_env_file(&ndjson_args, &env, &EnvFileValues::default())
                .expect_err("ndjson stdout logging should fail");
        let message = error.to_string();
        assert!(message.contains("stdout logging"));
        assert!(message.contains("ndjson output"));
    }

    #[test]
    fn machine_output_rejects_stdout_logging_environment() {
        let json_args = CliArgs::parse_from(["radroots", "--json", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([(
            "RADROOTS_CLI_LOGGING_STDOUT".to_owned(),
            "true".to_owned(),
        )]));
        let error =
            RuntimeConfig::resolve_with_env_file(&json_args, &env, &EnvFileValues::default())
                .expect_err("json stdout logging from env should fail");
        let message = error.to_string();
        assert!(message.contains("RADROOTS_CLI_LOGGING_STDOUT"));
        assert!(message.contains("RADROOTS_LOG_STDOUT"));

        let ndjson_env_args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([
            ("RADROOTS_OUTPUT".to_owned(), "ndjson".to_owned()),
            ("RADROOTS_LOG_STDOUT".to_owned(), "true".to_owned()),
        ]));
        let error =
            RuntimeConfig::resolve_with_env_file(&ndjson_env_args, &env, &EnvFileValues::default())
                .expect_err("ndjson stdout logging from env should fail");
        assert!(error.to_string().contains("ndjson output"));
    }

    #[test]
    fn no_log_stdout_overrides_environment_for_machine_output() {
        let args = CliArgs::parse_from(["radroots", "--json", "--no-log-stdout", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([(
            "RADROOTS_LOG_STDOUT".to_owned(),
            "true".to_owned(),
        )]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve machine output with stdout logging disabled");
        assert_eq!(resolved.output.format, OutputFormat::Json);
        assert!(!resolved.logging.stdout);
    }

    #[test]
    fn invalid_environment_value_fails() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([(
            "RADROOTS_LOG_STDOUT".to_owned(),
            "maybe".to_owned(),
        )]));
        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("invalid bool");
        assert!(error.to_string().contains("RADROOTS_LOG_STDOUT"));
    }

    #[test]
    fn env_file_values_fill_missing_flags() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::new());
        let env_file = parse_env_file_values(
            r#"
RADROOTS_OUTPUT=json
RADROOTS_CLI_LOGGING_FILTER="debug,radroots_cli=trace"
RADROOTS_CLI_LOGGING_OUTPUT_DIR=/tmp/radroots-cli-logs
RADROOTS_CLI_LOGGING_STDOUT=false
RADROOTS_ACCOUNT=acct_env_file
RADROOTS_IDENTITY_PATH=state/identity.json
RADROOTS_SIGNER=myc
RADROOTS_RELAYS=wss://relay.env-file
RADROOTS_MYC_EXECUTABLE=bin/myc
RADROOTS_HYF_ENABLED=true
RADROOTS_HYF_EXECUTABLE=bin/hyfd
"#,
            Path::new(".env.test"),
        )
        .expect("parse env file");

        let resolved =
            RuntimeConfig::resolve_with_env_file(&args, &env, &env_file).expect("resolve config");
        assert_eq!(resolved.output.format, OutputFormat::Json);
        assert_eq!(resolved.logging.filter, "debug,radroots_cli=trace");
        assert_eq!(
            resolved.logging.directory,
            Some(PathBuf::from("/tmp/radroots-cli-logs"))
        );
        assert!(!resolved.logging.stdout);
        assert_eq!(resolved.account.selector.as_deref(), Some("acct_env_file"));
        assert_eq!(resolved.identity.path, PathBuf::from("state/identity.json"));
        assert_eq!(resolved.signer.backend, SignerBackend::Myc);
        assert_eq!(resolved.relay.urls, vec!["wss://relay.env-file".to_owned()]);
        assert_eq!(resolved.relay.source, RelayConfigSource::Environment);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc"));
        assert_eq!(
            resolved.hyf,
            HyfConfig {
                enabled: true,
                executable: PathBuf::from("bin/hyfd"),
            }
        );
    }

    #[test]
    fn explicit_output_flag_overrides_environment_output() {
        let args = CliArgs::parse_from(["radroots", "--output", "ndjson", "find", "eggs"]);
        let env = MapEnvironment::new(BTreeMap::from([(
            "RADROOTS_OUTPUT".to_owned(),
            "json".to_owned(),
        )]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(resolved.output.format, OutputFormat::Ndjson);
    }

    #[test]
    fn interaction_config_reflects_tty_and_flags() {
        let args = CliArgs::parse_from(["radroots", "--no-input", "--yes", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::new()).with_tty(true, true);

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(
            resolved.interaction,
            InteractionConfig {
                input_enabled: false,
                assume_yes: true,
                stdin_tty: true,
                stdout_tty: true,
                prompts_allowed: false,
                confirmations_allowed: false,
            }
        );

        let interactive_args = CliArgs::parse_from(["radroots", "config", "show"]);
        let interactive = RuntimeConfig::resolve_with_env_file(
            &interactive_args,
            &env,
            &EnvFileValues::default(),
        )
        .expect("resolve interactive runtime config");
        assert_eq!(
            interactive.interaction,
            InteractionConfig {
                input_enabled: true,
                assume_yes: false,
                stdin_tty: true,
                stdout_tty: true,
                prompts_allowed: true,
                confirmations_allowed: true,
            }
        );
    }

    #[test]
    fn process_environment_overrides_env_file_values() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([
            ("RADROOTS_LOG_FILTER".to_owned(), "info".to_owned()),
            ("RADROOTS_LOG_STDOUT".to_owned(), "true".to_owned()),
        ]));
        let env_file = parse_env_file_values(
            r#"
RADROOTS_CLI_LOGGING_FILTER=debug
RADROOTS_CLI_LOGGING_STDOUT=false
"#,
            Path::new(".env.test"),
        )
        .expect("parse env file");

        let resolved =
            RuntimeConfig::resolve_with_env_file(&args, &env, &env_file).expect("resolve config");
        assert_eq!(resolved.output.format, OutputFormat::Human);
        assert_eq!(resolved.logging.filter, "info");
        assert!(resolved.logging.stdout);
    }

    #[test]
    fn user_relay_config_overrides_workspace_relay_config() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let user_home = temp.path().join("home");
        fs::create_dir_all(workspace_root.join("infra/local/runtime/radroots"))
            .expect("workspace config dir");
        fs::create_dir_all(user_home.join(".radroots/config/apps/cli")).expect("app config dir");
        fs::write(
            workspace_root.join("infra/local/runtime/radroots/config.toml"),
            "[relay]\nurls = [\"wss://relay.workspace\"]\npublish_policy = \"any\"\n",
        )
        .expect("write workspace config");
        fs::write(
            user_home.join(".radroots/config/apps/cli/config.toml"),
            "[relay]\nurls = [\"wss://relay.user\", \"wss://relay.workspace\"]\n",
        )
        .expect("write user config");

        let env = MapEnvironment {
            values: BTreeMap::new(),
            current_dir: workspace_root,
            path_resolver: RadrootsPathResolver::new(
                RadrootsPlatform::Linux,
                RadrootsHostEnvironment {
                    home_dir: Some(user_home),
                    ..RadrootsHostEnvironment::default()
                },
            ),
            stdin_tty: false,
            stdout_tty: false,
        };
        let args = CliArgs::parse_from(["radroots", "config", "show"]);

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve config");
        assert_eq!(
            resolved.relay.urls,
            vec![
                "wss://relay.user".to_owned(),
                "wss://relay.workspace".to_owned()
            ]
        );
        assert_eq!(resolved.relay.source, RelayConfigSource::UserConfig);
        assert_eq!(resolved.relay.publish_policy, RelayPublishPolicy::Any);
    }

    #[test]
    fn user_hyf_config_overrides_workspace_hyf_config() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let user_home = temp.path().join("home");
        fs::create_dir_all(workspace_root.join("infra/local/runtime/radroots"))
            .expect("workspace config dir");
        fs::create_dir_all(user_home.join(".radroots/config/apps/cli")).expect("app config dir");
        fs::write(
            workspace_root.join("infra/local/runtime/radroots/config.toml"),
            "[hyf]\nenabled = false\nexecutable = \"workspace-hyfd\"\n",
        )
        .expect("write workspace config");
        fs::write(
            user_home.join(".radroots/config/apps/cli/config.toml"),
            "[hyf]\nenabled = true\nexecutable = \"user-hyfd\"\n",
        )
        .expect("write user config");

        let env = MapEnvironment {
            values: BTreeMap::new(),
            current_dir: workspace_root,
            path_resolver: RadrootsPathResolver::new(
                RadrootsPlatform::Linux,
                RadrootsHostEnvironment {
                    home_dir: Some(user_home),
                    ..RadrootsHostEnvironment::default()
                },
            ),
            stdin_tty: false,
            stdout_tty: false,
        };
        let args = CliArgs::parse_from(["radroots", "config", "show"]);

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve config");
        assert_eq!(
            resolved.hyf,
            HyfConfig {
                enabled: true,
                executable: PathBuf::from("user-hyfd"),
            }
        );
    }

    #[test]
    fn user_capability_binding_overrides_workspace_binding() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let user_home = temp.path().join("home");
        fs::create_dir_all(workspace_root.join("infra/local/runtime/radroots"))
            .expect("workspace config dir");
        fs::create_dir_all(user_home.join(".radroots/config/apps/cli")).expect("app config dir");
        fs::write(
            workspace_root.join("infra/local/runtime/radroots/config.toml"),
            r#"
[[capability_binding]]
capability = "inference.hyf_stdio"
provider = "hyf"
target_kind = "managed_instance"
target = "workspace-hyf"
"#,
        )
        .expect("write workspace config");
        fs::write(
            user_home.join(".radroots/config/apps/cli/config.toml"),
            r#"
[[capability_binding]]
capability = "inference.hyf_stdio"
provider = "hyf"
target_kind = "explicit_endpoint"
target = "bin/user-hyfd"
"#,
        )
        .expect("write user config");

        let env = MapEnvironment {
            values: BTreeMap::new(),
            current_dir: workspace_root,
            path_resolver: RadrootsPathResolver::new(
                RadrootsPlatform::Linux,
                RadrootsHostEnvironment {
                    home_dir: Some(user_home),
                    ..RadrootsHostEnvironment::default()
                },
            ),
            stdin_tty: false,
            stdout_tty: false,
        };
        let args = CliArgs::parse_from(["radroots", "config", "show"]);

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve config");
        assert_eq!(resolved.capability_bindings.len(), 1);
        assert_eq!(
            resolved.capability_bindings[0],
            CapabilityBindingConfig {
                capability_id: INFERENCE_HYF_STDIO_CAPABILITY.to_owned(),
                provider_runtime_id: "hyf".to_owned(),
                binding_model: "stdio_service".to_owned(),
                target_kind: CapabilityBindingTargetKind::ExplicitEndpoint,
                target: "bin/user-hyfd".to_owned(),
                managed_account_ref: None,
                signer_session_ref: None,
                source: CapabilityBindingSource::UserConfig,
            }
        );
    }

    #[test]
    fn invalid_capability_binding_provider_fails() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let user_home = temp.path().join("home");
        fs::create_dir_all(workspace_root.join("infra/local/runtime/radroots"))
            .expect("workspace config dir");
        fs::create_dir_all(user_home.join(".radroots/config/apps/cli")).expect("app config dir");
        fs::write(
            workspace_root.join("infra/local/runtime/radroots/config.toml"),
            r#"
[[capability_binding]]
capability = "write_plane.trade_jsonrpc"
provider = "hyf"
target_kind = "explicit_endpoint"
target = "https://rpc.workspace.test/jsonrpc"
"#,
        )
        .expect("write workspace config");

        let env = MapEnvironment {
            values: BTreeMap::new(),
            current_dir: workspace_root,
            path_resolver: RadrootsPathResolver::new(
                RadrootsPlatform::Linux,
                RadrootsHostEnvironment {
                    home_dir: Some(user_home),
                    ..RadrootsHostEnvironment::default()
                },
            ),
            stdin_tty: false,
            stdout_tty: false,
        };
        let args = CliArgs::parse_from(["radroots", "config", "show"]);

        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("invalid capability binding provider");
        assert!(
            error
                .to_string()
                .contains("must use provider `radrootsd`, got `hyf`")
        );
    }

    #[test]
    fn invalid_relay_url_fails() {
        let args = CliArgs::parse_from([
            "radroots",
            "--relay",
            "https://not-a-websocket.example.com",
            "relay",
            "ls",
        ]);
        let env = MapEnvironment::new(BTreeMap::new());
        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("invalid relay url");
        assert!(error.to_string().contains("websocket relay urls"));
    }

    #[test]
    fn state_roots_are_resolved_from_home_and_workspace() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::new());
        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");

        assert_eq!(
            resolved.paths.app_config_path,
            PathBuf::from("/home/tester/.radroots/config/apps/cli/config.toml")
        );
        assert_eq!(resolved.paths.profile_source, "default");
        assert_eq!(resolved.paths.root_source, "host_defaults");
        assert_eq!(resolved.paths.repo_local_root, None);
        assert_eq!(resolved.paths.repo_local_root_source, None);
        assert_eq!(
            resolved.paths.subordinate_path_override_source,
            "runtime_config"
        );
        assert_eq!(resolved.paths.app_namespace, "apps/cli");
        assert_eq!(resolved.paths.shared_accounts_namespace, "shared/accounts");
        assert_eq!(
            resolved.paths.shared_identities_namespace,
            "shared/identities"
        );
        assert_eq!(
            resolved.paths.workspace_config_path,
            PathBuf::from("/workspaces/radroots-cli/infra/local/runtime/radroots/config.toml")
        );
        assert_eq!(
            resolved.paths.app_data_root,
            PathBuf::from("/home/tester/.radroots/data/apps/cli")
        );
        assert_eq!(
            resolved.paths.allowed_profiles,
            vec!["interactive_user".to_owned(), "repo_local".to_owned(),]
        );
    }

    #[test]
    fn windows_roots_use_native_user_directories() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment {
            values: BTreeMap::new(),
            current_dir: PathBuf::from(r"C:\workspaces\radroots-cli"),
            path_resolver: RadrootsPathResolver::new(
                RadrootsPlatform::Windows,
                RadrootsHostEnvironment {
                    appdata_dir: Some(PathBuf::from(r"C:\Users\tester\AppData\Roaming")),
                    localappdata_dir: Some(PathBuf::from(r"C:\Users\tester\AppData\Local")),
                    ..RadrootsHostEnvironment::default()
                },
            ),
            stdin_tty: false,
            stdout_tty: false,
        };

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");

        assert_eq!(
            resolved.paths.app_config_path,
            PathBuf::from(r"C:\Users\tester\AppData\Roaming")
                .join("Radroots")
                .join("config")
                .join("apps")
                .join("cli")
                .join("config.toml")
        );
        assert_eq!(
            resolved.paths.app_data_root,
            PathBuf::from(r"C:\Users\tester\AppData\Local")
                .join("Radroots")
                .join("data")
                .join("apps")
                .join("cli")
        );
        assert_eq!(
            resolved.paths.shared_accounts_data_root,
            PathBuf::from(r"C:\Users\tester\AppData\Local")
                .join("Radroots")
                .join("data")
                .join("shared")
                .join("accounts")
        );
        assert_eq!(
            resolved.paths.default_identity_path,
            PathBuf::from(r"C:\Users\tester\AppData\Roaming")
                .join("Radroots")
                .join("secrets")
                .join("shared")
                .join("identities")
                .join("default.json")
        );
    }

    #[test]
    fn repo_local_profile_uses_explicit_repo_local_root() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([
            (
                "RADROOTS_CLI_PATHS_PROFILE".to_owned(),
                "repo_local".to_owned(),
            ),
            (
                "RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT".to_owned(),
                ".local/radroots/dev".to_owned(),
            ),
        ]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");

        assert_eq!(resolved.paths.profile, "repo_local");
        assert_eq!(
            resolved.paths.profile_source,
            "process_env:RADROOTS_CLI_PATHS_PROFILE"
        );
        assert_eq!(resolved.paths.root_source, "repo_local_root");
        assert_eq!(
            resolved.paths.repo_local_root,
            Some(PathBuf::from(
                "/workspaces/radroots-cli/.local/radroots/dev"
            ))
        );
        assert_eq!(
            resolved.paths.repo_local_root_source,
            Some("process_env:RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT".to_owned())
        );
        assert_eq!(
            resolved.paths.app_config_path,
            PathBuf::from(
                "/workspaces/radroots-cli/.local/radroots/dev/config/apps/cli/config.toml"
            )
        );
        assert_eq!(
            resolved.paths.workspace_config_path,
            PathBuf::from("/workspaces/radroots-cli/.local/radroots/dev/config.toml")
        );
        assert_eq!(
            resolved.paths.app_data_root,
            PathBuf::from("/workspaces/radroots-cli/.local/radroots/dev/data/apps/cli")
        );
        assert_eq!(
            resolved.paths.app_logs_root,
            PathBuf::from("/workspaces/radroots-cli/.local/radroots/dev/logs/apps/cli")
        );
        assert_eq!(
            resolved.paths.shared_accounts_data_root,
            PathBuf::from("/workspaces/radroots-cli/.local/radroots/dev/data/shared/accounts")
        );
        assert_eq!(
            resolved.paths.default_identity_path,
            PathBuf::from(
                "/workspaces/radroots-cli/.local/radroots/dev/secrets/shared/identities/default.json"
            )
        );
    }

    #[test]
    fn repo_local_profile_requires_explicit_root() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([(
            "RADROOTS_CLI_PATHS_PROFILE".to_owned(),
            "repo_local".to_owned(),
        )]));

        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("repo_local should require an explicit root");
        assert!(
            error
                .to_string()
                .contains("RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT")
        );
    }

    #[test]
    fn env_file_can_select_repo_local_profile() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::new());
        let env_file = parse_env_file_values(
            r#"
RADROOTS_CLI_PATHS_PROFILE=repo_local
RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT=.local/radroots/dev
"#,
            Path::new(".env.test"),
        )
        .expect("parse env file");

        let resolved =
            RuntimeConfig::resolve_with_env_file(&args, &env, &env_file).expect("resolve config");
        assert_eq!(resolved.paths.profile, "repo_local");
        assert_eq!(
            resolved.paths.app_data_root,
            PathBuf::from("/workspaces/radroots-cli/.local/radroots/dev/data/apps/cli")
        );
        assert_eq!(
            resolved.paths.workspace_config_path,
            PathBuf::from("/workspaces/radroots-cli/.local/radroots/dev/config.toml")
        );
    }

    #[test]
    fn unknown_env_file_variable_fails() {
        let error = parse_env_file_values(
            "RADROOTS_CLI_LOGGING_FILTRE=debug\n",
            Path::new(".env.test"),
        )
        .expect_err("unknown env variable");
        assert!(
            error
                .to_string()
                .contains("unknown environment variable `RADROOTS_CLI_LOGGING_FILTRE`")
        );
    }

    #[test]
    fn env_output_accepts_ndjson() {
        let args = CliArgs::parse_from(["radroots", "config", "show"]);
        let env = MapEnvironment::new(BTreeMap::from([(
            "RADROOTS_OUTPUT".to_owned(),
            "ndjson".to_owned(),
        )]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(resolved.output.format, OutputFormat::Ndjson);
    }
}
