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

const ROOT_HELP: &str = "\
radroots - local food trade on Nostr

Start here
  setup                 Guided first-time setup for sellers and buyers
  status                Show what is ready and what needs attention

Sell from your farm
  farm                  Set up and publish your farm
  sell                  Create, check, and publish listings

Buy from the market
  market                Update local market data and search listings
  order                 Create and manage order requests

Accounts and settings
  account               Create, import, and manage local accounts
  config                Show effective configuration

Advanced and troubleshooting
  doctor                Check readiness and suggest next steps
  local                 Manage local market data storage
  sync                  Inspect sync status and watch updates
  relay                 Show relay configuration
  net                   Show network posture
  signer                Show signer readiness
  rpc                   Show runtime bridge status
  myc                   Show myc status
  runtime               Manage runtimes
  job                   Inspect background jobs
  listing               Advanced listing commands
  find                  Advanced search command

Global options
  --output <human|json|ndjson>
  --json
  --ndjson
  --dry-run
  --no-input
  --yes
  --quiet
  --verbose
  --trace
  --no-color
  --account <ACCOUNT>
  --signer <SIGNER>
  --relay <RELAY>

Examples
  radroots setup seller
  radroots market search eggs
  radroots sell add tomatoes
  radroots order create --listing sf-tomatoes --bin bin-1 --qty 2
";

const SETUP_HELP: &str = "\
Examples:
  radroots setup seller
  radroots setup buyer
  radroots setup both

This workflow layer sits on top of the existing account, local, and farm commands.
Use `radroots account create` or `radroots account select` explicitly when no actor is resolved.
";

const STATUS_HELP: &str = "\
Examples:
  radroots status
  radroots doctor
  radroots config show

This workflow summary reflects the current readiness and configuration surfaces.
When no actor is resolved, it points to explicit account commands instead of mutating account state.
";

const ACCOUNT_HELP: &str = "\
Examples:
  radroots account create
  radroots account import ./identity.json
  radroots account view
  radroots account list
  radroots account select market-main
  radroots account clear-default
  radroots account remove market-main

Select stores the default account. Clear-default removes the stored default without deleting accounts.

Compatibility aliases: new, whoami, ls, use.
";

const FARM_HELP: &str = "\
Examples:
  radroots farm init
  radroots farm set delivery pickup
  radroots farm check
  radroots farm show --scope workspace
  radroots farm publish

Compatibility paths: `farm setup`, `farm status`, and `farm get` remain available.
";

const MARKET_HELP: &str = "\
Examples:
  radroots market update
  radroots market search tomatoes
  radroots market view sf-tomatoes

Compatibility paths: `sync pull`, `find`, and `listing get` remain available.
";

const SELL_HELP: &str = "\
Examples:
  radroots sell add tomatoes --pack \"1 kg\" --price \"10 USD/kg\" --stock 25
  radroots sell check ./listing.toml
  radroots sell publish ./listing.toml

Compatibility path: the advanced `listing` command family remains available.
";

const ORDER_HELP: &str = "\
Examples:
  radroots order create --listing sf-tomatoes --bin bin-1 --qty 2
  radroots order view ord_demo
  radroots order list
  radroots order submit ord_demo --watch

Compatibility aliases: new, get, ls.
";

