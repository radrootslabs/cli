#![allow(dead_code)]

pub mod global;

pub mod account;
pub mod basket;
pub mod config;
pub mod farm;
pub mod health;
pub mod input;
pub mod listing;
pub mod market;
pub mod order;
pub mod relay;
pub mod signer;
pub mod store;
pub mod sync;
pub mod validation;
pub mod workspace;

pub use account::*;
pub use basket::*;
pub use config::*;
pub use farm::*;
pub use health::*;
pub use listing::*;
pub use market::*;
pub use order::*;
pub use relay::*;
pub use signer::*;
pub use store::*;
pub use sync::*;
pub use validation::*;
pub use workspace::*;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TargetOutputFormat {
    Human,
    Json,
    Ndjson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TargetPublishTransport {
    #[value(name = "direct_nostr_relay")]
    DirectNostrRelay,
    #[value(name = "radrootsd_proxy")]
    RadrootsdProxy,
}

impl TargetPublishTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DirectNostrRelay => "direct_nostr_relay",
            Self::RadrootsdProxy => "radrootsd_proxy",
        }
    }
}

#[derive(Debug, Parser, Clone)]
#[command(
    name = "radroots",
    about = "Operate Radroots local-first trade workflows.",
    long_about = "Operate Radroots local-first trade workflows.\n\nPublish transports:\n  direct_nostr_relay publishes directly to configured relays with local signer custody.\n  radrootsd_proxy publishes locally signed events through the local daemon proxy.",
    disable_help_subcommand = true
)]
pub struct TargetCliArgs {
    #[arg(long = "format", global = true, value_enum, default_value = "human")]
    pub format: TargetOutputFormat,
    #[arg(long = "account-id", global = true)]
    pub account_id: Option<String>,
    #[arg(long = "relay", global = true)]
    pub relay: Vec<String>,
    #[arg(
        long = "publish-transport",
        global = true,
        value_enum,
        help = "Select direct_nostr_relay direct relay publish or radrootsd_proxy daemon proxy publish"
    )]
    pub publish_transport: Option<TargetPublishTransport>,
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
    #[command(about = "Inspect and initialize workspace state.")]
    Workspace(WorkspaceArgs),
    #[command(about = "Inspect local readiness and mode-specific recovery steps.")]
    Health(HealthArgs),
    #[command(about = "Show effective configuration and publish-plane readiness.")]
    Config(ConfigArgs),
    #[command(about = "Manage local signer accounts and custody.")]
    Account(AccountArgs),
    #[command(about = "Inspect signer readiness for local relay writes.")]
    Signer(SignerArgs),
    #[command(about = "List configured relay targets for direct relay mode.")]
    Relay(RelayArgs),
    #[command(about = "Initialize and inspect the local replica store.")]
    Store(StoreArgs),
    #[command(about = "Read from relay events into the local replica.")]
    Sync(SyncArgs),
    #[command(about = "Create, inspect, and publish farm profile data.")]
    Farm(FarmArgs),
    #[command(about = "Create, inspect, and publish listing data.")]
    Listing(ListingArgs),
    #[command(about = "Refresh and query market data from the local replica.")]
    Market(MarketArgs),
    #[command(about = "Prepare baskets and quotes before order coordination.")]
    Basket(BasketArgs),
    #[command(about = "Coordinate buyer and farmer order agreement events.")]
    Order(OrderArgs),
    #[command(about = "Inspect validation receipts and proof state.")]
    Validation(ValidationArgs),
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
                AccountCommand::AttachSecret(_) => "account.attach_secret",
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
                StoreCommand::Backup(backup) => match &backup.command {
                    StoreBackupCommand::Create => "store.backup.create",
                    StoreBackupCommand::Restore(_) => "store.backup.restore",
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
            Self::Farm(args) => match &args.command {
                FarmCommand::Create(_) => "farm.create",
                FarmCommand::Get => "farm.get",
                FarmCommand::Rebind(_) => "farm.rebind",
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
                ListingCommand::App(app) => match &app.command {
                    ListingAppCommand::List => "listing.app.list",
                    ListingAppCommand::Export(_) => "listing.app.export",
                },
                ListingCommand::Update(_) => "listing.update",
                ListingCommand::Validate(_) => "listing.validate",
                ListingCommand::Rebind(_) => "listing.rebind",
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
                BasketCommand::Adjustment(adjustment) => match &adjustment.command {
                    BasketAdjustmentCommand::Add(_) => "basket.adjustment.add",
                    BasketAdjustmentCommand::Remove(_) => "basket.adjustment.remove",
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
                OrderCommand::App(app) => match &app.command {
                    OrderAppCommand::List => "order.app.list",
                    OrderAppCommand::Export(_) => "order.app.export",
                },
                OrderCommand::Rebind(_) => "order.rebind",
                OrderCommand::Accept(_) => "order.accept",
                OrderCommand::Decline(_) => "order.decline",
                OrderCommand::Cancel(_) => "order.cancel",
                OrderCommand::Revision(revision) => match &revision.command {
                    OrderRevisionCommand::Propose(_) => "order.revision.propose",
                    OrderRevisionCommand::Accept(_) => "order.revision.accept",
                    OrderRevisionCommand::Decline(_) => "order.revision.decline",
                },
                OrderCommand::Status(status) => match &status.command {
                    OrderStatusCommand::Get(_) => "order.status.get",
                },
                OrderCommand::Event(event) => match &event.command {
                    OrderEventCommand::List(_) => "order.event.list",
                    OrderEventCommand::Watch(_) => "order.event.watch",
                },
            },
            Self::Validation(args) => match &args.command {
                ValidationCommand::Receipt(receipt) => match &receipt.command {
                    ValidationReceiptCommand::Get(_) => "validation.receipt.get",
                    ValidationReceiptCommand::List(_) => "validation.receipt.list",
                    ValidationReceiptCommand::Verify(_) => "validation.receipt.verify",
                },
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::{CommandFactory, Parser};

    use super::{
        AccountCommand, FarmCommand, ListingCommand, OrderCommand, OrderRevisionCommand,
        TargetCliArgs, TargetOutputFormat, ValidationCommand, ValidationReceiptCommand,
    };
    use crate::registry::OPERATION_REGISTRY;

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
            "farm",
            "listing",
            "market",
            "basket",
            "order",
            "validation",
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
    fn target_parser_accepts_account_attach_secret_inputs() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "account",
            "attach-secret",
            "acct_test",
            "identity.json",
            "--default",
        ])
        .expect("target args parse");

        assert_eq!(parsed.command.operation_id(), "account.attach_secret");
        let crate::cli::TargetCommand::Account(account) = parsed.command else {
            panic!("expected account command")
        };
        let AccountCommand::AttachSecret(args) = account.command else {
            panic!("expected account attach-secret command")
        };
        assert_eq!(args.selector.as_deref(), Some("acct_test"));
        assert_eq!(
            args.path.as_ref().map(|path| path.as_os_str()),
            Some(std::ffi::OsStr::new("identity.json"))
        );
        assert!(args.default);
    }

    #[test]
    fn target_parser_accepts_farm_rebind_selector() {
        let parsed = TargetCliArgs::try_parse_from(["radroots", "farm", "rebind", "acct_test"])
            .expect("target args parse");

        assert_eq!(parsed.command.operation_id(), "farm.rebind");
        let crate::cli::TargetCommand::Farm(farm) = parsed.command else {
            panic!("expected farm command")
        };
        let FarmCommand::Rebind(args) = farm.command else {
            panic!("expected farm rebind command")
        };
        assert_eq!(args.selector.as_deref(), Some("acct_test"));
    }

    #[test]
    fn target_parser_accepts_listing_rebind_inputs() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "listing",
            "rebind",
            "listing.toml",
            "acct_test",
            "--farm-d-tag",
            "AAAAAAAAAAAAAAAAAAAAAw",
        ])
        .expect("target args parse");

        assert_eq!(parsed.command.operation_id(), "listing.rebind");
        let crate::cli::TargetCommand::Listing(listing) = parsed.command else {
            panic!("expected listing command")
        };
        let ListingCommand::Rebind(args) = listing.command else {
            panic!("expected listing rebind command")
        };
        assert_eq!(
            args.file.as_ref().map(|path| path.as_os_str()),
            Some(std::ffi::OsStr::new("listing.toml"))
        );
        assert_eq!(args.selector.as_deref(), Some("acct_test"));
        assert_eq!(args.farm_d_tag.as_deref(), Some("AAAAAAAAAAAAAAAAAAAAAw"));
    }

    #[test]
    fn target_parser_accepts_order_rebind_inputs() {
        let parsed =
            TargetCliArgs::try_parse_from(["radroots", "order", "rebind", "ord_test", "acct_test"])
                .expect("target args parse");

        assert_eq!(parsed.command.operation_id(), "order.rebind");
        let crate::cli::TargetCommand::Order(order) = parsed.command else {
            panic!("expected order command")
        };
        let OrderCommand::Rebind(args) = order.command else {
            panic!("expected order rebind command")
        };
        assert_eq!(args.order_id.as_deref(), Some("ord_test"));
        assert_eq!(args.selector.as_deref(), Some("acct_test"));
    }

    #[test]
    fn target_parser_accepts_order_cancel_reason() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "cancel",
            "ord_test",
            "--reason",
            "changed plans",
        ])
        .expect("target args parse");

        assert_eq!(parsed.command.operation_id(), "order.cancel");
        let crate::cli::TargetCommand::Order(order) = parsed.command else {
            panic!("expected order command")
        };
        let OrderCommand::Cancel(args) = order.command else {
            panic!("expected order cancel command")
        };
        assert_eq!(args.order_id.as_deref(), Some("ord_test"));
        assert_eq!(args.reason.as_deref(), Some("changed plans"));
    }

    #[test]
    fn target_parser_accepts_order_revision_propose_inputs() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "revision",
            "propose",
            "ord_test",
            "--reason",
            "update count",
            "--bin-id",
            "bin-1",
            "--bin-count",
            "3",
            "--adjustment-id",
            "adj_revision",
            "--adjustment-effect",
            "increase",
            "--adjustment-amount",
            "2",
            "--adjustment-currency",
            "USD",
            "--adjustment-reason",
            "packing change",
        ])
        .expect("target args parse");

        assert_eq!(parsed.command.operation_id(), "order.revision.propose");
        let crate::cli::TargetCommand::Order(order) = parsed.command else {
            panic!("expected order command")
        };
        let OrderCommand::Revision(revision) = order.command else {
            panic!("expected order revision command")
        };
        let OrderRevisionCommand::Propose(args) = revision.command else {
            panic!("expected order revision propose command")
        };
        assert_eq!(args.order_id.as_deref(), Some("ord_test"));
        assert_eq!(args.reason.as_deref(), Some("update count"));
        assert_eq!(args.bin_id.as_deref(), Some("bin-1"));
        assert_eq!(args.bin_count, Some(3));
        assert_eq!(args.adjustment_id.as_deref(), Some("adj_revision"));
        assert_eq!(args.adjustment_effect.as_deref(), Some("increase"));
    }

    #[test]
    fn target_parser_accepts_order_revision_decision_inputs() {
        let accepted = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "revision",
            "accept",
            "ord_test",
            "--revision-id",
            "rev_test",
        ])
        .expect("target args parse");

        assert_eq!(accepted.command.operation_id(), "order.revision.accept");
        let crate::cli::TargetCommand::Order(order) = accepted.command else {
            panic!("expected order command")
        };
        let OrderCommand::Revision(revision) = order.command else {
            panic!("expected order revision command")
        };
        let OrderRevisionCommand::Accept(args) = revision.command else {
            panic!("expected order revision accept command")
        };
        assert_eq!(args.order_id.as_deref(), Some("ord_test"));
        assert_eq!(args.revision_id.as_deref(), Some("rev_test"));

        let declined = TargetCliArgs::try_parse_from([
            "radroots",
            "order",
            "revision",
            "decline",
            "ord_test",
            "--revision-id",
            "rev_test",
            "--reason",
            "keep original order",
        ])
        .expect("target args parse");

        assert_eq!(declined.command.operation_id(), "order.revision.decline");
        let crate::cli::TargetCommand::Order(order) = declined.command else {
            panic!("expected order command")
        };
        let OrderCommand::Revision(revision) = order.command else {
            panic!("expected order revision command")
        };
        let OrderRevisionCommand::Decline(args) = revision.command else {
            panic!("expected order revision decline command")
        };
        assert_eq!(args.order_id.as_deref(), Some("ord_test"));
        assert_eq!(args.revision_id.as_deref(), Some("rev_test"));
        assert_eq!(args.reason.as_deref(), Some("keep original order"));
    }

    #[test]
    fn target_parser_accepts_validation_receipt_commands() {
        let get = TargetCliArgs::try_parse_from([
            "radroots",
            "validation",
            "receipt",
            "get",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ])
        .expect("target args parse");
        assert_eq!(get.command.operation_id(), "validation.receipt.get");
        let crate::cli::TargetCommand::Validation(validation) = get.command else {
            panic!("expected validation command")
        };
        let ValidationCommand::Receipt(receipt) = validation.command;
        let ValidationReceiptCommand::Get(args) = receipt.command else {
            panic!("expected validation receipt get command")
        };
        assert_eq!(
            args.receipt_event_id.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        let list = TargetCliArgs::try_parse_from([
            "radroots",
            "validation",
            "receipt",
            "list",
            "--order-id",
            "ord_1",
        ])
        .expect("target args parse");
        assert_eq!(list.command.operation_id(), "validation.receipt.list");
        let crate::cli::TargetCommand::Validation(validation) = list.command else {
            panic!("expected validation command")
        };
        let ValidationCommand::Receipt(receipt) = validation.command;
        let ValidationReceiptCommand::List(args) = receipt.command else {
            panic!("expected validation receipt list command")
        };
        assert_eq!(args.order_id.as_deref(), Some("ord_1"));

        let verify = TargetCliArgs::try_parse_from([
            "radroots",
            "validation",
            "receipt",
            "verify",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ])
        .expect("target args parse");
        assert_eq!(verify.command.operation_id(), "validation.receipt.verify");
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
