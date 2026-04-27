use std::path::Path;

use radroots_identity::{
    IdentityError, RadrootsIdentity, RadrootsIdentityPublic, load_identity_profile,
};
use radroots_nostr_accounts::prelude::{
    RadrootsNostrAccountRecord, RadrootsNostrAccountStatus, RadrootsNostrAccountsError,
    RadrootsNostrAccountsManager,
};
use radroots_secret_vault::{
    RadrootsHostVaultCapabilities, RadrootsResolvedSecretBackend,
    RadrootsSecretBackendAvailability, RadrootsSecretBackendSelection, RadrootsSecretVault,
    RadrootsSecretVaultError, RadrootsSecretVaultOsKeyring,
};

use crate::domain::runtime::{AccountResolutionView, AccountSummaryView};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

const HOST_VAULT_AVAILABILITY_OVERRIDE_ENV: &str = "RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE";
const HOST_VAULT_SERVICE_NAME: &str = "org.radroots.cli.local-account";
const HOST_VAULT_PROBE_SLOT: &str = "__radroots_cli_host_vault_probe__";
pub const SHARED_ACCOUNT_STORE_SOURCE: &str = "shared account store · local first";

#[derive(Debug, Clone)]
pub struct AccountSnapshot {
    pub accounts: Vec<AccountRecordView>,
}

#[derive(Debug, Clone)]
pub struct AccountRecordView {
    pub record: RadrootsNostrAccountRecord,
    pub is_default: bool,
    pub signer: &'static str,
}

#[derive(Debug, Clone)]
pub struct AccountSecretBackendStatus {
    pub configured_primary: String,
    pub configured_fallback: Option<String>,
    pub state: String,
    pub active_backend: Option<String>,
    pub used_fallback: bool,
    pub reason: Option<String>,
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

#[derive(Debug, Clone)]
pub struct AccountClearDefaultResult {
    pub cleared_account: Option<AccountRecordView>,
    pub remaining_account_count: usize,
}

#[derive(Debug, Clone)]
pub struct AccountRemoveResult {
    pub removed_account: AccountRecordView,
    pub default_cleared: bool,
    pub remaining_account_count: usize,
}

#[derive(Debug, Clone)]
pub struct AccountRemovePreview {
    pub account: AccountRecordView,
    pub default_would_clear: bool,
    pub remaining_account_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountResolutionSource {
    InvocationOverride,
    DefaultAccount,
    None,
}

impl AccountResolutionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvocationOverride => "invocation_override",
            Self::DefaultAccount => "default_account",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccountResolution {
    pub source: AccountResolutionSource,
    pub resolved_account: Option<AccountRecordView>,
    pub default_account: Option<AccountRecordView>,
}

#[derive(Debug, Clone)]
pub struct AccountSigningIdentity {
    pub account: AccountRecordView,
    pub identity: RadrootsIdentity,
}

pub fn create_or_migrate_default_account(
    config: &RuntimeConfig,
) -> Result<AccountCreateResult, RuntimeError> {
    let manager = account_manager(config)?;
    let existing = manager.list_accounts()?;
    let (mode, created_account_id) = if existing.is_empty() && config.identity.path.exists() {
        (
            AccountCreateMode::Migrated,
            manager.migrate_legacy_identity_file(&config.identity.path, None, false)?,
        )
    } else {
        (
            AccountCreateMode::Created,
            manager.generate_identity(None, false)?,
        )
    };

    let snapshot = snapshot(config)?;
    let account = snapshot_account(
        &snapshot,
        &created_account_id,
        "created account missing after account create",
    )?;

    Ok(AccountCreateResult { mode, account })
}

pub fn import_public_identity(
    config: &RuntimeConfig,
    path: &Path,
    make_default: bool,
) -> Result<AccountRecordView, RuntimeError> {
    let manager = account_manager(config)?;
    let public_identity = load_public_identity_for_import(path)?;
    let imported_account_id =
        manager.upsert_public_identity(public_identity, None, make_default)?;
    let snapshot = snapshot_from_manager(&manager)?;
    snapshot_account(
        &snapshot,
        &imported_account_id,
        "imported account missing after account import",
    )
}

pub fn preview_public_identity_import(
    config: &RuntimeConfig,
    path: &Path,
    make_default: bool,
) -> Result<AccountRecordView, RuntimeError> {
    let public_identity = load_public_identity_for_import(path)?;
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    if let Some(existing) = snapshot
        .accounts
        .iter()
        .find(|account| account.record.account_id == public_identity.id)
        .cloned()
    {
        let mut account = existing;
        if make_default {
            account.is_default = true;
        }
        return Ok(account);
    }

    Ok(AccountRecordView {
        record: RadrootsNostrAccountRecord::new(public_identity, None, 0),
        is_default: make_default,
        signer: "watch_only",
    })
}

pub fn snapshot(config: &RuntimeConfig) -> Result<AccountSnapshot, RuntimeError> {
    let manager = account_manager(config)?;
    snapshot_from_manager(&manager)
}

pub fn resolve_account(config: &RuntimeConfig) -> Result<Option<AccountRecordView>, RuntimeError> {
    Ok(resolve_account_resolution(config)?.resolved_account)
}

pub fn resolve_account_resolution(
    config: &RuntimeConfig,
) -> Result<AccountResolution, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    let default_account = snapshot
        .accounts
        .iter()
        .find(|account| account.is_default)
        .cloned();
    if let Some(selector) = config.account.selector.as_deref() {
        let account = resolve_selector_account(&manager, &snapshot, selector)?;
        return Ok(AccountResolution {
            source: AccountResolutionSource::InvocationOverride,
            resolved_account: Some(account),
            default_account,
        });
    }

