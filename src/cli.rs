use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser, Clone)]
#[command(name = "radroots")]
#[command(version)]
pub struct CliArgs {
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub json: bool,
    #[arg(long, global = true)]
    pub log_filter: Option<String>,
    #[arg(long, global = true)]
    pub log_dir: Option<PathBuf>,
    #[arg(long = "log-stdout", global = true, action = ArgAction::SetTrue)]
    pub log_stdout: bool,
    #[arg(long = "no-log-stdout", global = true, action = ArgAction::SetTrue)]
    pub no_log_stdout: bool,
    #[arg(long, global = true)]
    pub identity_path: Option<PathBuf>,
    #[arg(long = "allow-generate-identity", global = true, action = ArgAction::SetTrue)]
    pub allow_generate_identity: bool,
    #[arg(long = "no-allow-generate-identity", global = true, action = ArgAction::SetTrue)]
    pub no_allow_generate_identity: bool,
    #[arg(long, global = true)]
    pub signer_backend: Option<String>,
    #[arg(long, global = true)]
    pub myc_executable: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Runtime(RuntimeArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeArgs {
    #[command(subcommand)]
    pub command: RuntimeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RuntimeCommand {
    Show,
}

#[cfg(test)]
mod tests {
    use super::{CliArgs, Command, RuntimeCommand};
    use clap::Parser;

    #[test]
    fn parses_runtime_show_command() {
        let parsed = CliArgs::parse_from(["radroots", "runtime", "show"]);
        match parsed.command {
            Command::Runtime(runtime) => match runtime.command {
                RuntimeCommand::Show => {}
            },
        }
    }

    #[test]
    fn parses_global_runtime_flags() {
        let parsed = CliArgs::parse_from([
            "radroots",
            "--json",
            "--log-filter",
            "debug,radroots_cli=trace",
            "--log-dir",
            "logs",
            "--log-stdout",
            "--identity-path",
            "identity.local.json",
            "--allow-generate-identity",
            "--signer-backend",
            "myc",
            "--myc-executable",
            "bin/myc",
            "runtime",
            "show",
        ]);
        assert!(parsed.json);
        assert_eq!(
            parsed.log_filter.as_deref(),
            Some("debug,radroots_cli=trace")
        );
        assert_eq!(
            parsed.log_dir.as_deref().and_then(|path| path.to_str()),
            Some("logs")
        );
        assert!(parsed.log_stdout);
        assert_eq!(
            parsed
                .identity_path
                .as_deref()
                .and_then(|path| path.to_str()),
            Some("identity.local.json")
        );
        assert!(parsed.allow_generate_identity);
        assert_eq!(parsed.signer_backend.as_deref(), Some("myc"));
        assert_eq!(
            parsed
                .myc_executable
                .as_deref()
                .and_then(|path| path.to_str()),
            Some("bin/myc")
        );
    }
}
