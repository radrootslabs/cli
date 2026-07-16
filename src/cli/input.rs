use std::path::Path;

use crate::cli::global::{RuntimeInvocationArgs, RuntimeOutputFormatArg};
use crate::cli::{TargetCliArgs, TargetCommand, TargetOutputFormat};
use crate::ops::OperationData;
use serde_json::Value;

pub fn runtime_invocation_args_from_target(args: &TargetCliArgs) -> RuntimeInvocationArgs {
    RuntimeInvocationArgs {
        output_format: args.format.map(|format| match format {
            TargetOutputFormat::Terminal => RuntimeOutputFormatArg::Terminal,
            TargetOutputFormat::Json => RuntimeOutputFormatArg::Json,
            TargetOutputFormat::Ndjson => RuntimeOutputFormatArg::Ndjson,
        }),
        json: false,
        ndjson: false,
        env_file: None,
        quiet: args.quiet,
        verbose: args.verbose,
        trace: args.trace,
        dry_run: args.dry_run,
        no_input: args.no_input,
        yes: args.yes,
        log_filter: None,
        log_dir: None,
        log_stdout: false,
        no_log_stdout: false,
        account: args.account_id.clone(),
        identity_path: None,
        signer: None,
        myc_executable: None,
        myc_status_timeout_ms: None,
        hyf_enabled: false,
        no_hyf_enabled: false,
        hyf_executable: None,
    }
}

pub fn operation_id_from_target(args: &TargetCliArgs) -> &'static str {
    args.command.operation_id()
}