    Ok(AccountResolution {
        source: if default_account.is_some() {
            AccountResolutionSource::DefaultAccount
        } else {
            AccountResolutionSource::None
        },
        resolved_account: default_account.clone(),
        default_account,
    })
}

pub fn select_account(
    config: &RuntimeConfig,
    selector: &str,
) -> Result<AccountRecordView, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    let account = resolve_selector_account(&manager, &snapshot, selector)?;

    manager.set_default_account(&account.record.account_id)?;
    let snapshot = snapshot_from_manager(&manager)?;
    snapshot
        .accounts
        .into_iter()
        .find(|candidate| candidate.record.account_id == account.record.account_id)
        .ok_or_else(|| {
            RuntimeError::Accounts(
                radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::InvalidState(
                    "default account missing after account use".to_owned(),
                ),
            )
        })
}

pub fn resolve_account_selector(
    config: &RuntimeConfig,
    selector: &str,
) -> Result<AccountRecordView, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    resolve_selector_account(&manager, &snapshot, selector)
}

pub fn clear_default_account(
    config: &RuntimeConfig,
) -> Result<AccountClearDefaultResult, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    let cleared_account = snapshot
        .accounts
        .iter()
        .find(|account| account.is_default)
        .cloned();
    manager.clear_default_account()?;
    let remaining_account_count = snapshot_from_manager(&manager)?.accounts.len();
    Ok(AccountClearDefaultResult {
        cleared_account,
        remaining_account_count,
    })
}

pub fn remove_account(
    config: &RuntimeConfig,
    selector: &str,
) -> Result<AccountRemoveResult, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    let removed_account = resolve_selector_account(&manager, &snapshot, selector)?;
    let default_cleared = removed_account.is_default;
    manager.remove_account(&removed_account.record.account_id)?;
    let remaining_account_count = snapshot_from_manager(&manager)?.accounts.len();
    Ok(AccountRemoveResult {
        removed_account,
        default_cleared,
        remaining_account_count,
    })
}

