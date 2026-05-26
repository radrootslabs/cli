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
    AttachSecret(AccountAttachSecretArgs),
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
pub struct AccountAttachSecretArgs {
    pub selector: Option<String>,
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
