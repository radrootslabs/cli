use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use url::Url;

use crate::cli::CliArgs;
use crate::runtime::RuntimeError;

const DEFAULT_LOG_FILTER: &str = "info";
const DEFAULT_ENV_PATH: &str = ".env";
const DEFAULT_WORKSPACE_CONFIG_PATH: &str = ".radroots/config.toml";
const DEFAULT_USER_CONFIG_PATH: &str = ".config/radroots/config.toml";
const DEFAULT_USER_STATE_ROOT: &str = ".local/share/radroots";
const DEFAULT_LOCAL_STATE_DIR: &str = "replica";
const DEFAULT_LOCAL_DB_FILE: &str = "replica.sqlite";
const DEFAULT_LOCAL_BACKUPS_DIR: &str = "backups";
const DEFAULT_LOCAL_EXPORTS_DIR: &str = "exports";
const DEFAULT_RPC_URL: &str = "http://127.0.0.1:7070";
const ENV_FILE_PATH: &str = "RADROOTS_ENV_FILE";
const ENV_OUTPUT: &str = "RADROOTS_OUTPUT";
const ENV_CLI_LOG_FILTER: &str = "RADROOTS_CLI_LOGGING_FILTER";
const ENV_CLI_LOG_DIR: &str = "RADROOTS_CLI_LOGGING_OUTPUT_DIR";
const ENV_CLI_LOG_STDOUT: &str = "RADROOTS_CLI_LOGGING_STDOUT";
const ENV_LOG_FILTER: &str = "RADROOTS_LOG_FILTER";
const ENV_LOG_DIR: &str = "RADROOTS_LOG_DIR";
const ENV_LOG_STDOUT: &str = "RADROOTS_LOG_STDOUT";
const ENV_ACCOUNT: &str = "RADROOTS_ACCOUNT";
const ENV_IDENTITY_PATH: &str = "RADROOTS_IDENTITY_PATH";
const ENV_SIGNER: &str = "RADROOTS_SIGNER";
const ENV_RELAYS: &str = "RADROOTS_RELAYS";
const ENV_MYC_EXECUTABLE: &str = "RADROOTS_MYC_EXECUTABLE";
const ENV_RPC_URL: &str = "RADROOTS_RPC_URL";
const ENV_RPC_BEARER_TOKEN: &str = "RADROOTS_RPC_BEARER_TOKEN";
const SUPPORTED_ENV_FILE_KEYS: &[&str] = &[
    ENV_OUTPUT,
    ENV_CLI_LOG_FILTER,
    ENV_CLI_LOG_DIR,
    ENV_CLI_LOG_STDOUT,
    ENV_LOG_FILTER,
    ENV_LOG_DIR,
    ENV_LOG_STDOUT,
    ENV_ACCOUNT,
    ENV_IDENTITY_PATH,
    ENV_SIGNER,
    ENV_RELAYS,
    ENV_MYC_EXECUTABLE,
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
pub struct RpcConfig {
    pub url: String,
    pub bridge_bearer_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub output: OutputConfig,
    pub paths: PathsConfig,
    pub logging: LoggingConfig,
    pub account: AccountConfig,
    pub identity: IdentityConfig,
    pub signer: SignerConfig,
    pub relay: RelayConfig,
    pub local: LocalConfig,
    pub myc: MycConfig,
    pub rpc: RpcConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathsConfig {
    pub user_config_path: PathBuf,
    pub workspace_config_path: PathBuf,
    pub user_state_root: PathBuf,
}

#[derive(Debug, Default)]
struct EnvFileValues(BTreeMap<String, String>);

#[derive(Debug, Default, Deserialize)]
struct CliConfigFile {
    relay: Option<RelayFileConfig>,
    rpc: Option<RpcFileConfig>,
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

pub trait Environment {
    fn var(&self, key: &str) -> Option<String>;
    fn current_dir(&self) -> Result<PathBuf, RuntimeError>;
    fn home_dir(&self) -> Option<PathBuf>;
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

    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
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
        let paths = resolve_paths(env)?;
        let workspace_config = load_cli_config_file(paths.workspace_config_path.as_path())?;
        let user_config = load_cli_config_file(paths.user_config_path.as_path())?;
        Ok(Self {
            output: OutputConfig {
                format: resolve_output_format(args, env, env_file)?,
                verbosity: resolve_verbosity(args)?,
                color: !args.no_color,
                dry_run: args.dry_run,
            },
            paths: paths.clone(),
            logging: LoggingConfig {
                filter: args
                    .log_filter
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_CLI_LOG_FILTER, ENV_LOG_FILTER]))
                    .unwrap_or_else(|| DEFAULT_LOG_FILTER.to_owned()),
                directory: args.log_dir.clone().or_else(|| {
                    env_value(env, env_file, &[ENV_CLI_LOG_DIR, ENV_LOG_DIR]).map(PathBuf::from)
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
            },
            account: AccountConfig {
                selector: args
                    .account
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_ACCOUNT])),
                store_path: paths.user_state_root.join("accounts/store.json"),
                secrets_dir: paths.user_state_root.join("accounts/secrets"),
            },
            identity: IdentityConfig {
                path: args
                    .identity_path
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_IDENTITY_PATH]).map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("identity.json")),
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
                user_config.as_ref(),
                workspace_config.as_ref(),
            )?,
            local: LocalConfig {
                root: paths.user_state_root.join(DEFAULT_LOCAL_STATE_DIR),
                replica_db_path: paths
                    .user_state_root
                    .join(DEFAULT_LOCAL_STATE_DIR)
                    .join(DEFAULT_LOCAL_DB_FILE),
                backups_dir: paths
                    .user_state_root
                    .join(DEFAULT_LOCAL_STATE_DIR)
                    .join(DEFAULT_LOCAL_BACKUPS_DIR),
                exports_dir: paths
                    .user_state_root
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
            rpc: resolve_rpc_config(
                env,
                env_file,
                user_config.as_ref(),
                workspace_config.as_ref(),
            )?,
        })
    }
}