pub fn preview_account_removal(
    config: &RuntimeConfig,
    selector: &str,
) -> Result<AccountRemovePreview, RuntimeError> {
    let manager = account_manager(config)?;
    let snapshot = snapshot_from_manager(&manager)?;
    let account = resolve_selector_account(&manager, &snapshot, selector)?;
    Ok(AccountRemovePreview {
        default_would_clear: account.is_default,
        remaining_account_count: snapshot.accounts.len().saturating_sub(1),
        account,
    })
}

pub fn resolved_account_signing_status(
    config: &RuntimeConfig,
) -> Result<RadrootsNostrAccountStatus, RuntimeError> {
    let manager = account_manager(config)?;
    let resolution = resolve_account_resolution(config)?;
    let Some(account) = resolution.resolved_account else {
        return Ok(RadrootsNostrAccountStatus::NotConfigured);
    };

    Ok(
        match manager.get_signing_identity(&account.record.account_id)? {
            Some(_) => RadrootsNostrAccountStatus::Ready {
                account: account.record.clone(),
            },
            None => RadrootsNostrAccountStatus::PublicOnly {
                account: account.record.clone(),
            },
        },
    )
}

pub fn resolve_local_signing_identity(
    config: &RuntimeConfig,
) -> Result<AccountSigningIdentity, RuntimeError> {
    let manager = account_manager(config)?;
    let resolution = resolve_account_resolution(config)?;
    let Some(account) = resolution.resolved_account else {
        return Err(RuntimeError::Config(
            "no local account is selected for signing".to_owned(),
        ));
    };
    let Some(identity) = manager.get_signing_identity(&account.record.account_id)? else {
        return Err(RuntimeError::Config(format!(
            "watch_only account {} is present but not secret-backed",
            account.record.account_id
        )));
    };
    Ok(AccountSigningIdentity { account, identity })
}

pub fn account_summary_view(account: &AccountRecordView) -> AccountSummaryView {
    AccountSummaryView::from_account_record(&account.record, account.signer, account.is_default)
}

pub fn account_resolution_view(resolution: &AccountResolution) -> AccountResolutionView {
    AccountResolutionView {
        source: resolution.source.as_str().to_owned(),
        resolved_account: resolution
            .resolved_account
            .as_ref()
            .map(account_summary_view),
        default_account: resolution
            .default_account
            .as_ref()
            .map(account_summary_view),
    }
}

pub fn empty_account_resolution_view() -> AccountResolutionView {
    AccountResolutionView {
        source: AccountResolutionSource::None.as_str().to_owned(),
        resolved_account: None,
        default_account: None,
    }
}

pub fn unresolved_account_reason(config: &RuntimeConfig) -> Result<String, RuntimeError> {
    let snapshot = snapshot(config)?;
    Ok(if snapshot.accounts.is_empty() {
        format!(
            "no local accounts found in {}",
            config.account.store_path.display()
        )
    } else {
        format!(
            "accounts exist in {} but no default account is configured and no invocation override was provided",
            config.account.store_path.display()
        )
    })
}

pub fn secret_backend_status(config: &RuntimeConfig) -> AccountSecretBackendStatus {
    let configured_primary = config.account.secret_backend.kind().to_string();
    let configured_fallback = config
        .account
        .secret_fallback
        .map(|backend| backend.kind().to_string());

    match resolve_secret_backend(config) {
        Ok(resolved) => AccountSecretBackendStatus {
            configured_primary,
            configured_fallback,
            state: "ready".to_owned(),
            active_backend: Some(resolved.backend.kind().to_string()),
            used_fallback: resolved.used_fallback,
            reason: None,
        },
        Err(SecretBackendResolutionError::Unavailable(reason)) => AccountSecretBackendStatus {
            configured_primary,
            configured_fallback,
            state: "unavailable".to_owned(),
            active_backend: None,
            used_fallback: false,
            reason: Some(reason),
        },
        Err(SecretBackendResolutionError::Invalid(reason)) => AccountSecretBackendStatus {
            configured_primary,
            configured_fallback,
            state: "error".to_owned(),
            active_backend: None,
            used_fallback: false,
            reason: Some(reason),
        },
    }
}

