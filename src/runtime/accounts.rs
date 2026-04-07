use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use getrandom::getrandom;
use radroots_nostr_accounts::prelude::{
    RadrootsNostrAccountRecord, RadrootsNostrAccountsManager, RadrootsNostrFileAccountStore,
    RadrootsNostrSecretVaultMemory, RadrootsNostrSelectedAccountStatus,
};
use radroots_protected_store::{
    RADROOTS_PROTECTED_STORE_KEY_LENGTH, RADROOTS_PROTECTED_STORE_NONCE_LENGTH,
    RadrootsProtectedStoreEnvelope,
};
use radroots_secret_vault::{
    RadrootsHostVaultCapabilities, RadrootsResolvedSecretBackend, RadrootsSecretBackend,
    RadrootsSecretBackendSelection, RadrootsSecretKeyWrapping, RadrootsSecretVault,
    RadrootsSecretVaultAccessError, RadrootsSecretVaultError, RadrootsSecretVaultOsKeyring,
};
use zeroize::Zeroize;

use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;

const HOST_VAULT_AVAILABILITY_OVERRIDE_ENV: &str = "RADROOTS_ACCOUNT_HOST_VAULT_AVAILABLE";
const HOST_VAULT_SERVICE_NAME: &str = "org.radroots.cli.local-account";
const HOST_VAULT_PROBE_SLOT: &str = "__radroots_cli_host_vault_probe__";
const ENCRYPTED_FILE_MASTER_KEY_FILE: &str = ".vault.key";
const ENCRYPTED_FILE_SECRET_SUFFIX: &str = ".secret.json";
const PLAINTEXT_FILE_SECRET_SUFFIX: &str = ".secret";
const WRAPPED_KEY_VERSION: u8 = 1;

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
            match manager.get_signing_identity(&account.record.account_id)? {
                Some(_) => RadrootsNostrSelectedAccountStatus::Ready {
                    account: account.record.clone(),
                },
                None => RadrootsNostrSelectedAccountStatus::PublicOnly {
                    account: account.record.clone(),
                },
            },
        );
    }

    Ok(manager.selected_account_status()?)
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
    let vault = secret_vault(config)?;
    Ok(RadrootsNostrAccountsManager::new(store, vault)?)
}

fn secret_vault(config: &RuntimeConfig) -> Result<Arc<dyn RadrootsSecretVault>, RuntimeError> {
    let resolved = resolve_secret_backend(config).map_err(secret_backend_runtime_error)?;
    match resolved.backend {
        RadrootsSecretBackend::HostVault(_) => Ok(Arc::new(RadrootsSecretVaultOsKeyring::new(
            HOST_VAULT_SERVICE_NAME,
        ))),
        RadrootsSecretBackend::EncryptedFile => Ok(Arc::new(CliEncryptedFileSecretVault::new(
            config.account.secrets_dir.as_path(),
        ))),
        RadrootsSecretBackend::Memory => Ok(Arc::new(RadrootsNostrSecretVaultMemory::new())),
        RadrootsSecretBackend::PlaintextFile => Ok(Arc::new(CliPlaintextFileSecretVault::new(
            config.account.secrets_dir.as_path(),
        ))),
        RadrootsSecretBackend::ExternalCommand => Err(RuntimeError::Config(
            "external_command secret backend is not supported for local cli accounts".to_owned(),
        )),
    }
}

fn resolve_secret_backend(
    config: &RuntimeConfig,
) -> Result<RadrootsResolvedSecretBackend, SecretBackendResolutionError> {
    let availability = secret_backend_availability().map_err(|error| {
        SecretBackendResolutionError::Invalid(format!("account secret backend: {error}"))
    })?;
    let selection = RadrootsSecretBackendSelection {
        primary: config.account.secret_backend,
        fallback: config.account.secret_fallback,
    };

    selection
        .resolve(availability)
        .map_err(|error| match error {
            RadrootsSecretVaultError::BackendUnavailable { .. }
            | RadrootsSecretVaultError::FallbackUnavailable { .. } => {
                SecretBackendResolutionError::Unavailable(format!(
                    "account secret backend: {error}"
                ))
            }
            RadrootsSecretVaultError::FallbackDisallowed { .. }
            | RadrootsSecretVaultError::HostVaultPolicyUnsupported { .. } => {
                SecretBackendResolutionError::Invalid(format!("account secret backend: {error}"))
            }
        })
}

