#![allow(dead_code)]

use clap::{
    ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum,
    error::ErrorKind,
};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::runtime::config::OutputFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormatArg {
    Human,
    Json,
    Ndjson,
}

impl OutputFormatArg {
    pub fn as_output_format(self) -> OutputFormat {
        match self {
            Self::Human => OutputFormat::Human,
            Self::Json => OutputFormat::Json,
            Self::Ndjson => OutputFormat::Ndjson,
        }
    }
}

#[derive(Debug, Parser, Clone)]
#[command(name = "radroots")]
#[command(version)]
pub struct CliArgs {
    #[arg(skip)]
    pub output_format: Option<OutputFormatArg>,
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
    #[arg(
        long = "no-input",
        global = true,
        visible_alias = "non-interactive",
        action = ArgAction::SetTrue
    )]
    pub no_input: bool,
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub yes: bool,
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
    #[arg(long = "myc-status-timeout-ms", global = true)]
    pub myc_status_timeout_ms: Option<u64>,
    #[arg(long = "hyf-enabled", global = true, action = ArgAction::SetTrue)]
    pub hyf_enabled: bool,
    #[arg(long = "no-hyf-enabled", global = true, action = ArgAction::SetTrue)]
    pub no_hyf_enabled: bool,
    #[arg(long = "hyf-executable", global = true)]
    pub hyf_executable: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

impl CliArgs {
    pub fn parse() -> Self {
        Self::try_parse().unwrap_or_else(|error| error.exit())
    }

    pub fn try_parse() -> Result<Self, clap::Error> {
        Self::try_parse_from(std::env::args_os())
    }

    #[cfg(test)]
    pub fn parse_from<I, T>(itr: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::try_parse_from(itr).unwrap_or_else(|error| error.exit())
    }

    pub fn try_parse_from<I, T>(itr: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let args = itr.into_iter().map(Into::into).collect::<Vec<_>>();
        let (filtered_args, output_format) = extract_global_output_format(args)?;
        let mut command = Self::build_command();
        let matches = command.try_get_matches_from_mut(filtered_args)?;
        let mut parsed = <Self as FromArgMatches>::from_arg_matches(&matches)?;
        parsed.output_format = output_format;
        Ok(parsed)
    }

    pub fn build_command() -> clap::Command {
        <Self as CommandFactory>::command()
    }

    fn command_error(message: impl Into<String>, kind: ErrorKind) -> clap::Error {
        let mut command = Self::build_command();
        command.error(kind, message.into())
    }
}

fn extract_global_output_format(
    args: Vec<OsString>,
) -> Result<(Vec<OsString>, Option<OutputFormatArg>), clap::Error> {
    let mut iter = args.into_iter();
    let Some(program) = iter.next() else {
        return Ok((Vec::new(), None));
    };

    let mut filtered_args = vec![program];
    let mut output_format = None;
    let mut command_tokens = Vec::new();
    let mut skip_known_global_value = false;

    while let Some(arg) = iter.next() {
        if skip_known_global_value {
            filtered_args.push(arg);
            skip_known_global_value = false;
            continue;
        }

        if let Some((flag, value)) = split_long_option(arg.as_os_str()) {
            if flag == "output" && !matches_local_output_context(command_tokens.as_slice()) {
                output_format = Some(parse_output_format_value(value)?);
                continue;
            }

            if matches_known_global_value_option(flag) {
                filtered_args.push(arg);
                continue;
            }
        }

        if arg == OsStr::new("--output") {
            if matches_local_output_context(command_tokens.as_slice()) {
                filtered_args.push(arg);
                continue;
            }

            let Some(value) = iter.next() else {
                return Err(CliArgs::command_error(
                    "`--output` requires a value",
                    ErrorKind::InvalidValue,
                ));
            };
            output_format = Some(parse_output_format_value(value.as_os_str())?);
            continue;
        }

        if let Some(flag) = long_option_name(arg.as_os_str()) {
            if matches_known_global_value_option(flag) {
                skip_known_global_value = true;
            }
        }

        if let Some(token) = arg.to_str() {
            if !token.starts_with('-') {
                command_tokens.push(token.to_owned());
            }
        }

        filtered_args.push(arg);
    }

    Ok((filtered_args, output_format))
}

