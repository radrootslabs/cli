pub mod doctor;
pub mod farm;
pub mod find;
pub mod identity;
pub mod job;
pub mod listing;
pub mod local;
pub mod market;
pub mod myc;
pub mod net;
pub mod order;
pub mod relay;
pub mod rpc;
pub mod runtime;
pub mod sell;
pub mod signer;
pub mod sync;
pub mod workflow;

use crate::cli::{
    AccountCommand, Command, ConfigCommand, FarmCommand, JobCommand, ListingCommand, LocalCommand,
    MarketCommand, MycCommand, NetCommand, OrderCommand, RelayCommand, RpcCommand, RuntimeCommand,
    RuntimeConfigCommand, SellCommand, SignerCommand, SignerSessionCommand, SyncCommand,
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
            AccountCommand::Import(args) => Ok(CommandOutput::success(CommandView::AccountImport(
                identity::import(config, args)?,
            ))),
            AccountCommand::Whoami => identity::show(config),
            AccountCommand::Ls => identity::list(config),
            AccountCommand::Use(args) => Ok(CommandOutput::success(CommandView::AccountUse(
                identity::use_account(config, args.selector.as_str())?,
            ))),
            AccountCommand::ClearDefault => Ok(CommandOutput::success(
                CommandView::AccountClearDefault(identity::clear_default(config)?),
            )),
            AccountCommand::Remove(args) => Ok(CommandOutput::success(CommandView::AccountRemove(
                identity::remove(config, args.selector.as_str())?,
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
            SignerCommand::Session(session) => match &session.command {
                SignerSessionCommand::List => Ok(signer::session_list(config)),
                SignerSessionCommand::Show { session_id } => {
                    Ok(signer::session_show(config, session_id.as_str()))
                }
                SignerSessionCommand::ConnectBunker { url } => {
                    Ok(signer::session_connect_bunker(config, url.as_str()))
                }
                SignerSessionCommand::ConnectNostrconnect {
                    url,
                    client_secret_key,
                } => Ok(signer::session_connect_nostrconnect(
                    config,
                    url.as_str(),
                    client_secret_key.as_str(),
                )),
                SignerSessionCommand::PublicKey { session_id } => {
                    Ok(signer::session_public_key(config, session_id.as_str()))
                }
                SignerSessionCommand::Authorize { session_id } => {
                    Ok(signer::session_authorize(config, session_id.as_str()))
                }
                SignerSessionCommand::RequireAuth {
                    session_id,
                    auth_url,
                } => Ok(signer::session_require_auth(
                    config,
                    session_id.as_str(),
                    auth_url.as_str(),
                )),
                SignerSessionCommand::Close { session_id } => {
                    Ok(signer::session_close(config, session_id.as_str()))
                }
            },
        },
        Command::Doctor => doctor::report(config, logging),
        Command::Farm(farm_command) => match &farm_command.command {
            FarmCommand::Init(args) => farm::init(config, args),
            FarmCommand::Set(args) => farm::set(config, args),
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
            MarketCommand::Update => market::update(config),
            MarketCommand::Search(args) => market::search(config, args),
            MarketCommand::View(args) => market::view(config, args),
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
            SellCommand::Add(args) => sell::add(config, args),
            SellCommand::Show(args) => sell::show(config, args),
            SellCommand::Check(args) => sell::check(config, args),
            SellCommand::Publish(args) => sell::publish(config, args),
            SellCommand::Update(args) => sell::update(config, args),
            SellCommand::Pause(args) => sell::pause(config, args),
            SellCommand::Reprice(args) => sell::reprice(config, args),
            SellCommand::Restock(args) => sell::restock(config, args),
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
