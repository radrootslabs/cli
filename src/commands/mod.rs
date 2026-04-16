pub mod doctor;
pub mod farm;
pub mod find;
pub mod identity;
pub mod job;
pub mod listing;
pub mod local;
pub mod myc;
pub mod net;
pub mod order;
pub mod relay;
pub mod rpc;
pub mod runtime;
pub mod signer;
pub mod sync;
pub mod workflow;

use crate::cli::{
    AccountCommand, Command, ConfigCommand, FarmCommand, JobCommand, ListingCommand, LocalCommand,
    MarketCommand, MycCommand, NetCommand, OrderCommand, RelayCommand, RpcCommand, RuntimeCommand,
    RuntimeConfigCommand, SellCommand, SignerCommand, SyncCommand,
};
use crate::domain::runtime::{CommandOutput, CommandView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::logging::LoggingState;

pub fn dispatch(
    command: &Command,
    config: &RuntimeConfig,
    logging: &LoggingState,
) -> Result<CommandOutput, RuntimeError> {
    match command {
        Command::Account(account) => match &account.command {
            AccountCommand::New => Ok(CommandOutput::success(CommandView::AccountNew(
                identity::init(config)?,
            ))),
            AccountCommand::Whoami => identity::show(config),
            AccountCommand::Ls => identity::list(config),
            AccountCommand::Use(args) => Ok(CommandOutput::success(CommandView::AccountUse(
                identity::use_account(config, args.selector.as_str())?,
            ))),
        },
        Command::Myc(myc) => match &myc.command {
            MycCommand::Status => Ok(myc::status(config)),
        },
        Command::Config(config_command) => match &config_command.command {
            ConfigCommand::Show => Ok(CommandOutput::success(CommandView::ConfigShow(
                runtime::show(config, logging)?,
            ))),
        },
        Command::Signer(signer) => match &signer.command {
            SignerCommand::Status => Ok(signer::status(config)),
        },
        Command::Doctor => doctor::report(config, logging),
        Command::Farm(farm_command) => match &farm_command.command {
            FarmCommand::Publish(args) => farm::publish(config, args),
            FarmCommand::Setup(args) => farm::setup(config, args),
            FarmCommand::Status(args) => farm::status(config, args),
            FarmCommand::Get(args) => farm::get(config, args),
        },
        Command::Find(find_args) => find::search(config, find_args),
        Command::Job(job) => match &job.command {
            JobCommand::Ls => Ok(job::list(config)),
            JobCommand::Get(args) => Ok(job::get(config, args.key.as_str())),
            JobCommand::Watch(args) => job::watch(config, args),
        },
        Command::Listing(listing) => match &listing.command {
            ListingCommand::New(args) => listing::new(config, args),
            ListingCommand::Validate(args) => listing::validate(config, args),
            ListingCommand::Get(args) => listing::get(config, args),
            ListingCommand::Publish(args) => listing::publish(config, args),
            ListingCommand::Update(args) => listing::update(config, args),
            ListingCommand::Archive(args) => listing::archive(config, args),
        },
        Command::Local(local) => match &local.command {
            LocalCommand::Init => local::init(config),
            LocalCommand::Status => local::status(config),
            LocalCommand::Export(args) => local::export(config, args),
            LocalCommand::Backup(args) => local::backup(config, args),
        },
        Command::Market(market) => match &market.command {
            MarketCommand::Update => sync::pull(config),
            MarketCommand::Search(args) => find::search(config, args),
            MarketCommand::View(args) => listing::get(config, args),
        },
        Command::Net(net) => match &net.command {
            NetCommand::Status => net::status(config),
        },
        Command::Order(order) => match &order.command {
            OrderCommand::New(args) => order::new(config, args),
            OrderCommand::Get(args) => order::get(config, args),
            OrderCommand::Ls => order::list(config),
            OrderCommand::Submit(args) => order::submit(config, args),
            OrderCommand::Watch(args) => order::watch(config, args),
            OrderCommand::Cancel(args) => order::cancel(config, args),
            OrderCommand::History => order::history(config),
        },
        Command::Relay(relay) => match &relay.command {
            RelayCommand::Ls => Ok(relay::list(config)),
        },
        Command::Rpc(rpc) => match &rpc.command {
            RpcCommand::Status => Ok(rpc::status(config)),
            RpcCommand::Sessions => Ok(rpc::sessions(config)),
        },
        Command::Sell(sell) => match &sell.command {
            SellCommand::Add(args) => listing::new(config, args),
            SellCommand::Show(_args) => planned_command(
                "`sell show` will inspect local drafts in the next slice; use `listing validate <file>` for now",
            ),
            SellCommand::Check(args) => listing::validate(config, args),
            SellCommand::Publish(args) => listing::publish(config, args),
            SellCommand::Update(args) => listing::update(config, args),
            SellCommand::Pause(args) => listing::archive(config, args),
            SellCommand::Reprice(_args) => planned_command(
                "`sell reprice` will land in the draft-mutation slice; edit the draft file directly for now",
            ),
            SellCommand::Restock(_args) => planned_command(
                "`sell restock` will land in the draft-mutation slice; edit the draft file directly for now",
            ),
        },
        Command::Setup(setup) => workflow::setup(config, setup),
        Command::Runtime(runtime_command) => match &runtime_command.command {
            RuntimeCommand::Install(args) => runtime::install(config, args),
            RuntimeCommand::Uninstall(args) => runtime::uninstall(config, args),
            RuntimeCommand::Status(args) => runtime::status(config, args),
            RuntimeCommand::Start(args) => runtime::start(config, args),
            RuntimeCommand::Stop(args) => runtime::stop(config, args),
            RuntimeCommand::Restart(args) => runtime::restart(config, args),
            RuntimeCommand::Logs(args) => runtime::logs(config, args),
            RuntimeCommand::Config(runtime_config) => match &runtime_config.command {
                RuntimeConfigCommand::Show(args) => runtime::config_show(config, logging, args),
                RuntimeConfigCommand::Set(args) => runtime::config_set(config, args),
            },
        },
        Command::Status => workflow::status(config),
        Command::Sync(sync) => match &sync.command {
            SyncCommand::Status => sync::status(config),
            SyncCommand::Pull => sync::pull(config),
            SyncCommand::Push => sync::push(config),
            SyncCommand::Watch(args) => sync::watch(config, args),
        },
    }
}

fn planned_command(message: &str) -> Result<CommandOutput, RuntimeError> {
    Err(RuntimeError::Config(message.to_owned()))
}