fn parse_output_format_value(value: &OsStr) -> Result<OutputFormatArg, clap::Error> {
    let Some(value) = value.to_str() else {
        return Err(CliArgs::command_error(
            "`--output` must be one of: human, json, ndjson",
            ErrorKind::InvalidUtf8,
        ));
    };

    OutputFormatArg::from_str(value, false).map_err(|_| {
        CliArgs::command_error(
            format!("invalid value `{value}` for `--output`; expected one of: human, json, ndjson"),
            ErrorKind::InvalidValue,
        )
    })
}

fn long_option_name(arg: &OsStr) -> Option<&str> {
    let token = arg.to_str()?;
    token
        .strip_prefix("--")
        .map(|rest| rest.split_once('=').map_or(rest, |(flag, _value)| flag))
}

fn split_long_option(arg: &OsStr) -> Option<(&str, &OsStr)> {
    let token = arg.to_str()?;
    let (flag, value) = token.strip_prefix("--")?.split_once('=')?;
    Some((flag, OsStr::new(value)))
}

fn matches_known_global_value_option(flag: &str) -> bool {
    matches!(
        flag,
        "env-file"
            | "log-filter"
            | "log-dir"
            | "account"
            | "identity-path"
            | "signer"
            | "relay"
            | "myc-executable"
            | "hyf-executable"
    )
}

fn matches_local_output_context(command_tokens: &[String]) -> bool {
    matches!(
        command_tokens,
        [local, export, ..] if local == "local" && export == "export"
    ) || matches!(
        command_tokens,
        [local, backup, ..] if local == "local" && backup == "backup"
    ) || matches!(
        command_tokens,
        [listing, new, ..] if listing == "listing" && new == "new"
    ) || matches!(
        command_tokens,
        [sell, add, ..] if sell == "sell" && add == "add"
    )
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    #[command(about = "Create, import, and manage local accounts")]
    Account(AccountArgs),
    #[command(about = "Show effective configuration")]
    Config(ConfigArgs),
    #[command(about = "Check readiness and suggest next steps")]
    Doctor,
    #[command(about = "Set up and publish your farm")]
    Farm(FarmArgs),
    #[command(about = "Advanced search command")]
    Find(FindArgs),
    #[command(about = "Inspect background jobs")]
    Job(JobArgs),
    #[command(about = "Advanced listing commands")]
    Listing(ListingArgs),
    #[command(about = "Manage local market data storage")]
    Local(LocalArgs),
    #[command(about = "Update local market data and search listings")]
    Market(MarketArgs),
    #[command(about = "Show myc status")]
    Myc(MycArgs),
    #[command(about = "Show network posture")]
    Net(NetArgs),
    #[command(about = "Create and manage order requests")]
    Order(OrderArgs),
    #[command(about = "Show relay configuration")]
    Relay(RelayArgs),
    #[command(about = "Show runtime bridge status")]
    Rpc(RpcArgs),
    #[command(about = "Create, check, and publish listings")]
    Sell(SellArgs),
    #[command(about = "Guided first-time setup for sellers and buyers")]
    Setup(SetupArgs),
    Runtime(RuntimeArgs),
    #[command(about = "Show signer readiness")]
    Signer(SignerArgs),
    #[command(about = "Show what is ready and what needs attention")]
    Status,
    #[command(about = "Inspect sync status and watch updates")]
    Sync(SyncArgs),
}