fn secret_backend_availability()
-> Result<radroots_secret_vault::RadrootsSecretBackendAvailability, RuntimeError> {
    Ok(radroots_secret_vault::RadrootsSecretBackendAvailability {
        host_vault: host_vault_capabilities()?,
        encrypted_file: true,
        external_command: false,
        memory: true,
        plaintext_file: true,
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

fn secret_backend_runtime_error(error: SecretBackendResolutionError) -> RuntimeError {
    match error {
        SecretBackendResolutionError::Unavailable(message)
        | SecretBackendResolutionError::Invalid(message) => RuntimeError::Config(message),
    }
}

#[derive(Debug, Clone)]
enum SecretBackendResolutionError {
    Unavailable(String),
    Invalid(String),
}

#[derive(Debug, Clone)]
struct CliEncryptedFileSecretVault {
    secrets_dir: PathBuf,
}

impl CliEncryptedFileSecretVault {
    fn new(path: impl AsRef<Path>) -> Self {
        Self {
            secrets_dir: path.as_ref().to_path_buf(),
        }
    }

    fn secret_file_path(&self, slot: &str) -> PathBuf {
        self.secrets_dir
            .join(format!("{slot}{ENCRYPTED_FILE_SECRET_SUFFIX}"))
    }

    fn wrapping_key_path(&self) -> PathBuf {
        self.secrets_dir.join(ENCRYPTED_FILE_MASTER_KEY_FILE)
    }

    fn load_or_create_wrapping_key(
        &self,
    ) -> Result<[u8; RADROOTS_PROTECTED_STORE_KEY_LENGTH], RadrootsSecretVaultAccessError> {
        if self.wrapping_key_path().exists() {
            return self.load_wrapping_key();
        }

        fs::create_dir_all(&self.secrets_dir).map_err(io_backend_error)?;
        let mut key = [0_u8; RADROOTS_PROTECTED_STORE_KEY_LENGTH];
        getrandom(&mut key)
            .map_err(|_| RadrootsSecretVaultAccessError::Backend("entropy unavailable".into()))?;
        fs::write(self.wrapping_key_path(), key.as_slice()).map_err(io_backend_error)?;
        set_secret_permissions(self.wrapping_key_path().as_path())?;
        Ok(key)
    }

    fn load_wrapping_key(
        &self,
    ) -> Result<[u8; RADROOTS_PROTECTED_STORE_KEY_LENGTH], RadrootsSecretVaultAccessError> {
        let raw = fs::read(self.wrapping_key_path()).map_err(io_backend_error)?;
        if raw.len() != RADROOTS_PROTECTED_STORE_KEY_LENGTH {
            return Err(RadrootsSecretVaultAccessError::Backend(format!(
                "encrypted file wrapping key {} has invalid length {}",
                self.wrapping_key_path().display(),
                raw.len()
            )));
        }

        let mut key = [0_u8; RADROOTS_PROTECTED_STORE_KEY_LENGTH];
        key.copy_from_slice(&raw);
        Ok(key)
    }
}

impl RadrootsSecretVault for CliEncryptedFileSecretVault {
    fn store_secret(&self, slot: &str, secret: &str) -> Result<(), RadrootsSecretVaultAccessError> {
        fs::create_dir_all(&self.secrets_dir).map_err(io_backend_error)?;
        let envelope =
            RadrootsProtectedStoreEnvelope::seal_with_wrapped_key(self, slot, secret.as_bytes())
                .map_err(|error| RadrootsSecretVaultAccessError::Backend(error.to_string()))?;
        let encoded = envelope
            .encode_json()
            .map_err(|error| RadrootsSecretVaultAccessError::Backend(error.to_string()))?;
        let path = self.secret_file_path(slot);
        fs::write(&path, encoded).map_err(io_backend_error)?;
        set_secret_permissions(&path)?;
        Ok(())
    }

    fn load_secret(&self, slot: &str) -> Result<Option<String>, RadrootsSecretVaultAccessError> {
        let path = self.secret_file_path(slot);
        let encoded = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io_backend_error(source)),
        };
        let envelope = RadrootsProtectedStoreEnvelope::decode_json(encoded.as_slice())
            .map_err(|error| RadrootsSecretVaultAccessError::Backend(error.to_string()))?;
        let plaintext = envelope
            .open_with_wrapped_key(self)
            .map_err(|error| RadrootsSecretVaultAccessError::Backend(error.to_string()))?;
        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|error| RadrootsSecretVaultAccessError::Backend(error.to_string()))
    }

    fn remove_secret(&self, slot: &str) -> Result<(), RadrootsSecretVaultAccessError> {
        match fs::remove_file(self.secret_file_path(slot)) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(io_backend_error(source)),
        }
    }
}

impl RadrootsSecretKeyWrapping for CliEncryptedFileSecretVault {
    type Error = RadrootsSecretVaultAccessError;

