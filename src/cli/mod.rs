pub mod global;

pub mod account;
pub mod basket;
pub mod diagnostics;
pub mod farm;
pub mod health;
pub mod input;
pub mod listing;
pub mod market;
pub mod profile;
pub mod signer;
pub mod store;
pub mod sync;
pub mod trade;
pub mod transport;
pub mod validation;

pub use account::*;
pub use basket::*;
pub use diagnostics::*;
pub use farm::*;
pub use health::*;
pub use listing::*;
pub use market::*;
pub use profile::*;
pub use signer::*;
pub use store::*;
pub use sync::*;
pub use trade::*;
pub use transport::*;
pub use validation::*;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TargetOutputFormat {
    Terminal,
    Json,
    Ndjson,
}

#[derive(Debug, Parser, Clone)]
#[command(
    name = "radroots",
    about = "Operate Radroots local-first trade workflows.",
    long_about = "Operate Radroots local-first trade workflows.",
    disable_help_subcommand = true
)]
pub struct TargetCliArgs {
    #[arg(long = "format", global = true, value_enum)]
    pub format: Option<TargetOutputFormat>,
    #[arg(long = "account-id", global = true)]
    pub account_id: Option<String>,
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
    #[arg(long = "yes", global = true, action = ArgAction::SetTrue)]
    pub yes: bool,
    #[arg(long = "approval-proof", global = true)]
    pub approval_proof: Option<String>,
    #[arg(long = "no-input", global = true, action = ArgAction::SetTrue)]
    pub no_input: bool,
    #[arg(long = "quiet", global = true, action = ArgAction::SetTrue)]
    pub quiet: bool,
    #[arg(long = "verbose", global = true, action = ArgAction::SetTrue)]
    pub verbose: bool,
    #[arg(long = "trace", global = true, action = ArgAction::SetTrue)]
    pub trace: bool,
    #[command(subcommand)]
    pub command: TargetCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TargetCommand {
    #[command(about = "Inspect and reset runtime profile state.")]
    Profile(ProfileArgs),
    #[command(about = "Inspect local readiness and mode-specific recovery steps.")]
    Health(HealthArgs),
    #[command(about = "Manage local signer accounts and custody.")]
    Account(AccountArgs),
    #[command(about = "Inspect signer readiness for local relay writes.")]
    Signer(SignerArgs),
    #[command(about = "Manage transport profiles and outbox delivery.")]
    Transport(TransportArgs),
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
    #[command(about = "Prepare baskets and quotes before trade coordination.")]
    Basket(BasketArgs),
    #[command(about = "Coordinate buyer and farmer trade agreement events.")]
    Trade(TradeArgs),
    #[command(about = "Inspect validation receipts and proof state.")]
    Validation(ValidationArgs),
    #[command(about = "Inspect runtime diagnostics.")]
    Diagnostics(DiagnosticsArgs),
}

impl TargetCommand {
    pub fn operation_id(&self) -> &'static str {
        match self {
            Self::Profile(args) => match args.command {
                ProfileCommand::Inspect => "profile.inspect",
                ProfileCommand::Reset => "profile.reset",
            },
            Self::Health(args) => match args.command {
                HealthCommand::Inspect => "health.inspect",
            },
            Self::Account(args) => match &args.command {
                AccountCommand::Create => "account.create",
                AccountCommand::Import(_) => "account.import",
                AccountCommand::Select(_) => "account.select",
                AccountCommand::List => "account.list",
                AccountCommand::Remove(_) => "account.remove",
            },
            Self::Signer(args) => match args.command {
                SignerCommand::Status => "signer.status",
            },
            Self::Transport(args) => match &args.command {
                TransportCommand::Capability(capability) => match capability.command {
                    TransportCapabilityCommand::List => "transport.capability.list",
                },
                TransportCommand::Config(config) => match &config.command {
                    TransportConfigCommand::Inspect => "transport.config.inspect",
                    TransportConfigCommand::Update(_) => "transport.config.update",
                },
                TransportCommand::Status(status) => match status.command {
                    TransportStatusCommand::Inspect => "transport.status.inspect",
                },
                TransportCommand::Delivery(delivery) => match delivery.command {
                    TransportDeliveryCommand::Inspect => "transport.delivery.inspect",
                    TransportDeliveryCommand::Retry => "transport.delivery.retry",
                },
            },
            Self::Store(args) => match &args.command {
                StoreCommand::Inspect => "store.inspect",
                StoreCommand::Backup => "store.backup",
                StoreCommand::Restore(_) => "store.restore",
            },
            Self::Sync(args) => match &args.command {
                SyncCommand::Status => "sync.status",
                SyncCommand::Pull => "sync.pull",
                SyncCommand::Push => "sync.push",
            },
            Self::Farm(args) => match &args.command {
                FarmCommand::Create(_) => "farm.create",
                FarmCommand::Update(_) => "farm.update",
                FarmCommand::Publish => "farm.publish",
                FarmCommand::Get => "farm.get",
                FarmCommand::List => "farm.list",
            },
            Self::Listing(args) => match &args.command {
                ListingCommand::Create(_) => "listing.create",
                ListingCommand::Update(_) => "listing.update",
                ListingCommand::Publish(_) => "listing.publish",
                ListingCommand::Pause(_) => "listing.pause",
                ListingCommand::Withdraw(_) => "listing.withdraw",
                ListingCommand::Get(_) => "listing.get",
                ListingCommand::List => "listing.list",
            },
            Self::Market(args) => match &args.command {
                MarketCommand::Pull => "market.pull",
                MarketCommand::Search(_) => "market.search",
                MarketCommand::Get(_) => "market.get",
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
                BasketCommand::Quote(_) => "basket.quote",
            },
            Self::Trade(args) => match &args.command {
                TradeCommand::Request(_) => "trade.request",
                TradeCommand::Get(_) => "trade.get",
                TradeCommand::List => "trade.list",
                TradeCommand::Accept(_) => "trade.accept",
                TradeCommand::Decline(_) => "trade.decline",
                TradeCommand::Cancel(_) => "trade.cancel",
            },
            Self::Validation(args) => match &args.command {
                ValidationCommand::Status => "validation.status",
                ValidationCommand::Receipt(receipt) => match &receipt.command {
                    ValidationReceiptCommand::Get(_) => "validation.receipt.get",
                    ValidationReceiptCommand::Verify(_) => "validation.receipt.verify",
                },
            },
            Self::Diagnostics(args) => match args.command {
                DiagnosticsCommand::Inspect => "diagnostics.inspect",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::{CommandFactory, Parser};

    use super::{TargetCliArgs, TargetCommand, TargetOutputFormat};
    use crate::registry::OPERATION_REGISTRY;

    #[test]
    fn target_parser_accepts_every_generated_registry_path() {
        for operation in OPERATION_REGISTRY {
            let parsed = TargetCliArgs::try_parse_from(operation.cli_path.split_whitespace())
                .unwrap_or_else(|error| {
                    panic!("{} failed to parse: {error}", operation.cli_path);
                });
            assert_eq!(parsed.command.operation_id(), operation.operation_id);
        }
    }

    #[test]
    fn target_parser_exposes_only_v1_namespaces() {
        let actual = TargetCliArgs::command()
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect::<BTreeSet<_>>();
        let expected = [
            "profile",
            "health",
            "account",
            "signer",
            "transport",
            "store",
            "sync",
            "farm",
            "listing",
            "market",
            "basket",
            "trade",
            "validation",
            "diagnostics",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn target_global_flags_parse_v1_approval_inputs() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "--format",
            "ndjson",
            "--account-id",
            "acct_test",
            "--offline",
            "--dry-run",
            "--idempotency-key",
            "018f3d99-7d35-7c0c-8a0f-7f3b645abcde",
            "--correlation-id",
            "corr_test",
            "--yes",
            "--approval-proof",
            "{\"operation_id\":\"profile.reset\"}",
            "--no-input",
            "--quiet",
            "profile",
            "inspect",
        ])
        .expect("target args parse");

        assert_eq!(parsed.format, Some(TargetOutputFormat::Ndjson));
        assert_eq!(parsed.account_id.as_deref(), Some("acct_test"));
        assert!(parsed.offline);
        assert!(parsed.dry_run);
        assert_eq!(
            parsed.idempotency_key.as_deref(),
            Some("018f3d99-7d35-7c0c-8a0f-7f3b645abcde")
        );
        assert_eq!(parsed.correlation_id.as_deref(), Some("corr_test"));
        assert!(parsed.yes);
        assert!(parsed.approval_proof.is_some());
        assert!(parsed.no_input);
        assert!(parsed.quiet);
        assert_eq!(parsed.command.operation_id(), "profile.inspect");
    }

    #[test]
    fn target_parser_rejects_retired_commands_and_global_flags() {
        let rejected = [
            vec![
                "radroots".to_owned(),
                format!("--approval-{}", "token"),
                "approve".to_owned(),
                "profile".to_owned(),
                "reset".to_owned(),
            ],
            vec![
                "radroots".to_owned(),
                "workspace".to_owned(),
                "get".to_owned(),
            ],
            vec!["radroots".to_owned(), "config".to_owned(), "get".to_owned()],
            vec![
                "radroots".to_owned(),
                "mesh".to_owned(),
                "status".to_owned(),
            ],
            vec![
                "radroots".to_owned(),
                "trade".to_owned(),
                "submit".to_owned(),
            ],
            vec![
                "radroots".to_owned(),
                "listing".to_owned(),
                "archive".to_owned(),
            ],
            vec![
                "radroots".to_owned(),
                "validation".to_owned(),
                "receipt".to_owned(),
                "list".to_owned(),
            ],
        ];

        for args in rejected {
            assert!(
                TargetCliArgs::try_parse_from(args.iter().map(String::as_str)).is_err(),
                "{args:?}"
            );
        }
    }

    #[test]
    fn target_parser_maps_v1_operation_inputs() {
        let parsed = TargetCliArgs::try_parse_from([
            "radroots",
            "transport",
            "config",
            "update",
            "--kind",
            "multi-target",
            "--nostr-relay",
            "wss://relay.example",
        ])
        .expect("transport config update parses");
        assert_eq!(parsed.command.operation_id(), "transport.config.update");

        let TargetCommand::Transport(_) = parsed.command else {
            panic!("expected transport command")
        };

        let trade = TargetCliArgs::try_parse_from([
            "radroots",
            "trade",
            "request",
            "trade_test",
            "--confirm-public-note",
        ])
        .expect("trade request parses");
        assert_eq!(trade.command.operation_id(), "trade.request");
    }
}