pub fn target_operation_input(command: &TargetCommand) -> OperationData {
    use crate::cli::{
        AccountCommand, BasketCommand, BasketItemCommand, FarmCommand, ListingCommand,
        MarketCommand, StoreCommand, TradeCancellationCommand, TradeCandidateCommand, TradeCommand,
        TradeEvidenceCommand, TradeOperationCommand, TradePrivateArtifactCommand,
        TradeProposalCommand, TradeRevisionCommand, TransportCapabilityCommand, TransportCommand,
        TransportConfigCommand, TransportDeliveryCommand, ValidationCommand,
    };

    let mut input = OperationData::new();
    match command {
        TargetCommand::Account(args) => match &args.command {
            AccountCommand::Import(args) => {
                insert_path(&mut input, "path", &args.path);
                if args.default {
                    input.insert("default".to_owned(), Value::Bool(true));
                }
            }
            AccountCommand::Select(args) => insert_string(&mut input, "selector", &args.selector),
            AccountCommand::Remove(args) => insert_string(&mut input, "selector", &args.selector),
            AccountCommand::Create | AccountCommand::List => {}
        },
        TargetCommand::Farm(args) => match &args.command {
            FarmCommand::Create(args) => {
                insert_string(&mut input, "farm_d_tag", &args.farm_d_tag);
                insert_string(&mut input, "name", &args.name);
                insert_string(&mut input, "display_name", &args.display_name);
                insert_string(&mut input, "about", &args.about);
                insert_string(&mut input, "website", &args.website);
                insert_string(&mut input, "picture", &args.picture);
                insert_string(&mut input, "banner", &args.banner);
                insert_string(&mut input, "location", &args.location);
                insert_string(&mut input, "city", &args.city);
                insert_string(&mut input, "region", &args.region);
                insert_string(&mut input, "country", &args.country);
                insert_string(&mut input, "geohash", &args.geohash);
                insert_string(&mut input, "delivery_method", &args.delivery_method);
            }
            FarmCommand::Update(args) => {
                insert_string(&mut input, "field", &args.field);
                insert_string(&mut input, "value", &args.value);
            }
            FarmCommand::Get | FarmCommand::List | FarmCommand::Publish => {}
        },
        TargetCommand::Listing(args) => match &args.command {
            ListingCommand::Create(args) => {
                insert_path(&mut input, "output", &args.output);
                insert_string(&mut input, "key", &args.key);
                insert_string(&mut input, "title", &args.title);
                insert_string(&mut input, "category", &args.category);
                insert_string(&mut input, "summary", &args.summary);
                insert_string(&mut input, "bin_id", &args.bin_id);
                insert_string(&mut input, "quantity_amount", &args.quantity_amount);
                insert_string(&mut input, "quantity_unit", &args.quantity_unit);
                insert_string(&mut input, "price_amount", &args.price_amount);
                insert_string(&mut input, "price_currency", &args.price_currency);
                insert_string(&mut input, "price_per_amount", &args.price_per_amount);
                insert_string(&mut input, "price_per_unit", &args.price_per_unit);
                insert_string(&mut input, "available", &args.available);
                insert_string(&mut input, "label", &args.label);
                insert_string(&mut input, "discount_id", &args.discount_id);
                insert_string(&mut input, "discount_label", &args.discount_label);
                insert_string(&mut input, "discount_kind", &args.discount_kind);
                insert_string(&mut input, "discount_value", &args.discount_value);
                insert_string(&mut input, "discount_amount", &args.discount_amount);
                insert_string(&mut input, "discount_currency", &args.discount_currency);
            }
            ListingCommand::Get(args) => insert_string(&mut input, "key", &args.key),
            ListingCommand::Update(args)
            | ListingCommand::Publish(args)
            | ListingCommand::Pause(args)
            | ListingCommand::Withdraw(args) => insert_path(&mut input, "file", &args.file),
            ListingCommand::List => {}
        },
        TargetCommand::Market(args) => match &args.command {
            MarketCommand::Pull => {}
            MarketCommand::Search(args) => {
                insert_string_array(&mut input, "query", args.query.as_slice())
            }
            MarketCommand::Get(args) => insert_string(&mut input, "key", &args.key),
        },
        TargetCommand::Store(args) => match &args.command {
            StoreCommand::Restore(args) => {
                insert_path(&mut input, "source", &Some(args.source.clone()));
                insert_path(&mut input, "destination", &args.destination);
                if args.overwrite {
                    input.insert("overwrite".to_owned(), Value::Bool(true));
                }
            }
            StoreCommand::Inspect | StoreCommand::Backup => {}
        },
        TargetCommand::Basket(args) => match &args.command {
            BasketCommand::Create(args) => {
                insert_string(&mut input, "basket_id", &args.basket_id);
                insert_string(&mut input, "listing", &args.listing);
                insert_string(&mut input, "listing_addr", &args.listing_addr);
                insert_string(&mut input, "bin_id", &args.bin_id);
                insert_string(&mut input, "quantity", &args.quantity);
            }
            BasketCommand::Get(args) => insert_string(&mut input, "basket_id", &args.basket_id),
            BasketCommand::Item(item) => match &item.command {
                BasketItemCommand::Add(args) | BasketItemCommand::Update(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id);
                    insert_string(&mut input, "item_id", &args.item_id);
                    insert_string(&mut input, "listing", &args.listing);
                    insert_string(&mut input, "listing_addr", &args.listing_addr);
                    insert_string(&mut input, "bin_id", &args.bin_id);
                    insert_string(&mut input, "quantity", &args.quantity);
                }
                BasketItemCommand::Remove(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id);
                    insert_string(&mut input, "item_id", &args.item_id);
                }
            },
            BasketCommand::Quote(args) => insert_string(&mut input, "basket_id", &args.basket_id),
            BasketCommand::List => {}
        },
        TargetCommand::Trade(args) => match &args.command {
            TradeCommand::Proposal(args) => match &args.command {
                TradeProposalCommand::Submit(args) => {
                    insert_path_required(&mut input, "file", &args.file)
                }
            },
            TradeCommand::Revision(args) => match &args.command {
                TradeRevisionCommand::Propose(args) => {
                    insert_path_required(&mut input, "file", &args.file)
                }
            },
            TradeCommand::Candidate(args) => match &args.command {
                TradeCandidateCommand::Decide(args) => {
                    insert_path_required(&mut input, "file", &args.file);
                    insert_bool(
                        &mut input,
                        "acknowledge_private_terms",
                        args.acknowledge_private_terms,
                    );
                }
            },
            TradeCommand::Cancellation(args) => match &args.command {
                TradeCancellationCommand::Submit(args) => {
                    insert_path_required(&mut input, "file", &args.file)
                }
            },
            TradeCommand::Operation(args) => match &args.command {
                TradeOperationCommand::Resume(args) => {
                    insert_path_required(&mut input, "file", &args.file);
                    if let Some(operation_kind) = args.operation_kind {
                        input.insert(
                            "operation_kind".to_owned(),
                            Value::String(operation_kind.as_sdk_operation_kind().to_owned()),
                        );
                    }
                    insert_bool(
                        &mut input,
                        "acknowledge_private_terms",
                        args.acknowledge_private_terms,
                    );
                }
            },
            TradeCommand::Get(args) => insert_string(&mut input, "trade_id", &args.trade_id),
            TradeCommand::List(args) => {
                insert_u32(&mut input, "limit", args.limit);
                insert_string(&mut input, "cursor", &args.cursor);
            }
            TradeCommand::Evidence(args) => match &args.command {
                TradeEvidenceCommand::Refresh(args) => {
                    insert_string(&mut input, "trade_id", &args.trade_id)
                }
                TradeEvidenceCommand::Inspect(args) => {
                    insert_string(&mut input, "trade_id", &args.trade_id);
                    insert_u32(&mut input, "limit", args.limit);
                    insert_string(&mut input, "cursor", &args.cursor);
                }
            },
            TradeCommand::PrivateArtifact(args) => match &args.command {
                TradePrivateArtifactCommand::Seal(args) => {
                    insert_string(&mut input, "trade_id", &args.trade_id);
                    insert_string(&mut input, "artifact_id", &args.artifact_id);
                    insert_string(&mut input, "schema_id", &args.schema_id);
                    insert_path(&mut input, "input", &args.input);
                    input.insert(
                        "kind".to_owned(),
                        Value::String(args.kind.as_str().to_owned()),
                    );
                    insert_string(&mut input, "candidate_id", &args.candidate_id);
                    insert_string(&mut input, "retention_class", &args.retention_class);
                    insert_i64(&mut input, "expires_at_ms", args.expires_at_ms);
                }
                TradePrivateArtifactCommand::Open(args) => {
                    input.insert(
                        "artifact_id".to_owned(),
                        Value::String(args.artifact_id.clone()),
                    );
                    insert_path(&mut input, "output", &args.output);
                }
                TradePrivateArtifactCommand::Delete(args) => {
                    input.insert(
                        "artifact_id".to_owned(),
                        Value::String(args.artifact_id.clone()),
                    );
                }
            },
        },
        TargetCommand::Validation(args) => match &args.command {
            ValidationCommand::Status => {}
        },
        TargetCommand::Transport(args) => match &args.command {
            TransportCommand::Config(config) => match &config.command {
                TransportConfigCommand::Update(args) => {
                    input.insert(
                        "kind".to_owned(),
                        Value::String(args.kind.as_str().to_owned()),
                    );
                    insert_string_array(&mut input, "nostr_relays", args.nostr_relay.as_slice());
                    if let Some(behavior) = args.reticulum_behavior {
                        input.insert(
                            "reticulum_behavior".to_owned(),
                            Value::String(behavior.as_str().to_owned()),
                        );
                    }
                    insert_string(&mut input, "reticulum_scope", &args.reticulum_scope);
                    insert_string(
                        &mut input,
                        "reticulum_agent_endpoint",
                        &args.reticulum_agent_endpoint,
                    );
                }
                TransportConfigCommand::Inspect => {}
            },
            TransportCommand::Capability(capability) => match capability.command {
                TransportCapabilityCommand::List => {}
            },
            TransportCommand::Delivery(delivery) => match delivery.command {
                TransportDeliveryCommand::Inspect | TransportDeliveryCommand::Retry => {}
            },
            TransportCommand::Status(_) => {}
        },
        _ => {}
    }
    input
}

