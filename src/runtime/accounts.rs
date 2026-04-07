use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use radroots_identity::RadrootsIdentityId;
use radroots_nostr_accounts::prelude::{
    RadrootsNostrAccountRecord, RadrootsNostrAccountsManager, RadrootsNostrFileAccountStore,
    RadrootsNostrSecretVault, RadrootsNostrSelectedAccountStatus,
};

use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

#[derive(Debug, Clone)]
pub struct AccountSnapshot {
    pub accounts: Vec<AccountRecordView>,
}

#[derive(Debug, Clone)]
pub struct AccountRecordView {
    pub record: RadrootsNostrAccountRecord,
    pub selected: bool,
    pub signer: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountCreateMode {
    Created,
    Migrated,
}

#[derive(Debug, Clone)]
pub struct AccountCreateResult {
    pub mode: AccountCreateMode,
    pub account: AccountRecordView,
}

pub fn create_or_migrate_selected_account(
    config: &RuntimeConfig,
) -> Result<AccountCreateResult, RuntimeError> {
    let manager = account_manager(config)?;
    let existing = manager.list_accounts()?;
    let mode = if existing.is_empty() && config.identity.path.exists() {
        manager.migrate_legacy_identity_file(&config.identity.path, None, true)?;
        AccountCreateMode::Migrated
    } else {
        manager.generate_identity(None, true)?;
        AccountCreateMode::Created
    };

    let snapshot = snapshot(config)?;
    let Some(account) = snapshot
        .accounts
        .into_iter()
        .find(|account| account.selected)
    else {
        return Err(RuntimeError::Accounts(
            radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::InvalidState(
                "selected account missing after account create".to_owned(),
            ),
        ));
    };

    Ok(AccountCreateResult { mode, account })
}

pub fn snapshot(config: &RuntimeConfig) -> Result<AccountSnapshot, RuntimeError> {
    let manager = account_manager(config)?;
    snapshot_from_manager(&manager)
}

pub fn resolve_account(config: &RuntimeConfig) -> Result<Option<AccountRecordView>, RuntimeError> {
    let snapshot = snapshot(config)?;
    if let Some(selector) = config.account.selector.as_deref() {
        let Some(account) = find_by_selector(snapshot.accounts.as_slice(), selector) else {
            return Err(RuntimeError::Config(format!(
                "account selector `{selector}` did not match any local account"
            )));
        };
        return Ok(Some(account.clone()));
    }

    Ok(snapshot
        .accounts
        .into_iter()
        .find(|account| account.selected))
}

pub fn select_account(
    config: &RuntimeConfig,
    selector: &str,
) -> Result<AccountRecordView, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    let Some(account) = find_by_selector(snapshot.accounts.as_slice(), selector) else {
        return Err(RuntimeError::Config(format!(
            "account selector `{selector}` did not match any local account"
        )));
    };

    manager.select_account(&account.record.account_id)?;
    let snapshot = snapshot_from_manager(&manager)?;
    snapshot
        .accounts
        .into_iter()
        .find(|candidate| candidate.record.account_id == account.record.account_id)
        .ok_or_else(|| {
            RuntimeError::Accounts(
                radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::InvalidState(
                    "selected account missing after account use".to_owned(),
                ),
            )
        })
}

pub fn selected_account_status(
    config: &RuntimeConfig,
) -> Result<RadrootsNostrSelectedAccountStatus, RuntimeError> {
    let manager = account_manager(config)?;
    if let Some(selector) = config.account.selector.as_deref() {
        let snapshot = snapshot_from_manager(&manager)?;
        let Some(account) = find_by_selector(snapshot.accounts.as_slice(), selector) else {
            return Err(RuntimeError::Config(format!(
                "account selector `{selector}` did not match any local account"
            )));
        };

        return Ok(
            match secret_file_path(&config.account.secrets_dir, &account.record.account_id).exists()
            {
                true => RadrootsNostrSelectedAccountStatus::Ready {
                    account: account.record.clone(),
                },
                false => RadrootsNostrSelectedAccountStatus::PublicOnly {
                    account: account.record.clone(),
                },
            },
        );
    }

    Ok(manager.selected_account_status()?)
}

