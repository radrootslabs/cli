pub mod doctor;
pub mod find;
pub mod identity;
pub mod job;
pub mod listing;
pub mod local;
pub mod myc;
pub mod net;
pub mod relay;
pub mod rpc;
pub mod runtime;
pub mod signer;
pub mod sync;

use crate::cli::{
    AccountCommand, Command, ConfigCommand, JobCommand, ListingCommand, LocalCommand, MycCommand,
    NetCommand, OrderCommand, RelayCommand, RpcCommand, SignerCommand, SyncCommand,
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
                runtime::show(config, logging),
            ))),
        },
        Command::Signer(signer) => match &signer.command {
            SignerCommand::Status => Ok(signer::status(config)),
        },
        Command::Doctor => doctor::report(config, logging),
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
        Command::Net(net) => match &net.command {
            NetCommand::Status => net::status(config),
        },
        Command::Order(order) => match &order.command {
            OrderCommand::New => unimplemented_command("order new"),
            OrderCommand::Get(_) => unimplemented_command("order get"),
            OrderCommand::Ls => unimplemented_command("order ls"),
            OrderCommand::Submit => unimplemented_command("order submit"),
            OrderCommand::Watch(_) => unimplemented_command("order watch"),
            OrderCommand::Cancel(_) => unimplemented_command("order cancel"),
            OrderCommand::History => unimplemented_command("order history"),
        },
        Command::Relay(relay) => match &relay.command {
            RelayCommand::Ls => Ok(relay::list(config)),
        },
        Command::Rpc(rpc) => match &rpc.command {
            RpcCommand::Status => Ok(rpc::status(config)),
            RpcCommand::Sessions => Ok(rpc::sessions(config)),
        },
        Command::Sync(sync) => match &sync.command {
            SyncCommand::Status => sync::status(config),
            SyncCommand::Pull => sync::pull(config),
            SyncCommand::Push => sync::push(config),
            SyncCommand::Watch(args) => sync::watch(config, args),
        },
    }
}

fn unimplemented_command(name: &str) -> Result<CommandOutput, RuntimeError> {
    Err(RuntimeError::Config(format!(
        "`{name}` is not implemented yet"
    )))
}
