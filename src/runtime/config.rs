use std::path::PathBuf;

use crate::cli::CliArgs;
use crate::runtime::RuntimeError;

const DEFAULT_LOG_FILTER: &str = "info";
const ENV_OUTPUT: &str = "RADROOTS_OUTPUT";
const ENV_LOG_FILTER: &str = "RADROOTS_LOG_FILTER";
const ENV_LOG_DIR: &str = "RADROOTS_LOG_DIR";
const ENV_LOG_STDOUT: &str = "RADROOTS_LOG_STDOUT";
const ENV_IDENTITY_PATH: &str = "RADROOTS_IDENTITY_PATH";
const ENV_IDENTITY_ALLOW_GENERATE: &str = "RADROOTS_IDENTITY_ALLOW_GENERATE";
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
    pub allow_generate: bool,
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
        Self::resolve(args, &SystemEnvironment)
    }

    pub fn resolve(args: &CliArgs, env: &dyn Environment) -> Result<Self, RuntimeError> {
        Ok(Self {
            output_format: resolve_output_format(args, env)?,
            logging: LoggingConfig {
                filter: args
                    .log_filter
                    .clone()
                    .or_else(|| env.var(ENV_LOG_FILTER))
                    .unwrap_or_else(|| DEFAULT_LOG_FILTER.to_owned()),
                directory: args
                    .log_dir
                    .clone()
                    .or_else(|| env.var(ENV_LOG_DIR).map(PathBuf::from)),
                stdout: resolve_bool_pair(
                    args.log_stdout,
                    args.no_log_stdout,
                    ENV_LOG_STDOUT,
                    false,
                    env,
                    "--log-stdout",
                    "--no-log-stdout",
                )?,
            },
            identity: IdentityConfig {
                path: args
                    .identity_path
                    .clone()
                    .or_else(|| env.var(ENV_IDENTITY_PATH).map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("identity.json")),
                allow_generate: resolve_bool_pair(
                    args.allow_generate_identity,
                    args.no_allow_generate_identity,
                    ENV_IDENTITY_ALLOW_GENERATE,
                    false,
                    env,
                    "--allow-generate-identity",
                    "--no-allow-generate-identity",
                )?,
            },
            signer: SignerConfig {
                backend: args
                    .signer_backend
                    .clone()
                    .or_else(|| env.var(ENV_SIGNER_BACKEND))
                    .map(parse_signer_backend)
                    .transpose()?
                    .unwrap_or(SignerBackend::Local),
            },
            myc: MycConfig {
                executable: args
                    .myc_executable
                    .clone()
                    .or_else(|| env.var(ENV_MYC_EXECUTABLE).map(PathBuf::from))
                    .unwrap_or_else(|| PathBuf::from("myc")),
            },
        })
    }
}

fn resolve_output_format(
    args: &CliArgs,
    env: &dyn Environment,
) -> Result<OutputFormat, RuntimeError> {
    if args.json {
        return Ok(OutputFormat::Json);
    }
    match env.var(ENV_OUTPUT) {
        Some(value) => parse_output_format(value.as_str()),
        None => Ok(OutputFormat::Human),
    }
}

fn resolve_bool_pair(
    positive_flag: bool,
    negative_flag: bool,
    env_key: &str,
    default: bool,
    env: &dyn Environment,
    positive_label: &str,
    negative_label: &str,
) -> Result<bool, RuntimeError> {
    match (positive_flag, negative_flag) {
        (true, true) => Err(RuntimeError::Config(format!(
            "flags {positive_label} and {negative_label} cannot be used together"
        ))),
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        (false, false) => match env.var(env_key) {
            Some(value) => parse_bool_env(env_key, value.as_str()),
            None => Ok(default),
        },
    }
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
    use super::{Environment, OutputFormat, RuntimeConfig, SignerBackend};
    use crate::cli::CliArgs;
    use clap::Parser;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

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
            "--allow-generate-identity",
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
            (
                "RADROOTS_IDENTITY_ALLOW_GENERATE".to_owned(),
                "false".to_owned(),
            ),
            ("RADROOTS_SIGNER_BACKEND".to_owned(), "myc".to_owned()),
            ("RADROOTS_MYC_EXECUTABLE".to_owned(), "env-myc".to_owned()),
        ]));

        let resolved = RuntimeConfig::resolve(&args, &env).expect("resolve runtime config");
        assert_eq!(resolved.output_format, OutputFormat::Json);
        assert_eq!(resolved.logging.filter, "debug");
        assert!(resolved.logging.stdout);
        assert_eq!(
            resolved.identity.path,
            PathBuf::from("custom-identity.json")
        );
        assert!(resolved.identity.allow_generate);
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
            (
                "RADROOTS_IDENTITY_ALLOW_GENERATE".to_owned(),
                "true".to_owned(),
            ),
            ("RADROOTS_SIGNER_BACKEND".to_owned(), "myc".to_owned()),
            ("RADROOTS_MYC_EXECUTABLE".to_owned(), "bin/myc".to_owned()),
        ]));

        let resolved = RuntimeConfig::resolve(&args, &env).expect("resolve runtime config");
        assert_eq!(resolved.output_format, OutputFormat::Json);
        assert_eq!(resolved.logging.filter, "debug,cli=trace");
        assert_eq!(
            resolved.logging.directory,
            Some(PathBuf::from("logs/runtime"))
        );
        assert!(resolved.logging.stdout);
        assert_eq!(resolved.identity.path, PathBuf::from("state/identity.json"));
        assert!(resolved.identity.allow_generate);
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
        let error = RuntimeConfig::resolve(&args, &env).expect_err("conflicting flags");
        assert!(error.to_string().contains("cannot be used together"));
    }

    #[test]
    fn invalid_environment_value_fails() {
        let args = CliArgs::parse_from(["radroots", "runtime", "show"]);
        let env = MapEnvironment(BTreeMap::from([(
            "RADROOTS_LOG_STDOUT".to_owned(),
            "maybe".to_owned(),
        )]));
        let error = RuntimeConfig::resolve(&args, &env).expect_err("invalid bool");
        assert!(error.to_string().contains("RADROOTS_LOG_STDOUT"));
    }
}
