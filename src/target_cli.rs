#![allow(dead_code)]

use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TargetOutputFormat {
    Human,
    Json,
    Ndjson,
}

#[derive(Debug, Parser, Clone)]
#[command(name = "radroots", disable_help_subcommand = true)]
pub struct TargetCliArgs {
    #[arg(long = "format", global = true, value_enum, default_value = "human")]
    pub format: TargetOutputFormat,
    #[arg(long = "account-id", global = true)]
    pub account_id: Option<String>,
    #[arg(long = "relay", global = true)]
    pub relay: Vec<String>,
    #[arg(long = "offline", global = true, action = ArgAction::SetTrue, conflicts_with = "online")]
    pub offline: bool,
    #[arg(long = "online", global = true, action = ArgAction::SetTrue, conflicts_with = "offline")]
    pub online: bool,
    #[arg(long = "dry-run", global = true, action = ArgAction::SetTrue)]
    pub dry_run: bool,
    #[arg(long = "idempotency-key", global = true)]
    pub idempotency_key: Option<String>,
    #[arg(long = "correlation-id", global = true)]
    pub correlation_id: Option<String>,
    #[arg(long = "approval-token", global = true)]
    pub approval_token: Option<String>,
    #[arg(long = "no-input", global = true, action = ArgAction::SetTrue)]
    pub no_input: bool,
    #[arg(long = "quiet", global = true, action = ArgAction::SetTrue)]
    pub quiet: bool,
    #[arg(long = "verbose", global = true, action = ArgAction::SetTrue)]
    pub verbose: bool,
    #[arg(long = "trace", global = true, action = ArgAction::SetTrue)]
    pub trace: bool,
    #[arg(long = "no-color", global = true, action = ArgAction::SetTrue)]
    pub no_color: bool,
    #[command(subcommand)]
    pub command: TargetCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TargetCommand {
    Workspace(WorkspaceArgs),
    Health(HealthArgs),
    Config(ConfigArgs),
    Account(AccountArgs),
    Signer(SignerArgs),
    Relay(RelayArgs),
    Store(StoreArgs),
    Sync(SyncArgs),
    Runtime(RuntimeArgs),
    Job(JobArgs),
    Farm(FarmArgs),
    Listing(ListingArgs),
    Market(MarketArgs),
    Basket(BasketArgs),
    Order(OrderArgs),
}

impl TargetCommand {
    pub fn operation_id(&self) -> &'static str {
        match self {
            Self::Workspace(args) => match args.command {
                WorkspaceCommand::Init => "workspace.init",
                WorkspaceCommand::Get => "workspace.get",
            },
            Self::Health(args) => match &args.command {
                HealthCommand::Status(status) => match status.command {
                    HealthStatusCommand::Get => "health.status.get",
                },
                HealthCommand::Check(check) => match check.command {
                    HealthCheckCommand::Run => "health.check.run",
                },
            },
            Self::Config(args) => match args.command {
                ConfigCommand::Get => "config.get",
            },
            Self::Account(args) => match &args.command {
                AccountCommand::Create => "account.create",
                AccountCommand::Import(_) => "account.import",
                AccountCommand::Get(_) => "account.get",
                AccountCommand::List => "account.list",
                AccountCommand::Remove(_) => "account.remove",
                AccountCommand::Selection(selection) => match &selection.command {
                    AccountSelectionCommand::Get => "account.selection.get",
                    AccountSelectionCommand::Update(_) => "account.selection.update",
                    AccountSelectionCommand::Clear => "account.selection.clear",
                },
            },
            Self::Signer(args) => match &args.command {
                SignerCommand::Status(status) => match status.command {
                    SignerStatusCommand::Get => "signer.status.get",
                },
            },
            Self::Relay(args) => match args.command {
                RelayCommand::List => "relay.list",
            },
            Self::Store(args) => match &args.command {
                StoreCommand::Init => "store.init",
                StoreCommand::Status(status) => match status.command {
                    StoreStatusCommand::Get => "store.status.get",
                },
                StoreCommand::Export => "store.export",
                StoreCommand::Backup(backup) => match backup.command {
                    StoreBackupCommand::Create => "store.backup.create",
                },
            },
            Self::Sync(args) => match &args.command {
                SyncCommand::Status(status) => match status.command {
                    SyncStatusCommand::Get => "sync.status.get",
                },
                SyncCommand::Pull => "sync.pull",
                SyncCommand::Push => "sync.push",
                SyncCommand::Watch => "sync.watch",
            },
            Self::Runtime(args) => match &args.command {
                RuntimeCommand::Status(status) => match status.command {
                    RuntimeStatusCommand::Get => "runtime.status.get",
                },
                RuntimeCommand::Start => "runtime.start",
                RuntimeCommand::Stop => "runtime.stop",
                RuntimeCommand::Restart => "runtime.restart",
                RuntimeCommand::Log(log) => match log.command {
                    RuntimeLogCommand::Watch => "runtime.log.watch",
                },
                RuntimeCommand::Config(config) => match config.command {
                    RuntimeConfigCommand::Get => "runtime.config.get",
                },
            },
            Self::Job(args) => match args.command {
                JobCommand::Get => "job.get",
                JobCommand::List => "job.list",
                JobCommand::Watch => "job.watch",
            },
            Self::Farm(args) => match &args.command {
                FarmCommand::Create(_) => "farm.create",
                FarmCommand::Get => "farm.get",
                FarmCommand::Profile(profile) => match profile.command {
                    FarmProfileCommand::Update(_) => "farm.profile.update",
                },
                FarmCommand::Location(location) => match location.command {
                    FarmLocationCommand::Update(_) => "farm.location.update",
                },
                FarmCommand::Fulfillment(fulfillment) => match fulfillment.command {
                    FarmFulfillmentCommand::Update(_) => "farm.fulfillment.update",
                },
                FarmCommand::Readiness(readiness) => match readiness.command {
                    FarmReadinessCommand::Check => "farm.readiness.check",
                },
                FarmCommand::Publish => "farm.publish",
            },
            Self::Listing(args) => match &args.command {
                ListingCommand::Create(_) => "listing.create",
                ListingCommand::Get(_) => "listing.get",
                ListingCommand::List => "listing.list",
                ListingCommand::Update(_) => "listing.update",
                ListingCommand::Validate(_) => "listing.validate",
                ListingCommand::Publish(_) => "listing.publish",
                ListingCommand::Archive(_) => "listing.archive",
            },
            Self::Market(args) => match &args.command {
                MarketCommand::Refresh => "market.refresh",
                MarketCommand::Product(product) => match &product.command {
                    MarketProductCommand::Search(_) => "market.product.search",
                },
                MarketCommand::Listing(listing) => match &listing.command {
                    MarketListingCommand::Get(_) => "market.listing.get",
                },
            },
            Self::Basket(args) => match &args.command {
                BasketCommand::Create(_) => "basket.create",
                BasketCommand::Get(_) => "basket.get",
                BasketCommand::List => "basket.list",
                BasketCommand::Item(item) => match item.command {
                    BasketItemCommand::Add(_) => "basket.item.add",
                    BasketItemCommand::Update(_) => "basket.item.update",
                    BasketItemCommand::Remove(_) => "basket.item.remove",
                },
                BasketCommand::Validate(_) => "basket.validate",
                BasketCommand::Quote(quote) => match quote.command {
                    BasketQuoteCommand::Create(_) => "basket.quote.create",
                },
            },
            Self::Order(args) => match &args.command {
                OrderCommand::Submit(_) => "order.submit",
                OrderCommand::Get(_) => "order.get",
                OrderCommand::List => "order.list",
                OrderCommand::Event(event) => match &event.command {
                    OrderEventCommand::List(_) => "order.event.list",
                    OrderEventCommand::Watch(_) => "order.event.watch",
                },
            },
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct WorkspaceArgs {
    #[command(subcommand)]
    pub command: WorkspaceCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum WorkspaceCommand {
    Init,
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct HealthArgs {
    #[command(subcommand)]
    pub command: HealthCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum HealthCommand {
    Status(HealthStatusArgs),
    Check(HealthCheckArgs),
}

#[derive(Debug, Clone, Args)]
pub struct HealthStatusArgs {
    #[command(subcommand)]
    pub command: HealthStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HealthStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct HealthCheckArgs {
    #[command(subcommand)]
    pub command: HealthCheckCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HealthCheckCommand {
    Run,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum ConfigCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub command: AccountCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AccountCommand {
    Create,
    Import(AccountImportArgs),
    Get(AccountGetArgs),
    List,
    Remove(AccountSelectorArgs),
    Selection(AccountSelectionArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AccountImportArgs {
    pub path: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub default: bool,
}

#[derive(Debug, Clone, Args)]
pub struct AccountGetArgs {
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct AccountSelectorArgs {
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct AccountSelectionArgs {
    #[command(subcommand)]
    pub command: AccountSelectionCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AccountSelectionCommand {
    Get,
    Update(AccountSelectorArgs),
    Clear,
}

#[derive(Debug, Clone, Args)]
pub struct SignerArgs {
    #[command(subcommand)]
    pub command: SignerCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SignerCommand {
    Status(SignerStatusArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SignerStatusArgs {
    #[command(subcommand)]
    pub command: SignerStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SignerStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct RelayArgs {
    #[command(subcommand)]
    pub command: RelayCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum RelayCommand {
    List,
}

#[derive(Debug, Clone, Args)]
pub struct StoreArgs {
    #[command(subcommand)]
    pub command: StoreCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum StoreCommand {
    Init,
    Status(StoreStatusArgs),
    Export,
    Backup(StoreBackupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct StoreStatusArgs {
    #[command(subcommand)]
    pub command: StoreStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum StoreStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct StoreBackupArgs {
    #[command(subcommand)]
    pub command: StoreBackupCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum StoreBackupCommand {
    Create,
}

#[derive(Debug, Clone, Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: SyncCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SyncCommand {
    Status(SyncStatusArgs),
    Pull,
    Push,
    Watch,
}

#[derive(Debug, Clone, Args)]
pub struct SyncStatusArgs {
    #[command(subcommand)]
    pub command: SyncStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum SyncStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeArgs {
    #[command(subcommand)]
    pub command: RuntimeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RuntimeCommand {
    Status(RuntimeStatusArgs),
    Start,
    Stop,
    Restart,
    Log(RuntimeLogArgs),
    Config(RuntimeConfigArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeStatusArgs {
    #[command(subcommand)]
    pub command: RuntimeStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum RuntimeStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeLogArgs {
    #[command(subcommand)]
    pub command: RuntimeLogCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum RuntimeLogCommand {
    Watch,
}

#[derive(Debug, Clone, Args)]
pub struct RuntimeConfigArgs {
    #[command(subcommand)]
    pub command: RuntimeConfigCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum RuntimeConfigCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct JobArgs {
    #[command(subcommand)]
    pub command: JobCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum JobCommand {
    Get,
    List,
    Watch,
}

#[derive(Debug, Clone, Args)]
pub struct FarmArgs {
    #[command(subcommand)]
    pub command: FarmCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmCommand {
    Create(FarmCreateArgs),
    Get,
    Profile(FarmProfileArgs),
    Location(FarmLocationArgs),
    Fulfillment(FarmFulfillmentArgs),
    Readiness(FarmReadinessArgs),
    Publish,
}

#[derive(Debug, Clone, Args)]
pub struct FarmCreateArgs {
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
    #[arg(long = "delivery-method")]
    pub delivery_method: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmProfileArgs {
    #[command(subcommand)]
    pub command: FarmProfileCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmProfileCommand {
    Update(FarmProfileUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct FarmProfileUpdateArgs {
    #[arg(long)]
    pub field: Option<String>,
    #[arg(long)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmLocationArgs {
    #[command(subcommand)]
    pub command: FarmLocationCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmLocationCommand {
    Update(FarmLocationUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct FarmLocationUpdateArgs {
    #[arg(long)]
    pub field: Option<String>,
    #[arg(long)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmFulfillmentArgs {
    #[command(subcommand)]
    pub command: FarmFulfillmentCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum FarmFulfillmentCommand {
    Update(FarmFulfillmentUpdateArgs),
}

#[derive(Debug, Clone, Args)]
pub struct FarmFulfillmentUpdateArgs {
    #[arg(long)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct FarmReadinessArgs {
    #[command(subcommand)]
    pub command: FarmReadinessCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum FarmReadinessCommand {
    Check,
}

#[derive(Debug, Clone, Args)]
pub struct ListingArgs {
    #[command(subcommand)]
    pub command: ListingCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ListingCommand {
    Create(ListingCreateArgs),
    Get(LookupArgs),
    List,
    Update(FileArgs),
    Validate(FileArgs),
    Publish(FileArgs),
    Archive(FileArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ListingCreateArgs {
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
pub struct FileArgs {
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct LookupArgs {
    pub key: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct MarketArgs {
    #[command(subcommand)]
    pub command: MarketCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketCommand {
    Refresh,
    Product(MarketProductArgs),
    Listing(MarketListingArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MarketProductArgs {
    #[command(subcommand)]
    pub command: MarketProductCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketProductCommand {
    Search(QueryArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MarketListingArgs {
    #[command(subcommand)]
    pub command: MarketListingCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MarketListingCommand {
    Get(LookupArgs),
}

#[derive(Debug, Clone, Args)]
pub struct QueryArgs {
    pub query: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketArgs {
    #[command(subcommand)]
    pub command: BasketCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketCommand {
    Create(BasketCreateArgs),
    Get(BasketKeyArgs),
    List,
    Item(BasketItemArgs),
    Validate(BasketKeyArgs),
    Quote(BasketQuoteArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BasketCreateArgs {
    pub basket_id: Option<String>,
    #[arg(long)]
    pub listing: Option<String>,
    #[arg(long = "listing-addr")]
    pub listing_addr: Option<String>,
    #[arg(long = "bin-id")]
    pub bin_id: Option<String>,
    #[arg(long)]
    pub quantity: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketKeyArgs {
    pub basket_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketItemArgs {
    #[command(subcommand)]
    pub command: BasketItemCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketItemCommand {
    Add(BasketItemMutationArgs),
    Update(BasketItemMutationArgs),
    Remove(BasketItemRemoveArgs),
}

#[derive(Debug, Clone, Args)]
pub struct BasketItemMutationArgs {
    pub basket_id: Option<String>,
    #[arg(long = "item-id")]
    pub item_id: Option<String>,
    #[arg(long)]
    pub listing: Option<String>,
    #[arg(long = "listing-addr")]
    pub listing_addr: Option<String>,
    #[arg(long = "bin-id")]
    pub bin_id: Option<String>,
    #[arg(long)]
    pub quantity: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketItemRemoveArgs {
    pub basket_id: Option<String>,
    pub item_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct BasketQuoteArgs {
    #[command(subcommand)]
    pub command: BasketQuoteCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum BasketQuoteCommand {
    Create(BasketKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderArgs {
    #[command(subcommand)]
    pub command: OrderCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderCommand {
    Submit(OrderSubmitArgs),
    Get(OrderKeyArgs),
    List,
    Event(OrderEventArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OrderSubmitArgs {
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderKeyArgs {
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct OrderEventArgs {
    #[command(subcommand)]
    pub command: OrderEventCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum OrderEventCommand {
    List(OrderKeyArgs),
    Watch(OrderKeyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct PathOutputArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::{CommandFactory, Parser};

    use super::{TargetCliArgs, TargetOutputFormat};
    use crate::operation_registry::OPERATION_REGISTRY;

    #[test]
    fn target_parser_accepts_every_target_registry_path() {
        for operation in OPERATION_REGISTRY {
            let parsed = TargetCliArgs::try_parse_from(operation.cli_path.split_whitespace())
                .unwrap_or_else(|error| {
                    panic!("{} failed to parse: {error}", operation.cli_path);
                });
            assert_eq!(parsed.command.operation_id(), operation.operation_id);
        }
    }

    #[test]
    fn target_parser_exposes_only_target_top_level_namespaces() {
        let actual = TargetCliArgs::command()
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect::<BTreeSet<_>>();
        let expected = [
            "workspace",
            "health",
            "config",
            "account",
            "signer",
            "relay",
            "store",
            "sync",
            "runtime",
            "job",
            "farm",
            "listing",
            "market",
            "basket",
            "order",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn target_global_flags_parse() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "--format",
            "ndjson",
            "--account-id",
            "acct_test",
            "--relay",
            "wss://relay.one",
            "--relay",
            "wss://relay.two",
            "--offline",
            "--dry-run",
            "--idempotency-key",
            "idem_test",
            "--correlation-id",
            "corr_test",
            "--approval-token",
            "approval_test",
            "--no-input",
            "--quiet",
            "--no-color",
            "workspace",
            "get",
        ])
        .expect("target args parse");

        assert_eq!(parsed.format, TargetOutputFormat::Ndjson);
        assert_eq!(parsed.account_id.as_deref(), Some("acct_test"));
        assert_eq!(
            parsed.relay,
            vec!["wss://relay.one".to_owned(), "wss://relay.two".to_owned()]
        );
        assert!(parsed.offline);
        assert!(parsed.dry_run);
        assert_eq!(parsed.idempotency_key.as_deref(), Some("idem_test"));
        assert_eq!(parsed.correlation_id.as_deref(), Some("corr_test"));
        assert_eq!(parsed.approval_token.as_deref(), Some("approval_test"));
        assert!(parsed.no_input);
        assert!(parsed.quiet);
        assert!(parsed.no_color);
        assert_eq!(parsed.command.operation_id(), "workspace.get");
    }

    #[test]
    fn target_parser_rejects_removed_global_flags() {
        let rejected = [
            vec!["radroots", "--output", "json", "config", "get"],
            vec!["radroots", "--json", "config", "get"],
            vec!["radroots", "--ndjson", "config", "get"],
            vec!["radroots", "--yes", "config", "get"],
            vec!["radroots", "--non-interactive", "config", "get"],
            vec!["radroots", "--signer", "myc", "config", "get"],
            vec!["radroots", "--farm-id", "farm_test", "config", "get"],
            vec!["radroots", "--profile", "repo_local", "config", "get"],
            vec![
                "radroots",
                "--signer-session-id",
                "sess_test",
                "config",
                "get",
            ],
        ];

        for args in rejected {
            assert!(TargetCliArgs::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn target_parser_rejects_removed_top_level_commands() {
        for command in [
            "setup", "status", "doctor", "sell", "find", "local", "net", "myc", "rpc",
        ] {
            assert!(TargetCliArgs::try_parse_from(["radroots", command]).is_err());
        }
    }

    #[test]
    fn target_parser_rejects_deferred_namespaces() {
        for command in ["product", "message", "approval", "agent"] {
            assert!(TargetCliArgs::try_parse_from(["radroots", command]).is_err());
        }
    }

    #[test]
    fn target_parser_rejects_online_offline_conflict() {
        assert!(
            TargetCliArgs::try_parse_from([
                "radroots",
                "--online",
                "--offline",
                "health",
                "status",
                "get"
            ])
            .is_err()
        );
    }
}
