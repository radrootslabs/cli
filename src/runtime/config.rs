use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::cli::CliArgs;
use crate::runtime::RuntimeError;

const DEFAULT_LOG_FILTER: &str = "info";
const ENV_FILE_PATH: &str = "RADROOTS_ENV_FILE";
const ENV_OUTPUT: &str = "RADROOTS_OUTPUT";
const ENV_CLI_LOG_FILTER: &str = "RADROOTS_CLI_LOGGING_FILTER";
const ENV_CLI_LOG_DIR: &str = "RADROOTS_CLI_LOGGING_OUTPUT_DIR";
const ENV_CLI_LOG_STDOUT: &str = "RADROOTS_CLI_LOGGING_STDOUT";
const ENV_LOG_FILTER: &str = "RADROOTS_LOG_FILTER";
const ENV_LOG_DIR: &str = "RADROOTS_LOG_DIR";
const ENV_LOG_STDOUT: &str = "RADROOTS_LOG_STDOUT";
const ENV_IDENTITY_PATH: &str = "RADROOTS_IDENTITY_PATH";
const ENV_SIGNER_BACKEND: &str = "RADROOTS_SIGNER_BACKEND";
const ENV_MYC_EXECUTABLE: &str = "RADROOTS_MYC_EXECUTABLE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

impl OutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Json => "json",
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MycConfig {
    pub executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub output_format: OutputFormat,
    pub logging: LoggingConfig,
    pub identity: IdentityConfig,
    pub signer: SignerConfig,
    pub myc: MycConfig,
}

#[derive(Debug, Default)]
struct EnvFileValues(BTreeMap<String, String>);

pub trait Environment {
    fn var(&self, key: &str) -> Option<String>;
}

pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
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
        Ok(Self {
            output_format: resolve_output_format(args, env, env_file)?,
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
            identity: IdentityConfig {
                path: args
                    .identity_path
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_IDENTITY_PATH]).map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("identity.json")),
            },
            signer: SignerConfig {
                backend: args
                    .signer_backend
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_SIGNER_BACKEND]))
                    .map(parse_signer_backend)
                    .transpose()?
                    .unwrap_or(SignerBackend::Local),
            },
            myc: MycConfig {
                executable: args
                    .myc_executable
                    .clone()
                    .or_else(|| env_value(env, env_file, &[ENV_MYC_EXECUTABLE]).map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("myc")),
            },
        })
    }
}

fn resolve_env_file_path(args: &CliArgs, env: &dyn Environment) -> Option<PathBuf> {
    args.env_file
        .clone()
        .or_else(|| env.var(ENV_FILE_PATH).map(PathBuf::from))
}

fn resolve_output_format(
    args: &CliArgs,
    env: &dyn Environment,
    env_file: &EnvFileValues,
) -> Result<OutputFormat, RuntimeError> {
    if args.json {
        return Ok(OutputFormat::Json);
    }
    match env_value(env, env_file, &[ENV_OUTPUT]) {
        Some(value) => parse_output_format(value.as_str()),
        None => Ok(OutputFormat::Human),
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
        other => Err(RuntimeError::Config(format!(
            "{ENV_OUTPUT} must be `human` or `json`, got `{other}`"
        ))),
    }
}

