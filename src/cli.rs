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
    #[arg(long, global = true)]
    pub signer_backend: Option<String>,
    #[arg(long, global = true)]
    pub myc_executable: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Identity(IdentityArgs),
    Myc(MycArgs),
    Runtime(RuntimeArgs),
    Signer(SignerArgs),
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

#[derive(Debug, Clone, Args)]
pub struct IdentityArgs {
    #[command(subcommand)]
    pub command: IdentityCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum IdentityCommand {
    Init,
    Show,
}

#[derive(Debug, Clone, Args)]
pub struct MycArgs {
    #[command(subcommand)]
    pub command: MycCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MycCommand {
    Status,
}

#[derive(Debug, Clone, Args)]
pub struct SignerArgs {
    #[command(subcommand)]
    pub command: SignerCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SignerCommand {
    Status,
}

#[cfg(test)]
mod tests {
    use super::{CliArgs, Command, IdentityCommand, MycCommand, RuntimeCommand, SignerCommand};
    use clap::Parser;

    #[test]
    fn parses_runtime_show_command() {
        let parsed = CliArgs::parse_from(["radroots", "runtime", "show"]);
        match parsed.command {
            Command::Identity(_) | Command::Myc(_) | Command::Signer(_) => {
                panic!("unexpected command variant")
            }
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
        assert_eq!(parsed.signer_backend.as_deref(), Some("myc"));
        assert_eq!(
            parsed
                .myc_executable
                .as_deref()
                .and_then(|path| path.to_str()),
            Some("bin/myc")
        );
    }

    #[test]
    fn parses_identity_commands() {
        let init = CliArgs::parse_from(["radroots", "identity", "init"]);
        match init.command {
            Command::Identity(identity) => match identity.command {
                IdentityCommand::Init => {}
                IdentityCommand::Show => panic!("unexpected identity subcommand"),
            },
            Command::Myc(_) | Command::Runtime(_) | Command::Signer(_) => {
                panic!("unexpected command variant")
            }
        }

        let show = CliArgs::parse_from(["radroots", "identity", "show"]);
        match show.command {
            Command::Identity(identity) => match identity.command {
                IdentityCommand::Show => {}
                IdentityCommand::Init => panic!("unexpected identity subcommand"),
            },
            Command::Myc(_) | Command::Runtime(_) | Command::Signer(_) => {
                panic!("unexpected command variant")
            }
        }
    }

    #[test]
    fn parses_signer_status() {
        let parsed = CliArgs::parse_from(["radroots", "signer", "status"]);
        match parsed.command {
            Command::Signer(signer) => match signer.command {
                SignerCommand::Status => {}
            },
            Command::Identity(_) | Command::Myc(_) | Command::Runtime(_) => {
                panic!("unexpected command variant")
            }
        }
    }

    #[test]
    fn parses_myc_status() {
        let parsed = CliArgs::parse_from(["radroots", "myc", "status"]);
        match parsed.command {
            Command::Myc(myc) => match myc.command {
                MycCommand::Status => {}
            },
            Command::Identity(_) | Command::Runtime(_) | Command::Signer(_) => {
                panic!("unexpected command variant")
            }
        }
    }
}