fn resolve_paths(env: &dyn Environment) -> Result<PathsConfig, RuntimeError> {
    let current_dir = env.current_dir()?;
    let home_dir = env.home_dir().ok_or_else(|| {
        RuntimeError::Config(
            "failed to resolve home directory for Radroots config roots".to_owned(),
        )
    })?;

    Ok(PathsConfig {
        user_config_path: home_dir.join(DEFAULT_USER_CONFIG_PATH),
        workspace_config_path: current_dir.join(DEFAULT_WORKSPACE_CONFIG_PATH),
        user_state_root: home_dir.join(DEFAULT_USER_STATE_ROOT),
    })
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
    match (args.json, args.ndjson) {
        (true, true) => {
            return Err(RuntimeError::Config(
                "flags --json and --ndjson cannot be used together".to_owned(),
            ));
        }
        (true, false) => return Ok(OutputFormat::Json),
        (false, true) => return Ok(OutputFormat::Ndjson),
        (false, false) => {}
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
        AccountConfig, EnvFileValues, Environment, OutputConfig, OutputFormat, PathsConfig,
        RelayConfigSource, RelayPublishPolicy, RuntimeConfig, SignerBackend, Verbosity,
        parse_env_file_values,
    };
    use crate::cli::CliArgs;
    use clap::Parser;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    struct MapEnvironment {
        values: BTreeMap<String, String>,
        current_dir: PathBuf,
        home_dir: PathBuf,
    }

    impl MapEnvironment {
        fn new(values: BTreeMap<String, String>) -> Self {
            Self {
                values,
                current_dir: PathBuf::from("/workspaces/radroots-cli"),
                home_dir: PathBuf::from("/home/tester"),
            }
        }
    }

    impl Environment for MapEnvironment {
        fn var(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn current_dir(&self) -> Result<PathBuf, crate::runtime::RuntimeError> {
            Ok(self.current_dir.clone())
        }

        fn home_dir(&self) -> Option<PathBuf> {
            Some(self.home_dir.clone())
        }
    }

    #[test]
    fn flags_override_environment_values() {
        let args = CliArgs::parse_from([
            "radroots",
            "--json",
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
        ]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(
            resolved.output,
            OutputConfig {
                format: OutputFormat::Json,
                verbosity: Verbosity::Verbose,
                color: false,
                dry_run: true,
            }
        );
        assert_eq!(
            resolved.paths,
            PathsConfig {
                user_config_path: PathBuf::from("/home/tester/.config/radroots/config.toml"),
                workspace_config_path: PathBuf::from(
                    "/workspaces/radroots-cli/.radroots/config.toml"
                ),
                user_state_root: PathBuf::from("/home/tester/.local/share/radroots"),
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
                store_path: PathBuf::from("/home/tester/.local/share/radroots/accounts/store.json"),
                secrets_dir: PathBuf::from("/home/tester/.local/share/radroots/accounts/secrets"),
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
            ("RADROOTS_LOG_STDOUT".to_owned(), "true".to_owned()),
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
        assert_eq!(resolved.logging.filter, "debug,cli=trace");
        assert_eq!(
            resolved.logging.directory,
            Some(PathBuf::from("logs/runtime"))
        );
        assert!(resolved.logging.stdout);
        assert_eq!(resolved.account.selector.as_deref(), Some("acct_demo"));
        assert_eq!(resolved.identity.path, PathBuf::from("state/identity.json"));
        assert_eq!(resolved.signer.backend, SignerBackend::Myc);
        assert_eq!(
            resolved.relay.urls,
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
        assert_eq!(resolved.relay.source, RelayConfigSource::Environment);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc"));
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
        fs::create_dir_all(workspace_root.join(".radroots")).expect("workspace config dir");
        fs::create_dir_all(user_home.join(".config/radroots")).expect("user config dir");
        fs::write(
            workspace_root.join(".radroots/config.toml"),
            "[relay]\nurls = [\"wss://relay.workspace\"]\npublish_policy = \"any\"\n",
        )
        .expect("write workspace config");
        fs::write(
            user_home.join(".config/radroots/config.toml"),
            "[relay]\nurls = [\"wss://relay.user\", \"wss://relay.workspace\"]\n",
        )
        .expect("write user config");

        let env = MapEnvironment {
            values: BTreeMap::new(),
            current_dir: workspace_root,
            home_dir: user_home,
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
            resolved.paths.user_config_path,
            PathBuf::from("/home/tester/.config/radroots/config.toml")
        );
        assert_eq!(
            resolved.paths.workspace_config_path,
            PathBuf::from("/workspaces/radroots-cli/.radroots/config.toml")
        );
        assert_eq!(
            resolved.paths.user_state_root,
            PathBuf::from("/home/tester/.local/share/radroots")
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
