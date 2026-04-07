pub mod identity;
pub mod myc;
pub mod runtime;
pub mod signer;

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
            AccountCommand::Ls => unimplemented_command("account ls"),
            AccountCommand::Use(_) => unimplemented_command("account use"),
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
        Command::Doctor => unimplemented_command("doctor"),
        Command::Find(_) => unimplemented_command("find"),
        Command::Job(job) => match &job.command {
            JobCommand::Ls => unimplemented_command("job ls"),
            JobCommand::Get(_) => unimplemented_command("job get"),
            JobCommand::Watch(_) => unimplemented_command("job watch"),
        },
        Command::Listing(listing) => match &listing.command {
            ListingCommand::New => unimplemented_command("listing new"),
            ListingCommand::Validate => unimplemented_command("listing validate"),
            ListingCommand::Get(_) => unimplemented_command("listing get"),
            ListingCommand::Publish => unimplemented_command("listing publish"),
            ListingCommand::Update(_) => unimplemented_command("listing update"),
            ListingCommand::Archive(_) => unimplemented_command("listing archive"),
        },
        Command::Local(local) => match &local.command {
            LocalCommand::Init => unimplemented_command("local init"),
            LocalCommand::Status => unimplemented_command("local status"),
            LocalCommand::Export => unimplemented_command("local export"),
            LocalCommand::Backup => unimplemented_command("local backup"),
        },
        Command::Net(net) => match &net.command {
            NetCommand::Status => unimplemented_command("net status"),
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
            RelayCommand::Ls => unimplemented_command("relay ls"),
        },
        Command::Rpc(rpc) => match &rpc.command {
            RpcCommand::Status => unimplemented_command("rpc status"),
            RpcCommand::Sessions => unimplemented_command("rpc sessions"),
        },
        Command::Sync(sync) => match &sync.command {
            SyncCommand::Status => unimplemented_command("sync status"),
            SyncCommand::Pull => unimplemented_command("sync pull"),
            SyncCommand::Push => unimplemented_command("sync push"),
            SyncCommand::Watch => unimplemented_command("sync watch"),
        },
    }
}

fn unimplemented_command(name: &str) -> Result<CommandOutput, RuntimeError> {
    Err(RuntimeError::Config(format!(
        "`{name}` is not implemented yet"
    )))
}