fn snapshot_from_manager(
    manager: &RadrootsNostrAccountsManager,
) -> Result<AccountSnapshot, RuntimeError> {
    let selected_account_id = manager.selected_account_id()?.map(|id| id.to_string());
    let accounts = manager
        .list_accounts()?
        .into_iter()
        .map(|record| AccountRecordView {
            selected: selected_account_id
                .as_deref()
                .is_some_and(|selected| selected == record.account_id.as_str()),
            signer: "local",
            record,
        })
        .collect();

    Ok(AccountSnapshot { accounts })
}

fn find_by_selector<'a>(
    accounts: &'a [AccountRecordView],
    selector: &str,
) -> Option<&'a AccountRecordView> {
    let normalized = selector.trim();
    if normalized.is_empty() {
        return None;
    }

    accounts.iter().find(|account| {
        account.record.account_id.as_str() == normalized
            || account.record.public_identity.public_key_npub == normalized
            || account.record.label.as_deref() == Some(normalized)
    })
}

fn account_manager(config: &RuntimeConfig) -> Result<RadrootsNostrAccountsManager, RuntimeError> {
    let store = Arc::new(RadrootsNostrFileAccountStore::new(
        config.account.store_path.as_path(),
    ));
    let vault = Arc::new(CliFileSecretVault::new(
        config.account.secrets_dir.as_path(),
    ));
    Ok(RadrootsNostrAccountsManager::new(store, vault)?)
}

#[derive(Debug, Clone)]
struct CliFileSecretVault {
    secrets_dir: PathBuf,
}

impl CliFileSecretVault {
    fn new(path: impl AsRef<Path>) -> Self {
        Self {
            secrets_dir: path.as_ref().to_path_buf(),
        }
    }
}

impl RadrootsNostrSecretVault for CliFileSecretVault {
    fn store_secret_hex(
        &self,
        account_id: &RadrootsIdentityId,
        secret_key_hex: &str,
    ) -> Result<(), radroots_nostr_accounts::prelude::RadrootsNostrAccountsError> {
        fs::create_dir_all(&self.secrets_dir).map_err(|source| {
            radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::Vault(source.to_string())
        })?;
        let path = secret_file_path(&self.secrets_dir, account_id);
        fs::write(&path, secret_key_hex.as_bytes()).map_err(|source| {
            radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::Vault(source.to_string())
        })?;
        set_secret_permissions(&path)?;
        Ok(())
    }

    fn load_secret_hex(
        &self,
        account_id: &RadrootsIdentityId,
    ) -> Result<Option<String>, radroots_nostr_accounts::prelude::RadrootsNostrAccountsError> {
        let path = secret_file_path(&self.secrets_dir, account_id);
        match fs::read_to_string(path) {
            Ok(contents) => Ok(Some(contents.trim().to_owned())),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(
                radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::Vault(
                    source.to_string(),
                ),
            ),
        }
    }

    fn remove_secret(
        &self,
        account_id: &RadrootsIdentityId,
    ) -> Result<(), radroots_nostr_accounts::prelude::RadrootsNostrAccountsError> {
        let path = secret_file_path(&self.secrets_dir, account_id);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(
                radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::Vault(
                    source.to_string(),
                ),
            ),
        }
    }
}

fn secret_file_path(secrets_dir: &Path, account_id: &RadrootsIdentityId) -> PathBuf {
    secrets_dir.join(format!("{}.secret", account_id))
}

#[cfg(unix)]
fn set_secret_permissions(
    path: &Path,
) -> Result<(), radroots_nostr_accounts::prelude::RadrootsNostrAccountsError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .map_err(|source| {
            radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::Vault(source.to_string())
        })?
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions).map_err(|source| {
        radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::Vault(source.to_string())
    })
}

#[cfg(not(unix))]
fn set_secret_permissions(
    _path: &Path,
) -> Result<(), radroots_nostr_accounts::prelude::RadrootsNostrAccountsError> {
    Ok(())
}