fn insert_string(input: &mut OperationData, key: &str, value: &Option<String>) {
    if let Some(value) = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        input.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn insert_string_array(input: &mut OperationData, key: &str, values: &[String]) {
    let values = values
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Value::String(value.to_owned()))
        .collect::<Vec<_>>();
    if !values.is_empty() {
        input.insert(key.to_owned(), Value::Array(values));
    }
}

fn insert_bool(input: &mut OperationData, key: &str, value: bool) {
    if value {
        input.insert(key.to_owned(), Value::Bool(true));
    }
}

fn insert_path(input: &mut OperationData, key: &str, value: &Option<std::path::PathBuf>) {
    if let Some(value) = value {
        input.insert(
            key.to_owned(),
            Value::String(value.to_string_lossy().into_owned()),
        );
    }
}

fn insert_path_required(input: &mut OperationData, key: &str, value: &Path) {
    input.insert(
        key.to_owned(),
        Value::String(value.to_string_lossy().into_owned()),
    );
}

fn insert_u32(input: &mut OperationData, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        input.insert(key.to_owned(), Value::Number(value.into()));
    }
}

fn insert_i64(input: &mut OperationData, key: &str, value: Option<i64>) {
    if let Some(value) = value {
        input.insert(key.to_owned(), Value::Number(value.into()));
    }
}