fn snapshot_from_manager(
    manager: &RadrootsNostrAccountsManager,
) -> Result<AccountSnapshot, RuntimeError> {
    let default_account_id = manager.default_account_id()?.map(|id| id.to_string());
    let mut accounts = Vec::new();
    for record in manager.list_accounts()? {
        let is_default = default_account_id
            .as_deref()
            .is_some_and(|default| default == record.account_id.as_str());
        let signer = account_signer(manager, &record)?;
        accounts.push(AccountRecordView {
            record,
            is_default,
            signer,
        });
    }

    Ok(AccountSnapshot { accounts })
}

fn snapshot_account(
    snapshot: &AccountSnapshot,
    account_id: &radroots_identity::RadrootsIdentityId,
    missing_message: &str,
) -> Result<AccountRecordView, RuntimeError> {
    snapshot
        .accounts
        .iter()
        .find(|account| account.record.account_id == *account_id)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::Accounts(
                radroots_nostr_accounts::prelude::RadrootsNostrAccountsError::InvalidState(
                    missing_message.to_owned(),
                ),
            )
        })
}

fn resolve_selector_account(
    manager: &RadrootsNostrAccountsManager,
    snapshot: &AccountSnapshot,
    selector: &str,
) -> Result<AccountRecordView, RuntimeError> {
    let record = manager
        .resolve_account_selector(selector)
        .map_err(|error| selector_runtime_error(selector, error))?;
    snapshot
        .accounts
        .iter()
        .find(|account| account.record.account_id == record.account_id)
        .cloned()
        .ok_or_else(|| {
            RuntimeError::Accounts(RadrootsNostrAccountsError::InvalidState(
                "resolved account missing from snapshot".to_owned(),
            ))
        })
}

fn selector_runtime_error(selector: &str, error: RadrootsNostrAccountsError) -> RuntimeError {
    let normalized = selector.trim();
    match error {
        RadrootsNostrAccountsError::InvalidAccountSelector(reason) => RuntimeError::Config(reason),
        RadrootsNostrAccountsError::AccountNotFound(_) => RuntimeError::Config(format!(
            "account selector `{normalized}` did not match any local account"
        )),
        RadrootsNostrAccountsError::AmbiguousAccountSelector(_) => RuntimeError::Config(format!(
            "account selector `{normalized}` matched multiple local accounts; use account id or npub"
        )),
        other => RuntimeError::Accounts(other),
    }
}

fn account_signer(
    manager: &RadrootsNostrAccountsManager,
    record: &RadrootsNostrAccountRecord,
) -> Result<&'static str, RuntimeError> {
    Ok(
        if manager.get_signing_identity(&record.account_id)?.is_some() {
            "local"
        } else {
            "watch_only"
        },
    )
}

fn format_identity_error(error: IdentityError) -> String {
    match error {
        IdentityError::NotFound(path) => format!("path not found: {}", path.display()),
        other => other.to_string(),
    }
}

fn load_public_identity_for_import(path: &Path) -> Result<RadrootsIdentityPublic, RuntimeError> {
    load_identity_profile(path).map_err(|error| {
        RuntimeError::Config(format!(
            "failed to import account from {}: {}",
            path.display(),
            format_identity_error(error)
        ))
    })
}

fn account_manager(config: &RuntimeConfig) -> Result<RadrootsNostrAccountsManager, RuntimeError> {
    let (manager, _) = RadrootsNostrAccountsManager::new_local_file_backed(
        config.account.store_path.as_path(),
        config.account.secrets_dir.as_path(),
        account_secret_backend_selection(config),
        secret_backend_availability()?,
        HOST_VAULT_SERVICE_NAME,
    )?;
    Ok(manager)
}

