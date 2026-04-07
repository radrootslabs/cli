use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser, Clone)]
#[command(name = "radroots")]
#[command(version)]
pub struct CliArgs {
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub json: bool,
    #[arg(long = "env-file", global = true)]
    pub env_file: Option<PathBuf>,
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
    Account(AccountArgs),
    Config(ConfigArgs),
    Doctor,
    Find(FindArgs),
    Job(JobArgs),
    Listing(ListingArgs),
    Local(LocalArgs),
    Myc(MycArgs),
    Net(NetArgs),
    Order(OrderArgs),
    Relay(RelayArgs),
    Rpc(RpcArgs),
    Signer(SignerArgs),
    Sync(SyncArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    Show,
}

#[derive(Debug, Clone, Args)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub command: AccountCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AccountCommand {
    New,
    Whoami,
    Ls,
    Use(AccountUseArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AccountUseArgs {
    pub selector: String,
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

#[derive(Debug, Clone, Args)]
pub struct RelayArgs {
    #[command(subcommand)]
    pub command: RelayCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RelayCommand {
    Ls,
}

#[derive(Debug, Clone, Args)]
pub struct NetArgs {
    #[command(subcommand)]
    pub command: NetCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum NetCommand {
    Status,
}

#[derive(Debug, Clone, Args)]
pub struct LocalArgs {
    #[command(subcommand)]
    pub command: LocalCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum LocalCommand {
    Init,
    Status,
    Export,
    Backup,
}

#[derive(Debug, Clone, Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: SyncCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SyncCommand {
    Status,
    Pull,
    Push,
    Watch,
}

#[derive(Debug, Clone, Args)]
pub struct FindArgs {
    #[arg(value_name = "query", num_args = 1..)]
    pub query: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ListingArgs {
    #[command(subcommand)]
    pub command: ListingCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ListingCommand {
    New,
    Validate,
    Get(RecordKeyArgs),
    Publish,
    Update(RecordKeyArgs),
    Archive(RecordKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct JobArgs {
    #[command(subcommand)]
    pub command: JobCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum JobCommand {
    Ls,
    Get(RecordKeyArgs),
    Watch(RecordKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RpcArgs {
    #[command(subcommand)]
    pub command: RpcCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RpcCommand {
    Status,
    Sessions,
}

#[derive(Debug, Clone, Args)]
pub struct OrderArgs {
    #[command(subcommand)]
    pub command: OrderCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderCommand {
    New,
    Get(RecordKeyArgs),
    Ls,
    Submit,
    Watch(RecordKeyArgs),
    Cancel(RecordKeyArgs),
    History,
}

#[derive(Debug, Clone, Args)]
pub struct RecordKeyArgs {
    pub key: String,
}

#[cfg(test)]
mod tests {
    use super::{
        AccountCommand, CliArgs, Command, ConfigCommand, JobCommand, ListingCommand, LocalCommand,
        MycCommand, NetCommand, OrderCommand, RelayCommand, RpcCommand, SignerCommand, SyncCommand,
    };
    use clap::Parser;

    #[test]
    fn parses_config_show_command() {
        let parsed = CliArgs::parse_from(["radroots", "config", "show"]);
        match parsed.command {
            Command::Config(config) => match config.command {
                ConfigCommand::Show => {}
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_global_runtime_flags() {
        let parsed = CliArgs::parse_from([
            "radroots",
            "--json",
            "--env-file",
            ".env.local",
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
            "config",
            "show",
        ]);
        assert!(parsed.json);
        assert_eq!(
            parsed.env_file.as_deref().and_then(|path| path.to_str()),
            Some(".env.local")
        );
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
    fn parses_account_commands() {
        let new = CliArgs::parse_from(["radroots", "account", "new"]);
        match new.command {
            Command::Account(account) => match account.command {
                AccountCommand::New => {}
                _ => panic!("unexpected account subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let whoami = CliArgs::parse_from(["radroots", "account", "whoami"]);
        match whoami.command {
            Command::Account(account) => match account.command {
                AccountCommand::Whoami => {}
                _ => panic!("unexpected account subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let ls = CliArgs::parse_from(["radroots", "account", "ls"]);
        match ls.command {
            Command::Account(account) => match account.command {
                AccountCommand::Ls => {}
                _ => panic!("unexpected account subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let use_account = CliArgs::parse_from(["radroots", "account", "use", "market-main"]);
        match use_account.command {
            Command::Account(account) => match account.command {
                AccountCommand::Use(args) => assert_eq!(args.selector, "market-main"),
                _ => panic!("unexpected account subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_signer_status() {
        let parsed = CliArgs::parse_from(["radroots", "signer", "status"]);
        match parsed.command {
            Command::Signer(signer) => match signer.command {
                SignerCommand::Status => {}
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_myc_status() {
        let parsed = CliArgs::parse_from(["radroots", "myc", "status"]);
        match parsed.command {
            Command::Myc(myc) => match myc.command {
                MycCommand::Status => {}
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_v1_command_skeleton() {
        let doctor = CliArgs::parse_from(["radroots", "doctor"]);
        assert!(matches!(doctor.command, Command::Doctor));

        let find = CliArgs::parse_from(["radroots", "find", "tomatoes"]);
        match find.command {
            Command::Find(args) => assert_eq!(args.query, vec!["tomatoes"]),
            _ => panic!("unexpected command variant"),
        }

        let relay = CliArgs::parse_from(["radroots", "relay", "ls"]);
        match relay.command {
            Command::Relay(args) => match args.command {
                RelayCommand::Ls => {}
            },
            _ => panic!("unexpected command variant"),
        }

        let net = CliArgs::parse_from(["radroots", "net", "status"]);
        match net.command {
            Command::Net(args) => match args.command {
                NetCommand::Status => {}
            },
            _ => panic!("unexpected command variant"),
        }

        let local = CliArgs::parse_from(["radroots", "local", "init"]);
        match local.command {
            Command::Local(args) => match args.command {
                LocalCommand::Init => {}
                _ => panic!("unexpected local subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let sync = CliArgs::parse_from(["radroots", "sync", "status"]);
        match sync.command {
            Command::Sync(args) => match args.command {
                SyncCommand::Status => {}
                _ => panic!("unexpected sync subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing = CliArgs::parse_from(["radroots", "listing", "get", "lst_123"]);
        match listing.command {
            Command::Listing(args) => match args.command {
                ListingCommand::Get(key) => assert_eq!(key.key, "lst_123"),
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let job = CliArgs::parse_from(["radroots", "job", "get", "job_123"]);
        match job.command {
            Command::Job(args) => match args.command {
                JobCommand::Get(key) => assert_eq!(key.key, "job_123"),
                _ => panic!("unexpected job subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let rpc = CliArgs::parse_from(["radroots", "rpc", "status"]);
        match rpc.command {
            Command::Rpc(args) => match args.command {
                RpcCommand::Status => {}
                _ => panic!("unexpected rpc subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order = CliArgs::parse_from(["radroots", "order", "history"]);
        match order.command {
            Command::Order(args) => match args.command {
                OrderCommand::History => {}
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }
    }
}