fn parse_signer_backend(value: String) -> Result<SignerBackend, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "local" => Ok(SignerBackend::Local),
        "myc" => Ok(SignerBackend::Myc),
        other => Err(RuntimeError::Config(format!(
            "{ENV_SIGNER_BACKEND} or --signer-backend must be `local` or `myc`, got `{other}`"
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
        EnvFileValues, Environment, OutputFormat, RuntimeConfig, SignerBackend,
        parse_env_file_values,
    };
    use crate::cli::CliArgs;
    use clap::Parser;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    struct MapEnvironment(BTreeMap<String, String>);

    impl Environment for MapEnvironment {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn flags_override_environment_values() {
        let args = CliArgs::parse_from([
            "radroots",
            "--json",
            "--log-filter",
            "debug",
            "--log-stdout",
            "--identity-path",
            "custom-identity.json",
            "--signer-backend",
            "local",
            "--myc-executable",
            "bin/myc-cli",
            "runtime",
            "show",
        ]);
        let env = MapEnvironment(BTreeMap::from([
            ("RADROOTS_OUTPUT".to_owned(), "human".to_owned()),
            ("RADROOTS_LOG_FILTER".to_owned(), "trace".to_owned()),
            ("RADROOTS_LOG_STDOUT".to_owned(), "false".to_owned()),
            (
                "RADROOTS_IDENTITY_PATH".to_owned(),
                "env-identity.json".to_owned(),
            ),
            ("RADROOTS_SIGNER_BACKEND".to_owned(), "myc".to_owned()),
            ("RADROOTS_MYC_EXECUTABLE".to_owned(), "env-myc".to_owned()),
        ]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(resolved.output_format, OutputFormat::Json);
        assert_eq!(resolved.logging.filter, "debug");
        assert!(resolved.logging.stdout);
        assert_eq!(
            resolved.identity.path,
            PathBuf::from("custom-identity.json")
        );
        assert_eq!(resolved.signer.backend, SignerBackend::Local);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc-cli"));
    }

    #[test]
    fn environment_values_fill_missing_flags() {
        let args = CliArgs::parse_from(["radroots", "runtime", "show"]);
        let env = MapEnvironment(BTreeMap::from([
            ("RADROOTS_OUTPUT".to_owned(), "json".to_owned()),
            (
                "RADROOTS_LOG_FILTER".to_owned(),
                "debug,cli=trace".to_owned(),
            ),
            ("RADROOTS_LOG_DIR".to_owned(), "logs/runtime".to_owned()),
            ("RADROOTS_LOG_STDOUT".to_owned(), "true".to_owned()),
            (
                "RADROOTS_IDENTITY_PATH".to_owned(),
                "state/identity.json".to_owned(),
            ),
            ("RADROOTS_SIGNER_BACKEND".to_owned(), "myc".to_owned()),
            ("RADROOTS_MYC_EXECUTABLE".to_owned(), "bin/myc".to_owned()),
        ]));

        let resolved = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect("resolve runtime config");
        assert_eq!(resolved.output_format, OutputFormat::Json);
        assert_eq!(resolved.logging.filter, "debug,cli=trace");
        assert_eq!(
            resolved.logging.directory,
            Some(PathBuf::from("logs/runtime"))
        );
        assert!(resolved.logging.stdout);
        assert_eq!(resolved.identity.path, PathBuf::from("state/identity.json"));
        assert_eq!(resolved.signer.backend, SignerBackend::Myc);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc"));
    }

    #[test]
    fn conflicting_boolean_flags_fail() {
        let args = CliArgs::parse_from([
            "radroots",
            "--log-stdout",
            "--no-log-stdout",
            "runtime",
            "show",
        ]);
        let env = MapEnvironment(BTreeMap::new());
        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("conflicting flags");
        assert!(error.to_string().contains("cannot be used together"));
    }

    #[test]
    fn invalid_environment_value_fails() {
        let args = CliArgs::parse_from(["radroots", "runtime", "show"]);
        let env = MapEnvironment(BTreeMap::from([(
            "RADROOTS_LOG_STDOUT".to_owned(),
            "maybe".to_owned(),
        )]));
        let error = RuntimeConfig::resolve_with_env_file(&args, &env, &EnvFileValues::default())
            .expect_err("invalid bool");
        assert!(error.to_string().contains("RADROOTS_LOG_STDOUT"));
    }

    #[test]
    fn env_file_values_fill_missing_flags() {
        let args = CliArgs::parse_from(["radroots", "runtime", "show"]);
        let env = MapEnvironment(BTreeMap::new());
        let env_file = parse_env_file_values(
            r#"
RADROOTS_OUTPUT=json
RADROOTS_CLI_LOGGING_FILTER="debug,radroots_cli=trace"
RADROOTS_CLI_LOGGING_OUTPUT_DIR=/tmp/radroots-cli-logs
RADROOTS_CLI_LOGGING_STDOUT=false
RADROOTS_IDENTITY_PATH=state/identity.json
RADROOTS_SIGNER_BACKEND=myc
RADROOTS_MYC_EXECUTABLE=bin/myc
"#,
            Path::new(".env.test"),
        )
        .expect("parse env file");

        let resolved =
            RuntimeConfig::resolve_with_env_file(&args, &env, &env_file).expect("resolve config");
        assert_eq!(resolved.output_format, OutputFormat::Json);
        assert_eq!(resolved.logging.filter, "debug,radroots_cli=trace");
        assert_eq!(
            resolved.logging.directory,
            Some(PathBuf::from("/tmp/radroots-cli-logs"))
        );
        assert!(!resolved.logging.stdout);
        assert_eq!(resolved.identity.path, PathBuf::from("state/identity.json"));
        assert_eq!(resolved.signer.backend, SignerBackend::Myc);
        assert_eq!(resolved.myc.executable, PathBuf::from("bin/myc"));
    }

    #[test]
    fn process_environment_overrides_env_file_values() {
        let args = CliArgs::parse_from(["radroots", "runtime", "show"]);
        let env = MapEnvironment(BTreeMap::from([
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
        assert_eq!(resolved.logging.filter, "info");
        assert!(resolved.logging.stdout);
    }
}