fn resolve_secret_backend(
    config: &RuntimeConfig,
) -> Result<RadrootsResolvedSecretBackend, SecretBackendResolutionError> {
    let availability = secret_backend_availability().map_err(|error| {
        SecretBackendResolutionError::Invalid(format!("account secret backend: {error}"))
    })?;
    RadrootsNostrAccountsManager::resolve_local_backend(
        account_secret_backend_selection(config),
        availability,
    )
    .map_err(|error| match error {
        RadrootsSecretVaultError::BackendUnavailable { .. }
        | RadrootsSecretVaultError::FallbackUnavailable { .. } => {
            SecretBackendResolutionError::Unavailable(format!("account secret backend: {error}"))
        }
        RadrootsSecretVaultError::FallbackDisallowed { .. }
        | RadrootsSecretVaultError::HostVaultPolicyUnsupported { .. } => {
            SecretBackendResolutionError::Invalid(format!("account secret backend: {error}"))
        }
    })
}

fn account_secret_backend_selection(config: &RuntimeConfig) -> RadrootsSecretBackendSelection {
    RadrootsSecretBackendSelection {
        primary: config.account.secret_backend,
        fallback: config.account.secret_fallback,
    }
}

fn secret_backend_availability() -> Result<RadrootsSecretBackendAvailability, RuntimeError> {
    Ok(RadrootsSecretBackendAvailability {
        host_vault: host_vault_capabilities()?,
        encrypted_file: true,
        external_command: false,
        memory: true,
    })
}

fn host_vault_capabilities() -> Result<RadrootsHostVaultCapabilities, RuntimeError> {
    if let Some(available) = host_vault_availability_override()? {
        return Ok(match available {
            true => RadrootsHostVaultCapabilities::desktop_keyring(),
            false => RadrootsHostVaultCapabilities::unavailable(),
        });
    }

    let keyring = RadrootsSecretVaultOsKeyring::new(HOST_VAULT_SERVICE_NAME);
    match keyring.load_secret(HOST_VAULT_PROBE_SLOT) {
        Ok(_) => Ok(RadrootsHostVaultCapabilities::desktop_keyring()),
        Err(_) => Ok(RadrootsHostVaultCapabilities::unavailable()),
    }
}

fn host_vault_availability_override() -> Result<Option<bool>, RuntimeError> {
    let Ok(value) = std::env::var(HOST_VAULT_AVAILABILITY_OVERRIDE_ENV) else {
        return Ok(None);
    };

    parse_bool_value(HOST_VAULT_AVAILABILITY_OVERRIDE_ENV, value.trim()).map(Some)
}

fn parse_bool_value(key: &str, value: &str) -> Result<bool, RuntimeError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        other => Err(RuntimeError::Config(format!(
            "{key} must be a boolean value, got `{other}`"
        ))),
    }
}

#[derive(Debug, Clone)]
enum SecretBackendResolutionError {
    Unavailable(String),
    Invalid(String),
}

#[cfg(test)]
mod tests {
    use radroots_protected_store::RadrootsProtectedFileSecretVault;
    use radroots_secret_vault::RadrootsSecretVault;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn protected_file_vault_round_trips_secret() {
        let temp = tempdir().expect("tempdir");
        let vault = RadrootsProtectedFileSecretVault::new(temp.path());

        vault.store_secret("acct_demo", "deadbeef").expect("store");
        let loaded = vault.load_secret("acct_demo").expect("load");
        assert_eq!(loaded.as_deref(), Some("deadbeef"));
        let raw = fs::read_to_string(temp.path().join("acct_demo.secret.json")).expect("raw file");
        assert!(!raw.contains("deadbeef"));
    }

    #[test]
    fn protected_file_vault_removes_secret() {
        let temp = tempdir().expect("tempdir");
        let vault = RadrootsProtectedFileSecretVault::new(temp.path());

        vault.store_secret("acct_demo", "deadbeef").expect("store");
        vault.remove_secret("acct_demo").expect("remove");
        assert!(vault.load_secret("acct_demo").expect("load").is_none());
    }
}
