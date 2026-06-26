use crate::cli::global::{RuntimeInvocationArgs, RuntimeOutputFormatArg};
use crate::cli::{TargetCliArgs, TargetCommand, TargetOutputFormat};
use crate::ops::OperationData;
use serde_json::Value;

pub fn runtime_invocation_args_from_target(args: &TargetCliArgs) -> RuntimeInvocationArgs {
    RuntimeInvocationArgs {
        output_format: Some(match args.format {
            TargetOutputFormat::Human => RuntimeOutputFormatArg::Human,
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
        no_color: args.no_color,
        no_input: args.no_input,
        yes: false,
        log_filter: None,
        log_dir: None,
        log_stdout: false,
        no_log_stdout: false,
        account: args.account_id.clone(),
        identity_path: None,
        signer: None,
        publish_transport: args.publish_transport.map(|mode| mode.as_str().to_owned()),
        relay: args.relay.clone(),
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
        AccountCommand, AccountSelectionCommand, BasketAdjustmentCommand, BasketCommand,
        BasketItemCommand, BasketQuoteCommand, FarmCommand, FarmFulfillmentCommand,
        FarmLocationCommand, FarmProfileCommand, ListingAppCommand, ListingCommand, MarketCommand,
        MarketListingCommand, MarketProductCommand, OrderAppCommand, OrderCommand,
        OrderEventCommand, OrderRevisionCommand, OrderStatusCommand, StoreBackupCommand,
        StoreCommand, ValidationCommand, ValidationReceiptCommand,
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
            AccountCommand::AttachSecret(args) => {
                insert_string(&mut input, "selector", &args.selector);
                insert_path(&mut input, "path", &args.path);
                if args.default {
                    input.insert("default".to_owned(), Value::Bool(true));
                }
            }
            AccountCommand::Get(args) => insert_string(&mut input, "selector", &args.selector),
            AccountCommand::Remove(args) => insert_string(&mut input, "selector", &args.selector),
            AccountCommand::Selection(args) => match &args.command {
                AccountSelectionCommand::Update(args) => {
                    insert_string(&mut input, "selector", &args.selector)
                }
                AccountSelectionCommand::Get | AccountSelectionCommand::Clear => {}
            },
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
            FarmCommand::Rebind(args) => {
                insert_string(&mut input, "selector", &args.selector);
            }
            FarmCommand::Profile(args) => match &args.command {
                FarmProfileCommand::Update(args) => {
                    insert_string(&mut input, "field", &args.field);
                    insert_string(&mut input, "value", &args.value);
                }
            },
            FarmCommand::Location(args) => match &args.command {
                FarmLocationCommand::Update(args) => {
                    insert_string(&mut input, "field", &args.field);
                    insert_string(&mut input, "value", &args.value);
                }
            },
            FarmCommand::Fulfillment(args) => match &args.command {
                FarmFulfillmentCommand::Update(args) => {
                    insert_string(&mut input, "value", &args.value);
                }
            },
            FarmCommand::Get | FarmCommand::Readiness(_) | FarmCommand::Publish => {}
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
            ListingCommand::App(args) => match &args.command {
                ListingAppCommand::Export(args) => {
                    insert_string(&mut input, "record_id", &args.record_id);
                    insert_path(&mut input, "output", &args.output);
                }
                ListingAppCommand::List => {}
            },
            ListingCommand::Update(args)
            | ListingCommand::Validate(args)
            | ListingCommand::Publish(args)
            | ListingCommand::Archive(args) => insert_path(&mut input, "file", &args.file),
            ListingCommand::Rebind(args) => {
                insert_path(&mut input, "file", &args.file);
                insert_string(&mut input, "selector", &args.selector);
                insert_string(&mut input, "farm_d_tag", &args.farm_d_tag);
            }
            ListingCommand::List => {}
        },
        TargetCommand::Market(args) => match &args.command {
            MarketCommand::Product(product) => match &product.command {
                MarketProductCommand::Search(args) => {
                    insert_string_array(&mut input, "query", args.query.as_slice())
                }
            },
            MarketCommand::Listing(listing) => match &listing.command {
                MarketListingCommand::Get(args) => insert_string(&mut input, "key", &args.key),
            },
            MarketCommand::Refresh => {}
        },
        TargetCommand::Store(args) => match &args.command {
            StoreCommand::Backup(backup) => match &backup.command {
                StoreBackupCommand::Restore(args) => {
                    insert_path(&mut input, "source", &Some(args.source.clone()));
                    insert_path(&mut input, "destination", &args.destination);
                    if args.overwrite {
                        input.insert("overwrite".to_owned(), Value::Bool(true));
                    }
                }
                StoreBackupCommand::Create => {}
            },
            StoreCommand::Init | StoreCommand::Status(_) | StoreCommand::Export => {}
        },
        TargetCommand::Basket(args) => match &args.command {
            BasketCommand::Create(args) => {
                insert_string(&mut input, "basket_id", &args.basket_id);
                insert_string(&mut input, "listing", &args.listing);
                insert_string(&mut input, "listing_addr", &args.listing_addr);
                insert_string(&mut input, "bin_id", &args.bin_id);
                insert_string(&mut input, "quantity", &args.quantity);
            }
            BasketCommand::Get(args) | BasketCommand::Validate(args) => {
                insert_string(&mut input, "basket_id", &args.basket_id)
            }
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
            BasketCommand::Adjustment(adjustment) => match &adjustment.command {
                BasketAdjustmentCommand::Add(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id);
                    insert_string(&mut input, "id", &args.id);
                    insert_string(&mut input, "effect", &args.effect);
                    insert_string(&mut input, "amount", &args.amount);
                    insert_string(&mut input, "currency", &args.currency);
                    insert_string(&mut input, "reason", &args.reason);
                }
                BasketAdjustmentCommand::Remove(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id);
                    insert_string(&mut input, "id", &args.id);
                }
            },
            BasketCommand::Quote(quote) => match &quote.command {
                BasketQuoteCommand::Create(args) => {
                    insert_string(&mut input, "basket_id", &args.basket_id)
                }
            },
            BasketCommand::List => {}
        },
        TargetCommand::Order(args) => match &args.command {
            OrderCommand::Submit(args) => {
                insert_string(&mut input, "order_id", &args.order_id);
            }
            OrderCommand::Get(args) => insert_string(&mut input, "order_id", &args.order_id),
            OrderCommand::App(args) => match &args.command {
                OrderAppCommand::Export(args) => {
                    insert_string(&mut input, "record_id", &args.record_id);
                    insert_path(&mut input, "output", &args.output);
                }
                OrderAppCommand::List => {}
            },
            OrderCommand::Rebind(args) => {
                insert_string(&mut input, "order_id", &args.order_id);
                insert_string(&mut input, "selector", &args.selector);
            }
            OrderCommand::Accept(args) => insert_string(&mut input, "order_id", &args.order_id),
            OrderCommand::Decline(args) => {
                insert_string(&mut input, "order_id", &args.order_id);
                insert_string(&mut input, "reason", &args.reason);
            }
            OrderCommand::Cancel(args) => {
                insert_string(&mut input, "order_id", &args.order_id);
                insert_string(&mut input, "reason", &args.reason);
            }
            OrderCommand::Revision(revision) => match &revision.command {
                OrderRevisionCommand::Propose(args) => {
                    insert_string(&mut input, "order_id", &args.order_id);
                    insert_string(&mut input, "reason", &args.reason);
                    insert_string(&mut input, "bin_id", &args.bin_id);
                    if let Some(bin_count) = args.bin_count {
                        input.insert(
                            "bin_count".to_owned(),
                            Value::Number(serde_json::Number::from(bin_count)),
                        );
                    }
                    insert_string(&mut input, "adjustment_id", &args.adjustment_id);
                    insert_string(&mut input, "adjustment_effect", &args.adjustment_effect);
                    insert_string(&mut input, "adjustment_amount", &args.adjustment_amount);
                    insert_string(&mut input, "adjustment_currency", &args.adjustment_currency);
                    insert_string(&mut input, "adjustment_reason", &args.adjustment_reason);
                }
                OrderRevisionCommand::Accept(args) => {
                    insert_string(&mut input, "order_id", &args.order_id);
                    insert_string(&mut input, "revision_id", &args.revision_id);
                }
                OrderRevisionCommand::Decline(args) => {
                    insert_string(&mut input, "order_id", &args.order_id);
                    insert_string(&mut input, "revision_id", &args.revision_id);
                    insert_string(&mut input, "reason", &args.reason);
                }
            },
            OrderCommand::Status(status) => match &status.command {
                OrderStatusCommand::Get(args) => {
                    insert_string(&mut input, "order_id", &args.order_id)
                }
            },
            OrderCommand::Event(event) => match &event.command {
                OrderEventCommand::List(args) | OrderEventCommand::Watch(args) => {
                    insert_string(&mut input, "order_id", &args.order_id)
                }
            },
            OrderCommand::List => {}
        },
        TargetCommand::Validation(args) => match &args.command {
            ValidationCommand::Receipt(receipt) => match &receipt.command {
                ValidationReceiptCommand::Get(args) | ValidationReceiptCommand::Verify(args) => {
                    insert_string(&mut input, "receipt_event_id", &args.receipt_event_id);
                }
                ValidationReceiptCommand::List(args) => {
                    insert_string(&mut input, "order_id", &args.order_id);
                }
            },
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

fn insert_path(input: &mut OperationData, key: &str, value: &Option<std::path::PathBuf>) {
    if let Some(value) = value {
        input.insert(
            key.to_owned(),
            Value::String(value.to_string_lossy().into_owned()),
        );
    }
}
