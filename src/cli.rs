use clap::{ArgAction, Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::runtime::config::OutputFormat;

#[derive(Debug, Parser, Clone)]
#[command(name = "radroots")]
#[command(version)]
pub struct CliArgs {
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub json: bool,
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub ndjson: bool,
    #[arg(long = "env-file", global = true)]
    pub env_file: Option<PathBuf>,
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub quiet: bool,
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub verbose: bool,
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub trace: bool,
    #[arg(long = "dry-run", global = true, action = ArgAction::SetTrue)]
    pub dry_run: bool,
    #[arg(long = "no-color", global = true, action = ArgAction::SetTrue)]
    pub no_color: bool,
    #[arg(long, global = true)]
    pub log_filter: Option<String>,
    #[arg(long, global = true)]
    pub log_dir: Option<PathBuf>,
    #[arg(long = "log-stdout", global = true, action = ArgAction::SetTrue)]
    pub log_stdout: bool,
    #[arg(long = "no-log-stdout", global = true, action = ArgAction::SetTrue)]
    pub no_log_stdout: bool,
    #[arg(long, global = true)]
    pub account: Option<String>,
    #[arg(long, global = true)]
    pub identity_path: Option<PathBuf>,
    #[arg(long, global = true)]
    pub signer: Option<String>,
    #[arg(long, global = true)]
    pub relay: Vec<String>,
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

impl Command {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Account(account) => match account.command {
                AccountCommand::New => "account new",
                AccountCommand::Whoami => "account whoami",
                AccountCommand::Ls => "account ls",
                AccountCommand::Use(_) => "account use",
            },
            Self::Config(config) => match config.command {
                ConfigCommand::Show => "config show",
            },
            Self::Doctor => "doctor",
            Self::Find(_) => "find",
            Self::Job(job) => match job.command {
                JobCommand::Ls => "job ls",
                JobCommand::Get(_) => "job get",
                JobCommand::Watch(_) => "job watch",
            },
            Self::Listing(listing) => match listing.command {
                ListingCommand::New(_) => "listing new",
                ListingCommand::Validate(_) => "listing validate",
                ListingCommand::Get(_) => "listing get",
                ListingCommand::Publish(_) => "listing publish",
                ListingCommand::Update(_) => "listing update",
                ListingCommand::Archive(_) => "listing archive",
            },
            Self::Local(local) => match local.command {
                LocalCommand::Init => "local init",
                LocalCommand::Status => "local status",
                LocalCommand::Export(_) => "local export",
                LocalCommand::Backup(_) => "local backup",
            },
            Self::Myc(myc) => match myc.command {
                MycCommand::Status => "myc status",
            },
            Self::Net(net) => match net.command {
                NetCommand::Status => "net status",
            },
            Self::Order(order) => match order.command {
                OrderCommand::New => "order new",
                OrderCommand::Get(_) => "order get",
                OrderCommand::Ls => "order ls",
                OrderCommand::Submit => "order submit",
                OrderCommand::Watch(_) => "order watch",
                OrderCommand::Cancel(_) => "order cancel",
                OrderCommand::History => "order history",
            },
            Self::Relay(relay) => match relay.command {
                RelayCommand::Ls => "relay ls",
            },
            Self::Rpc(rpc) => match rpc.command {
                RpcCommand::Status => "rpc status",
                RpcCommand::Sessions => "rpc sessions",
            },
            Self::Signer(signer) => match signer.command {
                SignerCommand::Status => "signer status",
            },
            Self::Sync(sync) => match sync.command {
                SyncCommand::Status => "sync status",
                SyncCommand::Pull => "sync pull",
                SyncCommand::Push => "sync push",
                SyncCommand::Watch(_) => "sync watch",
            },
        }
    }

    pub fn supports_output_format(&self, format: OutputFormat) -> bool {
        match format {
            OutputFormat::Human | OutputFormat::Json => true,
            OutputFormat::Ndjson => matches!(
                self,
                Self::Account(AccountArgs {
                    command: AccountCommand::Ls,
                }) | Self::Relay(RelayArgs {
                    command: RelayCommand::Ls,
                }) | Self::Job(JobArgs {
                    command: JobCommand::Ls,
                }) | Self::Rpc(RpcArgs {
                    command: RpcCommand::Sessions,
                }) | Self::Order(OrderArgs {
                    command: OrderCommand::Ls | OrderCommand::History,
                }) | Self::Sync(SyncArgs {
                    command: SyncCommand::Watch(_),
                }) | Self::Find(_)
            ),
        }
    }

    pub fn supports_dry_run(&self) -> bool {
        !matches!(
            self,
            Self::Account(AccountArgs {
                command: AccountCommand::New | AccountCommand::Use(_),
            }) | Self::Local(LocalArgs {
                command: LocalCommand::Init | LocalCommand::Export(_) | LocalCommand::Backup(_),
            }) | Self::Sync(SyncArgs {
                command: SyncCommand::Pull | SyncCommand::Push,
            }) | Self::Listing(ListingArgs {
                command: ListingCommand::New(_)
                    | ListingCommand::Publish(_)
                    | ListingCommand::Update(_)
                    | ListingCommand::Archive(_),
            }) | Self::Order(OrderArgs {
                command: OrderCommand::New | OrderCommand::Submit | OrderCommand::Cancel(_),
            })
        )
    }
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
    Export(LocalExportArgs),
    Backup(LocalBackupArgs),
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum LocalExportFormatArg {
    Json,
    Ndjson,
}

impl LocalExportFormatArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Ndjson => "ndjson",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct LocalExportArgs {
    #[arg(long)]
    pub format: LocalExportFormatArg,
    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct LocalBackupArgs {
    #[arg(long)]
    pub output: PathBuf,
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
    Watch(SyncWatchArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SyncWatchArgs {
    #[arg(long)]
    pub frames: usize,
    #[arg(long, default_value_t = 1_000)]
    pub interval_ms: u64,
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
    New(ListingNewArgs),
    Validate(ListingFileArgs),
    Get(RecordKeyArgs),
    Publish(ListingFileArgs),
    Update(RecordKeyArgs),
    Archive(RecordKeyArgs),
}

#[derive(Debug, Clone, Args, Default)]
pub struct ListingNewArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct ListingFileArgs {
    pub file: PathBuf,
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
        LocalExportFormatArg, MycCommand, NetCommand, OrderCommand, RelayCommand, RpcCommand,
        SignerCommand, SyncCommand, SyncWatchArgs,
    };
    use crate::runtime::config::OutputFormat;
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
            "--verbose",
            "--dry-run",
            "--no-color",
            "--env-file",
            ".env.local",
            "--account",
            "acct_demo",
            "--log-filter",
            "debug,radroots_cli=trace",
            "--log-dir",
            "logs",
            "--log-stdout",
            "--identity-path",
            "identity.local.json",
            "--signer",
            "myc",
            "--relay",
            "wss://relay.one",
            "--relay",
            "wss://relay.two",
            "--myc-executable",
            "bin/myc",
            "config",
            "show",
        ]);
        assert!(parsed.json);
        assert!(parsed.verbose);
        assert!(parsed.dry_run);
        assert!(parsed.no_color);
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
        assert_eq!(parsed.account.as_deref(), Some("acct_demo"));
        assert!(parsed.log_stdout);
        assert_eq!(
            parsed
                .identity_path
                .as_deref()
                .and_then(|path| path.to_str()),
            Some("identity.local.json")
        );
        assert_eq!(parsed.signer.as_deref(), Some("myc"));
        assert_eq!(
            parsed.relay,
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
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

        let local_export = CliArgs::parse_from([
            "radroots",
            "local",
            "export",
            "--format",
            "ndjson",
            "--output",
            "replica.ndjson",
        ]);
        match local_export.command {
            Command::Local(args) => match args.command {
                LocalCommand::Export(export) => {
                    assert!(matches!(export.format, LocalExportFormatArg::Ndjson));
                    assert_eq!(export.output.to_str(), Some("replica.ndjson"));
                }
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

        let sync_watch = CliArgs::parse_from([
            "radroots",
            "sync",
            "watch",
            "--frames",
            "2",
            "--interval-ms",
            "25",
        ]);
        match sync_watch.command {
            Command::Sync(args) => match args.command {
                SyncCommand::Watch(SyncWatchArgs {
                    frames,
                    interval_ms,
                }) => {
                    assert_eq!(frames, 2);
                    assert_eq!(interval_ms, 25);
                }
                _ => panic!("unexpected sync subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing_new = CliArgs::parse_from(["radroots", "listing", "new"]);
        match listing_new.command {
            Command::Listing(args) => match args.command {
                ListingCommand::New(new) => assert!(new.output.is_none()),
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing_validate =
            CliArgs::parse_from(["radroots", "listing", "validate", "draft.toml"]);
        match listing_validate.command {
            Command::Listing(args) => match args.command {
                ListingCommand::Validate(file) => {
                    assert_eq!(file.file.to_str(), Some("draft.toml"));
                }
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing_publish =
            CliArgs::parse_from(["radroots", "listing", "publish", "draft.toml"]);
        match listing_publish.command {
            Command::Listing(args) => match args.command {
                ListingCommand::Publish(file) => {
                    assert_eq!(file.file.to_str(), Some("draft.toml"));
                }
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing_get = CliArgs::parse_from(["radroots", "listing", "get", "lst_123"]);
        match listing_get.command {
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

    #[test]
    fn command_contract_helpers_report_supported_modes() {
        let config_show = CliArgs::parse_from(["radroots", "config", "show"]);
        assert!(
            config_show
                .command
                .supports_output_format(OutputFormat::Human)
        );
        assert!(
            config_show
                .command
                .supports_output_format(OutputFormat::Json)
        );
        assert!(
            !config_show
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );
        assert!(config_show.command.supports_dry_run());

        let account_new = CliArgs::parse_from(["radroots", "account", "new"]);
        assert_eq!(account_new.command.display_name(), "account new");
        assert!(!account_new.command.supports_dry_run());

        let find = CliArgs::parse_from(["radroots", "find", "eggs"]);
        assert!(find.command.supports_output_format(OutputFormat::Ndjson));

        let sync_watch = CliArgs::parse_from(["radroots", "sync", "watch", "--frames", "1"]);
        assert!(
            sync_watch
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );
    }
}