    fn wrap_data_key(&self, key_slot: &str, plaintext_key: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let mut master_key = self.load_or_create_wrapping_key()?;
        let mut nonce = [0_u8; RADROOTS_PROTECTED_STORE_NONCE_LENGTH];
        getrandom(&mut nonce)
            .map_err(|_| RadrootsSecretVaultAccessError::Backend("entropy unavailable".into()))?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&master_key));
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext_key,
                    aad: key_slot.as_bytes(),
                },
            )
            .map_err(|_| {
                RadrootsSecretVaultAccessError::Backend("failed to wrap data key".into())
            })?;
        master_key.zeroize();

        let mut encoded = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
        encoded.push(WRAPPED_KEY_VERSION);
        encoded.extend_from_slice(&nonce);
        encoded.extend_from_slice(ciphertext.as_slice());
        Ok(encoded)
    }

    fn unwrap_data_key(&self, key_slot: &str, wrapped_key: &[u8]) -> Result<Vec<u8>, Self::Error> {
        if wrapped_key.len() <= 1 + RADROOTS_PROTECTED_STORE_NONCE_LENGTH {
            return Err(RadrootsSecretVaultAccessError::Backend(
                "wrapped data key is truncated".into(),
            ));
        }
        if wrapped_key[0] != WRAPPED_KEY_VERSION {
            return Err(RadrootsSecretVaultAccessError::Backend(format!(
                "unsupported wrapped data key version {}",
                wrapped_key[0]
            )));
        }

        let mut master_key = self.load_wrapping_key()?;
        let nonce_offset = 1;
        let ciphertext_offset = nonce_offset + RADROOTS_PROTECTED_STORE_NONCE_LENGTH;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&master_key));
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(&wrapped_key[nonce_offset..ciphertext_offset]),
                Payload {
                    msg: &wrapped_key[ciphertext_offset..],
                    aad: key_slot.as_bytes(),
                },
            )
            .map_err(|_| {
                RadrootsSecretVaultAccessError::Backend("failed to unwrap data key".into())
            })?;
        master_key.zeroize();
        Ok(plaintext)
    }
}

#[derive(Debug, Clone)]
struct CliPlaintextFileSecretVault {
    secrets_dir: PathBuf,
}

impl CliPlaintextFileSecretVault {
    fn new(path: impl AsRef<Path>) -> Self {
        Self {
            secrets_dir: path.as_ref().to_path_buf(),
        }
    }

    fn secret_file_path(&self, slot: &str) -> PathBuf {
        self.secrets_dir
            .join(format!("{slot}{PLAINTEXT_FILE_SECRET_SUFFIX}"))
    }
}

impl RadrootsSecretVault for CliPlaintextFileSecretVault {
    fn store_secret(&self, slot: &str, secret: &str) -> Result<(), RadrootsSecretVaultAccessError> {
        fs::create_dir_all(&self.secrets_dir).map_err(io_backend_error)?;
        let path = self.secret_file_path(slot);
        fs::write(&path, secret.as_bytes()).map_err(io_backend_error)?;
        set_secret_permissions(&path)?;
        Ok(())
    }

    fn load_secret(&self, slot: &str) -> Result<Option<String>, RadrootsSecretVaultAccessError> {
        match fs::read_to_string(self.secret_file_path(slot)) {
            Ok(contents) => Ok(Some(contents.trim().to_owned())),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(io_backend_error(source)),
        }
    }

    fn remove_secret(&self, slot: &str) -> Result<(), RadrootsSecretVaultAccessError> {
        match fs::remove_file(self.secret_file_path(slot)) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(io_backend_error(source)),
        }
    }
}

fn io_backend_error(source: std::io::Error) -> RadrootsSecretVaultAccessError {
    RadrootsSecretVaultAccessError::Backend(source.to_string())
}

#[cfg(unix)]
fn set_secret_permissions(path: &Path) -> Result<(), RadrootsSecretVaultAccessError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).map_err(io_backend_error)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions).map_err(io_backend_error)
}

#[cfg(not(unix))]
fn set_secret_permissions(_path: &Path) -> Result<(), RadrootsSecretVaultAccessError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encrypted_file_vault_round_trips_secret() {
        let temp = tempdir().expect("tempdir");
        let vault = CliEncryptedFileSecretVault::new(temp.path());

        vault.store_secret("acct_demo", "deadbeef").expect("store");
        let loaded = vault.load_secret("acct_demo").expect("load");
        assert_eq!(loaded.as_deref(), Some("deadbeef"));
        let raw = fs::read_to_string(temp.path().join("acct_demo.secret.json")).expect("raw file");
        assert!(!raw.contains("deadbeef"));
    }

    #[test]
    fn encrypted_file_vault_removes_secret() {
        let temp = tempdir().expect("tempdir");
        let vault = CliEncryptedFileSecretVault::new(temp.path());

        vault.store_secret("acct_demo", "deadbeef").expect("store");
        vault.remove_secret("acct_demo").expect("remove");
        assert!(vault.load_secret("acct_demo").expect("load").is_none());
    }
}