impl Command {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Account(account) => match account.command {
                AccountCommand::New => "account create",
                AccountCommand::Import(_) => "account import",
                AccountCommand::Whoami => "account view",
                AccountCommand::Ls => "account list",
                AccountCommand::Use(_) => "account select",
                AccountCommand::ClearDefault => "account clear-default",
                AccountCommand::Remove(_) => "account remove",
            },
            Self::Config(config) => match config.command {
                ConfigCommand::Show => "config show",
            },
            Self::Doctor => "doctor",
            Self::Farm(farm) => match farm.command {
                FarmCommand::Init(_) => "farm init",
                FarmCommand::Set(_) => "farm set",
                FarmCommand::Publish(_) => "farm publish",
                FarmCommand::Setup(_) => "farm setup",
                FarmCommand::Status(_) => "farm check",
                FarmCommand::Get(_) => "farm show",
            },
            Self::Find(_) => "find",
            Self::Job(job) => match job.command {
                JobCommand::Ls => "job list",
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
            Self::Market(market) => match market.command {
                MarketCommand::Update => "market update",
                MarketCommand::Search(_) => "market search",
                MarketCommand::View(_) => "market view",
            },
            Self::Myc(myc) => match myc.command {
                MycCommand::Status => "myc status",
            },
            Self::Net(net) => match net.command {
                NetCommand::Status => "net status",
            },
            Self::Order(order) => match order.command {
                OrderCommand::New(_) => "order create",
                OrderCommand::Get(_) => "order view",
                OrderCommand::Ls => "order list",
                OrderCommand::Submit(_) => "order submit",
                OrderCommand::Watch(_) => "order watch",
                OrderCommand::Cancel(_) => "order cancel",
                OrderCommand::History => "order history",
            },
            Self::Relay(relay) => match relay.command {
                RelayCommand::Ls => "relay list",
            },
            Self::Rpc(rpc) => match rpc.command {
                RpcCommand::Status => "rpc status",
                RpcCommand::Sessions => "rpc sessions",
            },
            Self::Sell(sell) => match sell.command {
                SellCommand::Add(_) => "sell add",
                SellCommand::Show(_) => "sell show",
                SellCommand::Check(_) => "sell check",
                SellCommand::Publish(_) => "sell publish",
                SellCommand::Update(_) => "sell update",
                SellCommand::Pause(_) => "sell pause",
                SellCommand::Reprice(_) => "sell reprice",
                SellCommand::Restock(_) => "sell restock",
            },
            Self::Setup(args) => match args.role {
                SetupRoleArg::Seller => "setup seller",
                SetupRoleArg::Buyer => "setup buyer",
                SetupRoleArg::Both => "setup both",
            },
            Self::Runtime(runtime) => match &runtime.command {
                RuntimeCommand::Install(_) => "runtime install",
                RuntimeCommand::Uninstall(_) => "runtime uninstall",
                RuntimeCommand::Status(_) => "runtime status",
                RuntimeCommand::Start(_) => "runtime start",
                RuntimeCommand::Stop(_) => "runtime stop",
                RuntimeCommand::Restart(_) => "runtime restart",
                RuntimeCommand::Logs(_) => "runtime logs",
                RuntimeCommand::Config(runtime_config) => match &runtime_config.command {
                    RuntimeConfigCommand::Show(_) => "runtime config show",
                    RuntimeConfigCommand::Set(_) => "runtime config set",
                },
            },
            Self::Signer(signer) => match signer.command {
                SignerCommand::Status => "signer status",
                SignerCommand::Session(_) => "signer session",
            },
            Self::Status => "status",
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
                }) | Self::Job(JobArgs {
                    command: JobCommand::Watch(_),
                }) | Self::Rpc(RpcArgs {
                    command: RpcCommand::Sessions,
                }) | Self::Order(OrderArgs {
                    command: OrderCommand::Ls | OrderCommand::Watch(_) | OrderCommand::History,
                }) | Self::Sync(SyncArgs {
                    command: SyncCommand::Watch(_),
                }) | Self::Find(_)
                    | Self::Market(MarketArgs {
                        command: MarketCommand::Search(_),
                    })
            ),
        }
    }

    pub fn supports_dry_run(&self) -> bool {
        !matches!(
            self,
            Self::Account(AccountArgs {
                command: AccountCommand::New
                    | AccountCommand::Import(_)
                    | AccountCommand::Use(_)
                    | AccountCommand::ClearDefault
                    | AccountCommand::Remove(_),
            }) | Self::Farm(FarmArgs {
                command: FarmCommand::Init(_) | FarmCommand::Set(_) | FarmCommand::Setup(_),
            }) | Self::Local(LocalArgs {
                command: LocalCommand::Init | LocalCommand::Export(_) | LocalCommand::Backup(_),
            }) | Self::Sync(SyncArgs {
                command: SyncCommand::Pull | SyncCommand::Push,
            }) | Self::Listing(ListingArgs {
                command: ListingCommand::New(_),
            }) | Self::Market(MarketArgs {
                command: MarketCommand::Update,
            }) | Self::Order(OrderArgs {
                command: OrderCommand::New(_) | OrderCommand::Cancel(_),
            }) | Self::Sell(SellArgs {
                command: SellCommand::Add(_),
            }) | Self::Setup(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SetupRoleArg {
    Seller,
    Buyer,
    Both,
}

#[derive(Debug, Clone, Args)]
pub struct SetupArgs {
    #[arg(value_enum, default_value = "both")]
    pub role: SetupRoleArg,
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
    #[command(
        name = "create",
        visible_alias = "new",
        about = "Create a local account"
    )]
    New,
    #[command(name = "import", about = "Import a watch-only local account")]
    Import(AccountImportArgs),
    #[command(
        name = "view",
        visible_alias = "whoami",
        about = "Show the selected local account"
    )]
    Whoami,
    #[command(name = "list", visible_alias = "ls", about = "List local accounts")]
    Ls,
    #[command(
        name = "select",
        visible_alias = "use",
        about = "Select a local account"
    )]
    Use(AccountUseArgs),
    #[command(name = "clear-default", about = "Clear the stored default account")]
    ClearDefault,
    #[command(name = "remove", about = "Remove a local account")]
    Remove(AccountRemoveArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AccountImportArgs {
    pub path: PathBuf,
    #[arg(long, action = ArgAction::SetTrue)]
    pub default: bool,
}

#[derive(Debug, Clone, Args)]
pub struct AccountUseArgs {
    pub selector: String,
}

#[derive(Debug, Clone, Args)]
pub struct AccountRemoveArgs {
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
    Session(SignerSessionArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SignerSessionArgs {
    #[command(subcommand)]
    pub command: SignerSessionCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SignerSessionCommand {
    List,
    Show {
        session_id: String,
    },
    ConnectBunker {
        url: String,
    },
    ConnectNostrconnect {
        url: String,
        #[arg(long)]
        client_secret_key: String,
    },
    PublicKey {
        session_id: String,
    },
    Authorize {
        session_id: String,
    },
    RequireAuth {
        session_id: String,
        #[arg(long)]
        auth_url: String,
    },
    Close {
        session_id: String,
    },
}

#[derive(Debug, Clone, Args)]
pub struct RelayArgs {
    #[command(subcommand)]
    pub command: RelayCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RelayCommand {
    #[command(name = "list", visible_alias = "ls", about = "List configured relays")]
    Ls,
}

#[derive(Debug, Clone, Args)]
pub struct FarmArgs {
    #[command(subcommand)]
    pub command: FarmCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmCommand {
    #[command(about = "Create or refresh a farm draft progressively")]
    Init(FarmInitArgs),
    #[command(about = "Set one farm draft field")]
    Set(FarmSetArgs),
    #[command(about = "Publish the current farm draft")]
    Publish(FarmPublishArgs),
    #[command(about = "Create or update a farm draft in one command")]
    Setup(FarmSetupArgs),
    #[command(
        name = "check",
        visible_alias = "status",
        about = "Check farm readiness"
    )]
    Status(FarmScopedArgs),
    #[command(name = "show", visible_alias = "get", about = "Show the farm draft")]
    Get(FarmScopedArgs),
}

#[derive(Debug, Clone, Args, Default)]
pub struct FarmPublishArgs {
    #[arg(long, value_enum)]
    pub scope: Option<FarmScopeArg>,
    #[arg(long = "idempotency-key")]
    pub idempotency_key: Option<String>,
    #[arg(long = "signer-session-id")]
    pub signer_session_id: Option<String>,
    #[arg(long = "print-job", action = ArgAction::SetTrue)]
    pub print_job: bool,
    #[arg(long = "print-event", action = ArgAction::SetTrue)]
    pub print_event: bool,
}

#[derive(Debug, Clone, Args, Default)]
pub struct FarmScopedArgs {
    #[arg(long, value_enum)]
    pub scope: Option<FarmScopeArg>,
}

#[derive(Debug, Clone, Args, Default)]
pub struct FarmInitArgs {
    #[arg(long, value_enum)]
    pub scope: Option<FarmScopeArg>,
    #[arg(long = "farm-d-tag")]
    pub farm_d_tag: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long = "display-name")]
    pub display_name: Option<String>,
    #[arg(long)]
    pub about: Option<String>,
    #[arg(long)]
    pub website: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub banner: Option<String>,
    #[arg(long)]
    pub location: Option<String>,
    #[arg(long)]
    pub city: Option<String>,
    #[arg(long)]
    pub region: Option<String>,
    #[arg(long)]
    pub country: Option<String>,
    #[arg(long = "delivery", visible_alias = "delivery-method")]
    pub delivery_method: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FarmFieldArg {
    Name,
    #[value(name = "display_name", alias = "display-name")]
    DisplayName,
    About,
    Website,
    Picture,
    Banner,
    Location,
    City,
    Region,
    Country,
    Delivery,
}

#[derive(Debug, Clone, Args)]
pub struct FarmSetArgs {
    #[arg(long, value_enum)]
    pub scope: Option<FarmScopeArg>,
    #[arg(value_enum)]
    pub field: FarmFieldArg,
    #[arg(value_name = "value", num_args = 1..)]
    pub value: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmSetupArgs {
    #[arg(long, value_enum)]
    pub scope: Option<FarmScopeArg>,
    #[arg(long = "farm-d-tag")]
    pub farm_d_tag: Option<String>,
    #[arg(long)]
    pub name: String,
    #[arg(long = "display-name")]
    pub display_name: Option<String>,
    #[arg(long)]
    pub about: Option<String>,
    #[arg(long)]
    pub website: Option<String>,
    #[arg(long)]
    pub picture: Option<String>,
    #[arg(long)]
    pub banner: Option<String>,
    #[arg(long)]
    pub location: String,
    #[arg(long)]
    pub city: Option<String>,
    #[arg(long)]
    pub region: Option<String>,
    #[arg(long)]
    pub country: Option<String>,
    #[arg(long = "delivery-method", default_value = "pickup")]
    pub delivery_method: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FarmScopeArg {
    User,
    Workspace,
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
pub struct MarketArgs {
    #[command(subcommand)]
    pub command: MarketCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketCommand {
    #[command(about = "Update local market data")]
    Update,
    #[command(about = "Search listings in local market data")]
    Search(FindArgs),
    #[command(about = "View one published listing")]
    View(RecordKeyArgs),
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
    Publish(ListingMutationArgs),
    Update(ListingMutationArgs),
    Archive(ListingMutationArgs),
}

#[derive(Debug, Clone, Args, Default)]
pub struct ListingNewArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub key: Option<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub category: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "bin-id")]
    pub bin_id: Option<String>,
    #[arg(long = "quantity-amount")]
    pub quantity_amount: Option<String>,
    #[arg(long = "quantity-unit")]
    pub quantity_unit: Option<String>,
    #[arg(long = "price-amount")]
    pub price_amount: Option<String>,
    #[arg(long = "price-currency")]
    pub price_currency: Option<String>,
    #[arg(long = "price-per-amount")]
    pub price_per_amount: Option<String>,
    #[arg(long = "price-per-unit")]
    pub price_per_unit: Option<String>,
    #[arg(long)]
    pub available: Option<String>,
    #[arg(long)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ListingFileArgs {
    pub file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct ListingMutationArgs {
    pub file: PathBuf,
    #[arg(long)]
    pub idempotency_key: Option<String>,
    #[arg(long = "signer-session-id")]
    pub signer_session_id: Option<String>,
    #[arg(long = "print-job", action = ArgAction::SetTrue)]
    pub print_job: bool,
    #[arg(long = "print-event", action = ArgAction::SetTrue)]
    pub print_event: bool,
}

#[derive(Debug, Clone, Args)]
pub struct JobArgs {
    #[command(subcommand)]
    pub command: JobCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum JobCommand {
    #[command(name = "list", visible_alias = "ls", about = "List background jobs")]
    Ls,
    #[command(about = "Show one background job")]
    Get(RecordKeyArgs),
    #[command(about = "Watch a background job")]
    Watch(JobWatchArgs),
}

#[derive(Debug, Clone, Args)]
pub struct JobWatchArgs {
    pub key: String,
    #[arg(long)]
    pub frames: Option<usize>,
    #[arg(long, default_value_t = 1_000)]
    pub interval_ms: u64,
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
pub struct RuntimeArgs {
    #[command(subcommand)]
    pub command: RuntimeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RuntimeCommand {
    Install(RuntimeTargetArgs),
    Uninstall(RuntimeTargetArgs),
    Status(RuntimeTargetArgs),
    Start(RuntimeTargetArgs),
    Stop(RuntimeTargetArgs),
    Restart(RuntimeTargetArgs),
    Logs(RuntimeTargetArgs),
    Config(RuntimeConfigArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeTargetArgs {
    pub runtime: String,
    #[arg(long)]
    pub instance: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeConfigArgs {
    #[command(subcommand)]
    pub command: RuntimeConfigCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RuntimeConfigCommand {
    Show(RuntimeTargetArgs),
    Set(RuntimeConfigSetArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeConfigSetArgs {
    #[command(flatten)]
    pub target: RuntimeTargetArgs,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Args)]
pub struct OrderArgs {
    #[command(subcommand)]
    pub command: OrderCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderCommand {
    #[command(
        name = "create",
        visible_alias = "new",
        about = "Create a local order draft"
    )]
    New(OrderNewArgs),
    #[command(name = "view", visible_alias = "get", about = "Show one order")]
    Get(RecordKeyArgs),
    #[command(name = "list", visible_alias = "ls", about = "List local orders")]
    Ls,
    #[command(about = "Submit a local order draft")]
    Submit(OrderSubmitArgs),
    #[command(about = "Watch a submitted order")]
    Watch(OrderWatchArgs),
    #[command(about = "Explain durable order cancel availability")]
    Cancel(RecordKeyArgs),
    #[command(about = "Show submitted order history")]
    History,
}

#[derive(Debug, Clone, Args, Default)]
pub struct OrderNewArgs {
    #[arg(long)]
    pub listing: Option<String>,
    #[arg(long = "listing-addr")]
    pub listing_addr: Option<String>,
    #[arg(long = "bin")]
    pub bin_id: Option<String>,
    #[arg(long = "qty")]
    pub bin_count: Option<u32>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderSubmitArgs {
    pub key: String,
    #[arg(long, action = ArgAction::SetTrue)]
    pub watch: bool,
    #[arg(long)]
    pub idempotency_key: Option<String>,
    #[arg(long = "signer-session-id")]
    pub signer_session_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderWatchArgs {
    pub key: String,
    #[arg(long)]
    pub frames: Option<usize>,
    #[arg(long, default_value_t = 1_000)]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Args)]
pub struct RecordKeyArgs {
    pub key: String,
}

#[derive(Debug, Clone, Args)]
pub struct SellArgs {
    #[command(subcommand)]
    pub command: SellCommand,
}

#[derive(Debug, Clone, Args)]
pub struct SellAddArgs {
    pub product: String,
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub category: Option<String>,
    #[arg(long)]
    pub summary: Option<String>,
    #[arg(long = "pack")]
    pub pack: Option<String>,
    #[arg(long = "price")]
    pub price_expr: Option<String>,
    #[arg(long = "stock")]
    pub stock: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SellCommand {
    #[command(about = "Create a listing draft")]
    Add(SellAddArgs),
    #[command(about = "Show a local listing draft")]
    Show(SellShowArgs),
    #[command(about = "Check a listing draft")]
    Check(ListingFileArgs),
    #[command(about = "Publish a listing draft")]
    Publish(ListingMutationArgs),
    #[command(about = "Update a published listing from a draft")]
    Update(ListingMutationArgs),
    #[command(about = "Pause a published listing")]
    Pause(ListingMutationArgs),
    #[command(about = "Change the price in a local listing draft")]
    Reprice(SellRepriceArgs),
    #[command(about = "Change the available stock in a local listing draft")]
    Restock(SellRestockArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SellShowArgs {
    pub file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct SellRepriceArgs {
    pub file: PathBuf,
    pub price_expr: String,
}

#[derive(Debug, Clone, Args)]
pub struct SellRestockArgs {
    pub file: PathBuf,
    pub available: String,
}
