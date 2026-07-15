use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub command: AccountCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AccountCommand {
    Create,
    Import(AccountImportArgs),
    Select(AccountSelectorArgs),
    List,
    Remove(AccountSelectorArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AccountImportArgs {
    pub path: Option<PathBuf>,
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub default: bool,
}

#[derive(Debug, Clone, Args)]
pub struct AccountSelectorArgs {
    pub selector: Option<String>,
}