#[derive(Debug, Parser, Clone)]
#[command(name = "radroots")]
#[command(version)]
#[command(
    after_help = "Global output: use --output <human|json|ndjson>. Existing --json and --ndjson aliases remain supported."
)]
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
            .override_help(ROOT_HELP)
            .mut_subcommand("setup", |command| command.after_help(SETUP_HELP))
            .mut_subcommand("status", |command| command.after_help(STATUS_HELP))
            .mut_subcommand("account", |command| command.after_help(ACCOUNT_HELP))
            .mut_subcommand("farm", |command| command.after_help(FARM_HELP))
            .mut_subcommand("market", |command| command.after_help(MARKET_HELP))
            .mut_subcommand("sell", |command| command.after_help(SELL_HELP))
            .mut_subcommand("order", |command| command.after_help(ORDER_HELP))
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        AccountCommand, CliArgs, Command, ConfigCommand, FarmCommand, FarmFieldArg, FarmScopeArg,
        JobCommand, JobWatchArgs, ListingCommand, LocalCommand, LocalExportFormatArg,
        MarketCommand, MycCommand, NetCommand, OrderCommand, OrderWatchArgs, OutputFormatArg,
        RelayCommand, RpcCommand, RuntimeCommand, RuntimeConfigCommand, SellCommand, SetupRoleArg,
        SignerCommand, SyncCommand, SyncWatchArgs,
    };
    use crate::runtime::config::OutputFormat;
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
            "--output",
            "json",
            "--json",
            "--verbose",
            "--dry-run",
            "--no-color",
            "--no-input",
            "--yes",
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
            "--hyf-enabled",
            "--hyf-executable",
            "bin/hyfd",
            "config",
            "show",
        ]);
        assert_eq!(parsed.output_format, Some(OutputFormatArg::Json));
        assert!(parsed.json);
        assert!(parsed.verbose);
        assert!(parsed.dry_run);
        assert!(parsed.no_color);
        assert!(parsed.no_input);
        assert!(parsed.yes);
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
        assert!(parsed.hyf_enabled);
        assert_eq!(
            parsed
                .hyf_executable
                .as_deref()
                .and_then(|path| path.to_str()),
            Some("bin/hyfd")
        );
    }

    #[test]
    fn parses_output_format_and_interaction_flags() {
        let parsed = CliArgs::parse_from([
            "radroots",
            "--output",
            "ndjson",
            "--non-interactive",
            "--yes",
            "config",
            "show",
        ]);
        assert_eq!(parsed.output_format, Some(OutputFormatArg::Ndjson));
        assert!(parsed.no_input);
        assert!(parsed.yes);
    }

    #[test]
    fn parses_output_format_after_non_conflicting_subcommand() {
        let parsed = CliArgs::parse_from(["radroots", "config", "show", "--output", "json"]);
        assert_eq!(parsed.output_format, Some(OutputFormatArg::Json));
        match parsed.command {
            Command::Config(config) => match config.command {
                ConfigCommand::Show => {}
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn low_level_output_flags_remain_command_local() {
        let parsed = CliArgs::parse_from([
            "radroots",
            "--output",
            "json",
            "listing",
            "new",
            "--output",
            "listing.toml",
        ]);
        assert_eq!(parsed.output_format, Some(OutputFormatArg::Json));
        match parsed.command {
            Command::Listing(listing) => match listing.command {
                ListingCommand::New(args) => {
                    assert_eq!(
                        args.output.as_deref().and_then(|path| path.to_str()),
                        Some("listing.toml")
                    );
                }
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn command_group_output_flag_can_still_target_global_format() {
        let parsed = CliArgs::parse_from([
            "radroots",
            "listing",
            "--output",
            "json",
            "new",
            "--output",
            "listing.toml",
        ]);
        assert_eq!(parsed.output_format, Some(OutputFormatArg::Json));
        match parsed.command {
            Command::Listing(listing) => match listing.command {
                ListingCommand::New(args) => {
                    assert_eq!(
                        args.output.as_deref().and_then(|path| path.to_str()),
                        Some("listing.toml")
                    );
                }
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_human_first_top_level_commands() {
        let setup = CliArgs::parse_from(["radroots", "setup", "seller"]);
        match setup.command {
            Command::Setup(args) => assert_eq!(args.role, SetupRoleArg::Seller),
            _ => panic!("unexpected command variant"),
        }

        let status = CliArgs::parse_from(["radroots", "status"]);
        assert!(matches!(status.command, Command::Status));

        let market = CliArgs::parse_from(["radroots", "market", "search", "eggs"]);
        match market.command {
            Command::Market(args) => match args.command {
                MarketCommand::Search(find) => assert_eq!(find.query, vec!["eggs"]),
                _ => panic!("unexpected market subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let sell = CliArgs::parse_from([
            "radroots",
            "sell",
            "add",
            "eggs",
            "--pack",
            "dozen",
            "--price",
            "8 USD/dozen",
            "--stock",
            "10",
        ]);
        match sell.command {
            Command::Sell(args) => match args.command {
                SellCommand::Add(add) => {
                    assert_eq!(add.product, "eggs");
                    assert_eq!(add.pack.as_deref(), Some("dozen"));
                    assert_eq!(add.price_expr.as_deref(), Some("8 USD/dozen"));
                    assert_eq!(add.stock.as_deref(), Some("10"));
                }
                _ => panic!("unexpected sell subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn parses_human_first_aliases() {
        let account_create = CliArgs::parse_from(["radroots", "account", "create"]);
        match account_create.command {
            Command::Account(account) => assert!(matches!(account.command, AccountCommand::New)),
            _ => panic!("unexpected command variant"),
        }

        let account_view = CliArgs::parse_from(["radroots", "account", "view"]);
        match account_view.command {
            Command::Account(account) => assert!(matches!(account.command, AccountCommand::Whoami)),
            _ => panic!("unexpected command variant"),
        }

        let account_list = CliArgs::parse_from(["radroots", "account", "list"]);
        match account_list.command {
            Command::Account(account) => assert!(matches!(account.command, AccountCommand::Ls)),
            _ => panic!("unexpected command variant"),
        }

        let account_select = CliArgs::parse_from(["radroots", "account", "select", "market-main"]);
        match account_select.command {
            Command::Account(account) => match account.command {
                AccountCommand::Use(args) => assert_eq!(args.selector, "market-main"),
                _ => panic!("unexpected account subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let farm_check = CliArgs::parse_from(["radroots", "farm", "check"]);
        match farm_check.command {
            Command::Farm(farm) => assert!(matches!(farm.command, FarmCommand::Status(_))),
            _ => panic!("unexpected command variant"),
        }

        let farm_show = CliArgs::parse_from(["radroots", "farm", "show"]);
        match farm_show.command {
            Command::Farm(farm) => assert!(matches!(farm.command, FarmCommand::Get(_))),
            _ => panic!("unexpected command variant"),
        }

        let market_view = CliArgs::parse_from(["radroots", "market", "view", "lst_123"]);
        match market_view.command {
            Command::Market(market) => match market.command {
                MarketCommand::View(args) => assert_eq!(args.key, "lst_123"),
                _ => panic!("unexpected market subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_create =
            CliArgs::parse_from(["radroots", "order", "create", "--listing", "eggs"]);
        match order_create.command {
            Command::Order(order) => match order.command {
                OrderCommand::New(args) => assert_eq!(args.listing.as_deref(), Some("eggs")),
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_view = CliArgs::parse_from(["radroots", "order", "view", "ord_demo"]);
        match order_view.command {
            Command::Order(order) => match order.command {
                OrderCommand::Get(args) => assert_eq!(args.key, "ord_demo"),
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_list = CliArgs::parse_from(["radroots", "order", "list"]);
        match order_list.command {
            Command::Order(order) => assert!(matches!(order.command, OrderCommand::Ls)),
            _ => panic!("unexpected command variant"),
        }
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

        let import = CliArgs::parse_from([
            "radroots",
            "account",
            "import",
            "./identity.json",
            "--default",
        ]);
        match import.command {
            Command::Account(account) => match account.command {
                AccountCommand::Import(args) => {
                    assert_eq!(args.path, PathBuf::from("./identity.json"));
                    assert!(args.default);
                }
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

        let clear_default = CliArgs::parse_from(["radroots", "account", "clear-default"]);
        match clear_default.command {
            Command::Account(account) => match account.command {
                AccountCommand::ClearDefault => {}
                _ => panic!("unexpected account subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let remove = CliArgs::parse_from(["radroots", "account", "remove", "market-main"]);
        match remove.command {
            Command::Account(account) => match account.command {
                AccountCommand::Remove(args) => assert_eq!(args.selector, "market-main"),
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

        let farm_setup = CliArgs::parse_from([
            "radroots",
            "farm",
            "setup",
            "--scope",
            "workspace",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
            "--city",
            "San Francisco",
            "--region",
            "CA",
            "--country",
            "US",
            "--delivery-method",
            "local_delivery",
        ]);
        match farm_setup.command {
            Command::Farm(args) => match args.command {
                FarmCommand::Setup(setup) => {
                    assert_eq!(setup.scope, Some(FarmScopeArg::Workspace));
                    assert_eq!(setup.name, "La Huerta");
                    assert_eq!(setup.location, "San Francisco, CA");
                    assert_eq!(setup.city.as_deref(), Some("San Francisco"));
                    assert_eq!(setup.delivery_method, "local_delivery");
                }
                _ => panic!("unexpected farm subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let farm_init = CliArgs::parse_from([
            "radroots",
            "farm",
            "init",
            "--scope",
            "workspace",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
            "--delivery",
            "pickup",
        ]);
        match farm_init.command {
            Command::Farm(args) => match args.command {
                FarmCommand::Init(init) => {
                    assert_eq!(init.scope, Some(FarmScopeArg::Workspace));
                    assert_eq!(init.name.as_deref(), Some("La Huerta"));
                    assert_eq!(init.location.as_deref(), Some("San Francisco, CA"));
                    assert_eq!(init.delivery_method.as_deref(), Some("pickup"));
                }
                _ => panic!("unexpected farm subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let farm_set = CliArgs::parse_from([
            "radroots",
            "farm",
            "set",
            "--scope",
            "user",
            "display-name",
            "La",
            "Huerta",
            "Farm",
        ]);
        match farm_set.command {
            Command::Farm(args) => match args.command {
                FarmCommand::Set(set) => {
                    assert_eq!(set.scope, Some(FarmScopeArg::User));
                    assert_eq!(set.field, FarmFieldArg::DisplayName);
                    assert_eq!(set.value, vec!["La", "Huerta", "Farm"]);
                }
                _ => panic!("unexpected farm subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let farm_status = CliArgs::parse_from(["radroots", "farm", "status", "--scope", "user"]);
        match farm_status.command {
            Command::Farm(args) => match args.command {
                FarmCommand::Status(status) => assert_eq!(status.scope, Some(FarmScopeArg::User)),
                _ => panic!("unexpected farm subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let farm_get = CliArgs::parse_from(["radroots", "farm", "get"]);
        match farm_get.command {
            Command::Farm(args) => match args.command {
                FarmCommand::Get(get) => assert!(get.scope.is_none()),
                _ => panic!("unexpected farm subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let farm_publish = CliArgs::parse_from([
            "radroots",
            "farm",
            "publish",
            "--scope",
            "workspace",
            "--idempotency-key",
            "farm-publish-1",
            "--signer-session-id",
            "session-1",
            "--print-job",
            "--print-event",
        ]);
        match farm_publish.command {
            Command::Farm(args) => match args.command {
                FarmCommand::Publish(publish) => {
                    assert_eq!(publish.scope, Some(FarmScopeArg::Workspace));
                    assert_eq!(publish.idempotency_key.as_deref(), Some("farm-publish-1"));
                    assert_eq!(publish.signer_session_id.as_deref(), Some("session-1"));
                    assert!(publish.print_job);
                    assert!(publish.print_event);
                }
                _ => panic!("unexpected farm subcommand"),
            },
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

        let listing_new = CliArgs::parse_from([
            "radroots",
            "listing",
            "new",
            "--output",
            "draft.toml",
            "--key",
            "sf-tomatoes",
            "--title",
            "San Francisco Tomatoes",
            "--category",
            "produce.vegetables.tomatoes",
            "--summary",
            "Fresh tomatoes",
            "--quantity-amount",
            "1000",
            "--quantity-unit",
            "g",
            "--price-amount",
            "0.01",
            "--available",
            "25",
        ]);
        match listing_new.command {
            Command::Listing(args) => match args.command {
                ListingCommand::New(new) => {
                    assert_eq!(
                        new.output.as_deref().and_then(|path| path.to_str()),
                        Some("draft.toml")
                    );
                    assert_eq!(new.key.as_deref(), Some("sf-tomatoes"));
                    assert_eq!(new.title.as_deref(), Some("San Francisco Tomatoes"));
                    assert_eq!(new.category.as_deref(), Some("produce.vegetables.tomatoes"));
                    assert_eq!(new.summary.as_deref(), Some("Fresh tomatoes"));
                    assert_eq!(new.quantity_amount.as_deref(), Some("1000"));
                    assert_eq!(new.quantity_unit.as_deref(), Some("g"));
                    assert_eq!(new.price_amount.as_deref(), Some("0.01"));
                    assert_eq!(new.available.as_deref(), Some("25"));
                    assert!(new.price_currency.is_none());
                    assert!(new.price_per_amount.is_none());
                    assert!(new.price_per_unit.is_none());
                }
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

        let listing_publish = CliArgs::parse_from(["radroots", "listing", "publish", "draft.toml"]);
        match listing_publish.command {
            Command::Listing(args) => match args.command {
                ListingCommand::Publish(file) => {
                    assert_eq!(file.file.to_str(), Some("draft.toml"));
                    assert!(file.idempotency_key.is_none());
                    assert!(file.signer_session_id.is_none());
                    assert!(!file.print_job);
                    assert!(!file.print_event);
                }
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing_archive = CliArgs::parse_from([
            "radroots",
            "listing",
            "archive",
            "--idempotency-key",
            "archive-key",
            "--print-job",
            "--print-event",
            "draft.toml",
        ]);
        match listing_archive.command {
            Command::Listing(args) => match args.command {
                ListingCommand::Archive(file) => {
                    assert_eq!(file.file.to_str(), Some("draft.toml"));
                    assert_eq!(file.idempotency_key.as_deref(), Some("archive-key"));
                    assert!(file.signer_session_id.is_none());
                    assert!(file.print_job);
                    assert!(file.print_event);
                }
                _ => panic!("unexpected listing subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let listing_update = CliArgs::parse_from([
            "radroots",
            "listing",
            "update",
            "--signer-session-id",
            "sess_123",
            "draft.toml",
        ]);
        match listing_update.command {
            Command::Listing(args) => match args.command {
                ListingCommand::Update(file) => {
                    assert_eq!(file.file.to_str(), Some("draft.toml"));
                    assert_eq!(file.signer_session_id.as_deref(), Some("sess_123"));
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

        let job_watch = CliArgs::parse_from([
            "radroots",
            "job",
            "watch",
            "job_123",
            "--frames",
            "2",
            "--interval-ms",
            "5",
        ]);
        match job_watch.command {
            Command::Job(args) => match args.command {
                JobCommand::Watch(JobWatchArgs {
                    key,
                    frames,
                    interval_ms,
                }) => {
                    assert_eq!(key, "job_123");
                    assert_eq!(frames, Some(2));
                    assert_eq!(interval_ms, 5);
                }
                _ => panic!("unexpected job watch subcommand"),
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

        let runtime_status = CliArgs::parse_from(["radroots", "runtime", "status", "radrootsd"]);
        match runtime_status.command {
            Command::Runtime(args) => match args.command {
                RuntimeCommand::Status(target) => {
                    assert_eq!(target.runtime, "radrootsd");
                    assert!(target.instance.is_none());
                }
                _ => panic!("unexpected runtime subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let runtime_logs = CliArgs::parse_from([
            "radroots",
            "runtime",
            "logs",
            "radrootsd",
            "--instance",
            "local",
        ]);
        match runtime_logs.command {
            Command::Runtime(args) => match args.command {
                RuntimeCommand::Logs(target) => {
                    assert_eq!(target.runtime, "radrootsd");
                    assert_eq!(target.instance.as_deref(), Some("local"));
                }
                _ => panic!("unexpected runtime subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let runtime_config_show =
            CliArgs::parse_from(["radroots", "runtime", "config", "show", "radrootsd"]);
        match runtime_config_show.command {
            Command::Runtime(args) => match args.command {
                RuntimeCommand::Config(runtime_config) => match runtime_config.command {
                    RuntimeConfigCommand::Show(target) => {
                        assert_eq!(target.runtime, "radrootsd");
                        assert!(target.instance.is_none());
                    }
                    _ => panic!("unexpected runtime config subcommand"),
                },
                _ => panic!("unexpected runtime subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let runtime_config_set = CliArgs::parse_from([
            "radroots",
            "runtime",
            "config",
            "set",
            "radrootsd",
            "--instance",
            "local",
            "bridge.enabled",
            "true",
        ]);
        match runtime_config_set.command {
            Command::Runtime(args) => match args.command {
                RuntimeCommand::Config(runtime_config) => match runtime_config.command {
                    RuntimeConfigCommand::Set(set) => {
                        assert_eq!(set.target.runtime, "radrootsd");
                        assert_eq!(set.target.instance.as_deref(), Some("local"));
                        assert_eq!(set.key, "bridge.enabled");
                        assert_eq!(set.value, "true");
                    }
                    _ => panic!("unexpected runtime config subcommand"),
                },
                _ => panic!("unexpected runtime subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_new = CliArgs::parse_from([
            "radroots",
            "order",
            "new",
            "--listing",
            "pasture-eggs",
            "--listing-addr",
            "30402:deadbeef:AAAAAAAAAAAAAAAAAAAAAg",
            "--bin",
            "bin-1",
            "--qty",
            "2",
        ]);
        match order_new.command {
            Command::Order(args) => match args.command {
                OrderCommand::New(new) => {
                    assert_eq!(new.listing.as_deref(), Some("pasture-eggs"));
                    assert_eq!(
                        new.listing_addr.as_deref(),
                        Some("30402:deadbeef:AAAAAAAAAAAAAAAAAAAAAg")
                    );
                    assert_eq!(new.bin_id.as_deref(), Some("bin-1"));
                    assert_eq!(new.bin_count, Some(2));
                }
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_create = CliArgs::parse_from(["radroots", "order", "create"]);
        match order_create.command {
            Command::Order(args) => match args.command {
                OrderCommand::New(_) => {}
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_get = CliArgs::parse_from(["radroots", "order", "get", "ord_demo"]);
        match order_get.command {
            Command::Order(args) => match args.command {
                OrderCommand::Get(key) => assert_eq!(key.key, "ord_demo"),
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_view = CliArgs::parse_from(["radroots", "order", "view", "ord_demo"]);
        match order_view.command {
            Command::Order(args) => match args.command {
                OrderCommand::Get(key) => assert_eq!(key.key, "ord_demo"),
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_ls = CliArgs::parse_from(["radroots", "order", "ls"]);
        match order_ls.command {
            Command::Order(args) => match args.command {
                OrderCommand::Ls => {}
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_list = CliArgs::parse_from(["radroots", "order", "list"]);
        match order_list.command {
            Command::Order(args) => match args.command {
                OrderCommand::Ls => {}
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_submit = CliArgs::parse_from([
            "radroots",
            "order",
            "submit",
            "ord_demo",
            "--watch",
            "--idempotency-key",
            "submit-1",
            "--signer-session-id",
            "sess_456",
        ]);
        match order_submit.command {
            Command::Order(args) => match args.command {
                OrderCommand::Submit(submit) => {
                    assert_eq!(submit.key, "ord_demo");
                    assert!(submit.watch);
                    assert_eq!(submit.idempotency_key.as_deref(), Some("submit-1"));
                    assert_eq!(submit.signer_session_id.as_deref(), Some("sess_456"));
                }
                _ => panic!("unexpected order subcommand"),
            },
            _ => panic!("unexpected command variant"),
        }

        let order_watch = CliArgs::parse_from([
            "radroots",
            "order",
            "watch",
            "ord_demo",
            "--frames",
            "3",
            "--interval-ms",
            "25",
        ]);
        match order_watch.command {
            Command::Order(args) => match args.command {
                OrderCommand::Watch(OrderWatchArgs {
                    key,
                    frames,
                    interval_ms,
                }) => {
                    assert_eq!(key, "ord_demo");
                    assert_eq!(frames, Some(3));
                    assert_eq!(interval_ms, 25);
                }
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

        let account_create = CliArgs::parse_from(["radroots", "account", "create"]);
        assert_eq!(account_create.command.display_name(), "account create");
        assert!(!account_create.command.supports_dry_run());

        let account_import =
            CliArgs::parse_from(["radroots", "account", "import", "./identity.json"]);
        assert_eq!(account_import.command.display_name(), "account import");
        assert!(!account_import.command.supports_dry_run());

        let account_clear_default = CliArgs::parse_from(["radroots", "account", "clear-default"]);
        assert_eq!(
            account_clear_default.command.display_name(),
            "account clear-default"
        );
        assert!(!account_clear_default.command.supports_dry_run());

        let account_remove = CliArgs::parse_from(["radroots", "account", "remove", "market-main"]);
        assert_eq!(account_remove.command.display_name(), "account remove");
        assert!(!account_remove.command.supports_dry_run());

        let farm_init = CliArgs::parse_from(["radroots", "farm", "init"]);
        assert_eq!(farm_init.command.display_name(), "farm init");
        assert!(!farm_init.command.supports_dry_run());

        let farm_set = CliArgs::parse_from(["radroots", "farm", "set", "name", "La Huerta"]);
        assert_eq!(farm_set.command.display_name(), "farm set");
        assert!(!farm_set.command.supports_dry_run());

        let farm_setup = CliArgs::parse_from([
            "radroots",
            "farm",
            "setup",
            "--name",
            "La Huerta",
            "--location",
            "San Francisco, CA",
        ]);
        assert_eq!(farm_setup.command.display_name(), "farm setup");
        assert!(!farm_setup.command.supports_dry_run());

        let farm_check = CliArgs::parse_from(["radroots", "farm", "check"]);
        assert_eq!(farm_check.command.display_name(), "farm check");
        assert!(farm_check.command.supports_dry_run());
        assert!(
            !farm_check
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );

        let farm_publish = CliArgs::parse_from(["radroots", "farm", "publish"]);
        assert_eq!(farm_publish.command.display_name(), "farm publish");
        assert!(farm_publish.command.supports_dry_run());

        let find = CliArgs::parse_from(["radroots", "find", "eggs"]);
        assert!(find.command.supports_output_format(OutputFormat::Ndjson));

        let market_search = CliArgs::parse_from(["radroots", "market", "search", "eggs"]);
        assert_eq!(market_search.command.display_name(), "market search");
        assert!(
            market_search
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );

        let sync_watch = CliArgs::parse_from(["radroots", "sync", "watch", "--frames", "1"]);
        assert!(
            sync_watch
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );

        let sell_add = CliArgs::parse_from(["radroots", "sell", "add", "tomatoes"]);
        assert_eq!(sell_add.command.display_name(), "sell add");
        assert!(!sell_add.command.supports_dry_run());

        let order_create = CliArgs::parse_from(["radroots", "order", "create"]);
        assert_eq!(order_create.command.display_name(), "order create");
        assert!(!order_create.command.supports_dry_run());

        let order_view = CliArgs::parse_from(["radroots", "order", "view", "ord_demo"]);
        assert_eq!(order_view.command.display_name(), "order view");
        assert!(order_view.command.supports_dry_run());

        let order_list = CliArgs::parse_from(["radroots", "order", "list"]);
        assert_eq!(order_list.command.display_name(), "order list");
        assert!(order_list.command.supports_dry_run());
        let order_watch = CliArgs::parse_from(["radroots", "order", "watch", "ord_demo"]);
        assert!(
            order_watch
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );

        let order_submit = CliArgs::parse_from(["radroots", "order", "submit", "ord_demo"]);
        assert_eq!(order_submit.command.display_name(), "order submit");
        assert!(order_submit.command.supports_dry_run());

        let setup = CliArgs::parse_from(["radroots", "setup", "buyer"]);
        assert_eq!(setup.command.display_name(), "setup buyer");
        assert!(!setup.command.supports_dry_run());

        let status = CliArgs::parse_from(["radroots", "status"]);
        assert_eq!(status.command.display_name(), "status");
        assert!(status.command.supports_dry_run());

        let runtime_status = CliArgs::parse_from(["radroots", "runtime", "status", "radrootsd"]);
        assert_eq!(runtime_status.command.display_name(), "runtime status");
        assert!(runtime_status.command.supports_dry_run());
        assert!(
            !runtime_status
                .command
                .supports_output_format(OutputFormat::Ndjson)
        );
    }
}
