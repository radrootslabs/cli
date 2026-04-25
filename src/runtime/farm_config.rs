use std::fs;
use std::path::{Path, PathBuf};

use radroots_events::farm::RadrootsFarm;
use radroots_events::listing::{RadrootsListingDeliveryMethod, RadrootsListingLocation};
use radroots_events::profile::RadrootsProfile;
use radroots_events_codec::d_tag::is_d_tag_base64url;
use serde::{Deserialize, Serialize};

use crate::runtime::RuntimeError;
use crate::runtime::config::{PathsConfig, RuntimeConfig};

const FARM_CONFIG_FILE_NAME: &str = "farm.toml";
pub const SUPPORTED_FARM_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FarmConfigScope {
    User,
    Workspace,
}

impl FarmConfigScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Workspace => "workspace",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FarmConfigDocument {
    pub version: u32,
    pub selection: FarmConfigSelection,
    pub profile: RadrootsProfile,
    pub farm: RadrootsFarm,
    pub listing_defaults: FarmListingDefaults,
    #[serde(default)]
    pub publication: FarmPublicationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FarmConfigSelection {
    pub scope: FarmConfigScope,
    pub account: String,
    pub farm_d_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FarmListingDefaults {
    pub delivery_method: String,
    pub location: RadrootsListingLocation,
}

impl FarmListingDefaults {
    pub fn delivery_method_model(&self) -> Result<RadrootsListingDeliveryMethod, RuntimeError> {
        parse_delivery_method(self.delivery_method.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FarmPublicationStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farm_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_published_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub farm_published_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ResolvedFarmConfig {
    pub scope: FarmConfigScope,
    pub path: PathBuf,
    pub document: FarmConfigDocument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FarmMissingField {
    Name,
    Location,
    Delivery,
    Country,
}

impl FarmMissingField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Farm name",
            Self::Location => "Location",
            Self::Delivery => "Delivery method",
            Self::Country => "Country",
        }
    }
}

pub fn resolve_scope(
    paths: &PathsConfig,
    explicit_scope: Option<FarmConfigScope>,
) -> Result<FarmConfigScope, RuntimeError> {
    if let Some(scope) = explicit_scope {
        return Ok(scope);
    }
    match paths.profile.as_str() {
        "repo_local" => Ok(FarmConfigScope::Workspace),
        "interactive_user" => Ok(FarmConfigScope::User),
        other => Err(RuntimeError::Config(format!(
            "unsupported farm config path profile `{other}`"
        ))),
    }
}

pub fn user_config_path(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    let Some(parent) = paths.app_config_path.parent() else {
        return Err(RuntimeError::Config(format!(
            "app config path {} has no parent directory",
            paths.app_config_path.display()
        )));
    };
    Ok(parent.join(FARM_CONFIG_FILE_NAME))
}

pub fn workspace_config_path(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    let Some(parent) = paths.workspace_config_path.parent() else {
        return Err(RuntimeError::Config(format!(
            "workspace config path {} has no parent directory",
            paths.workspace_config_path.display()
        )));
    };
    Ok(parent.join("config/apps/cli").join(FARM_CONFIG_FILE_NAME))
}

pub fn config_path(paths: &PathsConfig, scope: FarmConfigScope) -> Result<PathBuf, RuntimeError> {
    match scope {
        FarmConfigScope::User => user_config_path(paths),
        FarmConfigScope::Workspace => workspace_config_path(paths),
    }
}

pub fn load(
    config: &RuntimeConfig,
    explicit_scope: Option<FarmConfigScope>,
) -> Result<Option<ResolvedFarmConfig>, RuntimeError> {
    load_from_paths(&config.paths, explicit_scope)
}

pub fn load_from_paths(
    paths: &PathsConfig,
    explicit_scope: Option<FarmConfigScope>,
) -> Result<Option<ResolvedFarmConfig>, RuntimeError> {
    let scope = resolve_scope(paths, explicit_scope)?;
    let path = config_path(paths, scope)?;
    load_from_path(path.as_path(), scope)
}

pub fn load_from_path(
    path: &Path,
    scope: FarmConfigScope,
) -> Result<Option<ResolvedFarmConfig>, RuntimeError> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)?;
    let document: FarmConfigDocument = toml::from_str(contents.as_str()).map_err(|error| {
        RuntimeError::Config(format!("parse farm config {}: {error}", path.display()))
    })?;
    validate(&document, scope)?;
    Ok(Some(ResolvedFarmConfig {
        scope,
        path: path.to_path_buf(),
        document,
    }))
}

pub fn write(
    paths: &PathsConfig,
    scope: FarmConfigScope,
    document: &FarmConfigDocument,
) -> Result<PathBuf, RuntimeError> {
    validate(document, scope)?;
    let path = config_path(paths, scope)?;
    let Some(parent) = path.parent() else {
        return Err(RuntimeError::Config(format!(
            "farm config path {} has no parent directory",
            path.display()
        )));
    };
    fs::create_dir_all(parent)?;
    let encoded = toml::to_string_pretty(document).map_err(|error| {
        RuntimeError::Config(format!("encode farm config {}: {error}", path.display()))
    })?;
    fs::write(&path, encoded)?;
    Ok(path)
}

pub fn validate(
    document: &FarmConfigDocument,
    resolved_scope: FarmConfigScope,
) -> Result<(), RuntimeError> {
    if document.version != SUPPORTED_FARM_CONFIG_VERSION {
        return Err(RuntimeError::Config(format!(
            "farm config version must be {}, got {}",
            SUPPORTED_FARM_CONFIG_VERSION, document.version
        )));
    }
    if document.selection.scope != resolved_scope {
        return Err(RuntimeError::Config(format!(
            "farm config scope `{}` does not match resolved `{}` scope",
            document.selection.scope.as_str(),
            resolved_scope.as_str()
        )));
    }
    if trimmed(document.selection.account.as_str()).is_empty() {
        return Err(RuntimeError::Config(
            "farm config selection.account must not be empty".to_owned(),
        ));
    }
    if trimmed(document.selection.farm_d_tag.as_str()).is_empty() {
        return Err(RuntimeError::Config(
            "farm config selection.farm_d_tag must not be empty".to_owned(),
        ));
    }
    if !is_d_tag_base64url(trimmed(document.selection.farm_d_tag.as_str())) {
        return Err(RuntimeError::Config(
            "farm config selection.farm_d_tag must be a 22-character base64url identifier"
                .to_owned(),
        ));
    }
    if trimmed(document.farm.d_tag.as_str()).is_empty() {
        return Err(RuntimeError::Config(
            "farm config farm.d_tag must not be empty".to_owned(),
        ));
    }
    if !is_d_tag_base64url(trimmed(document.farm.d_tag.as_str())) {
        return Err(RuntimeError::Config(
            "farm config farm.d_tag must be a 22-character base64url identifier".to_owned(),
        ));
    }
    if trimmed(document.selection.farm_d_tag.as_str()) != trimmed(document.farm.d_tag.as_str()) {
        return Err(RuntimeError::Config(
            "farm config selection.farm_d_tag must match farm.d_tag".to_owned(),
        ));
    }
    if !trimmed(document.listing_defaults.delivery_method.as_str()).is_empty() {
        let _ = document.listing_defaults.delivery_method_model()?;
    }
    Ok(())
}

pub fn missing_fields(document: &FarmConfigDocument) -> Vec<FarmMissingField> {
    let mut missing = Vec::new();

    if farm_name(document).is_none() {
        missing.push(FarmMissingField::Name);
    }

    let location_present = location_primary(document).is_some();
    if !location_present {
        missing.push(FarmMissingField::Location);
    }

    if trimmed(document.listing_defaults.delivery_method.as_str()).is_empty() {
        missing.push(FarmMissingField::Delivery);
    }

    if location_present && location_country(document).is_none() {
        missing.push(FarmMissingField::Country);
    }

    missing
}

fn farm_name(document: &FarmConfigDocument) -> Option<&str> {
    non_empty_ref(document.profile.name.as_str())
        .or_else(|| non_empty_ref(document.farm.name.as_str()))
}

fn location_primary(document: &FarmConfigDocument) -> Option<&str> {
    non_empty_ref(document.listing_defaults.location.primary.as_str()).or_else(|| {
        document
            .farm
            .location
            .as_ref()
            .and_then(|location| location.primary.as_deref())
            .and_then(non_empty_ref)
    })
}

fn location_country(document: &FarmConfigDocument) -> Option<&str> {
    document
        .listing_defaults
        .location
        .country
        .as_deref()
        .and_then(non_empty_ref)
        .or_else(|| {
            document
                .farm
                .location
                .as_ref()
                .and_then(|location| location.country.as_deref())
                .and_then(non_empty_ref)
        })
}

fn parse_delivery_method(value: &str) -> Result<RadrootsListingDeliveryMethod, RuntimeError> {
    let method = trimmed(value);
    if method.is_empty() {
        return Err(RuntimeError::Config(
            "farm config listing_defaults.delivery_method must not be empty".to_owned(),
        ));
    }
    Ok(match method {
        "pickup" => RadrootsListingDeliveryMethod::Pickup,
        "local_delivery" => RadrootsListingDeliveryMethod::LocalDelivery,
        "shipping" => RadrootsListingDeliveryMethod::Shipping,
        other => RadrootsListingDeliveryMethod::Other {
            method: other.to_owned(),
        },
    })
}

fn trimmed(value: &str) -> &str {
    value.trim()
}

fn non_empty_ref(value: &str) -> Option<&str> {
    let trimmed = trimmed(value);
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use radroots_events::farm::RadrootsFarmLocation;
    use tempfile::tempdir;

    fn sample_paths(profile: &str, root: &Path) -> PathsConfig {
        PathsConfig {
            profile: profile.to_owned(),
            profile_source: "test".to_owned(),
            allowed_profiles: vec!["interactive_user".to_owned(), "repo_local".to_owned()],
            root_source: "test".to_owned(),
            repo_local_root: Some(root.join("infra/local/runtime/radroots")),
            repo_local_root_source: Some("test".to_owned()),
            subordinate_path_override_source: "test".to_owned(),
            app_namespace: "apps/cli".to_owned(),
            shared_accounts_namespace: "shared/accounts".to_owned(),
            shared_identities_namespace: "shared/identities".to_owned(),
            app_config_path: root.join("home/.radroots/config/apps/cli/config.toml"),
            workspace_config_path: root.join("workspace/infra/local/runtime/radroots/config.toml"),
            app_data_root: root.join("home/.radroots/data/apps/cli"),
            app_logs_root: root.join("home/.radroots/logs/apps/cli"),
            shared_accounts_data_root: root.join("home/.radroots/data/shared/accounts"),
            shared_accounts_secrets_root: root.join("home/.radroots/secrets/shared/accounts"),
            default_identity_path: root
                .join("home/.radroots/secrets/shared/identities/default.json"),
        }
    }

    fn sample_document(scope: FarmConfigScope) -> FarmConfigDocument {
        FarmConfigDocument {
            version: SUPPORTED_FARM_CONFIG_VERSION,
            selection: FarmConfigSelection {
                scope,
                account: "seller".to_owned(),
                farm_d_tag: "AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            },
            profile: RadrootsProfile {
                name: "La Huerta".to_owned(),
                display_name: Some("La Huerta".to_owned()),
                nip05: None,
                about: Some("Small mixed vegetable farm.".to_owned()),
                website: Some("https://example.invalid/la-huerta".to_owned()),
                picture: None,
                banner: None,
                lud06: None,
                lud16: None,
                bot: None,
            },
            farm: RadrootsFarm {
                d_tag: "AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                name: "La Huerta".to_owned(),
                about: Some("Small mixed vegetable farm.".to_owned()),
                website: Some("https://example.invalid/la-huerta".to_owned()),
                picture: None,
                banner: None,
                location: Some(RadrootsFarmLocation {
                    primary: Some("San Francisco, CA".to_owned()),
                    city: Some("San Francisco".to_owned()),
                    region: Some("CA".to_owned()),
                    country: Some("US".to_owned()),
                    gcs: None,
                }),
                tags: None,
            },
            listing_defaults: FarmListingDefaults {
                delivery_method: "pickup".to_owned(),
                location: RadrootsListingLocation {
                    primary: "San Francisco, CA".to_owned(),
                    city: Some("San Francisco".to_owned()),
                    region: Some("CA".to_owned()),
                    country: Some("US".to_owned()),
                    lat: None,
                    lng: None,
                    geohash: None,
                },
            },
            publication: FarmPublicationStatus::default(),
        }
    }

    #[test]
    fn resolve_scope_defaults_from_runtime_profile() {
        let dir = tempdir().expect("tempdir");
        let interactive_paths = sample_paths("interactive_user", dir.path());
        let repo_local_paths = sample_paths("repo_local", dir.path());

        assert_eq!(
            resolve_scope(&interactive_paths, None).expect("interactive scope"),
            FarmConfigScope::User
        );
        assert_eq!(
            resolve_scope(&repo_local_paths, None).expect("repo_local scope"),
            FarmConfigScope::Workspace
        );
    }

    #[test]
    fn explicit_scope_override_selects_requested_document() {
        let dir = tempdir().expect("tempdir");
        let paths = sample_paths("repo_local", dir.path());
        let document = sample_document(FarmConfigScope::User);
        let path = write(&paths, FarmConfigScope::User, &document).expect("write user farm config");

        let resolved =
            load_from_paths(&paths, Some(FarmConfigScope::User)).expect("load user farm config");
        let resolved = resolved.expect("resolved farm config");

        assert_eq!(resolved.scope, FarmConfigScope::User);
        assert_eq!(resolved.path, path);
        assert_eq!(resolved.document.selection.account, "seller");
        assert_eq!(resolved.document.selection.scope, FarmConfigScope::User);
    }

    #[test]
    fn write_and_load_workspace_config_round_trip() {
        let dir = tempdir().expect("tempdir");
        let paths = sample_paths("repo_local", dir.path());
        let document = sample_document(FarmConfigScope::Workspace);
        let expected_path = PathBuf::from(dir.path())
            .join("workspace/infra/local/runtime/radroots/config/apps/cli/farm.toml");

        let written_path =
            write(&paths, FarmConfigScope::Workspace, &document).expect("write workspace config");
        let resolved = load_from_paths(&paths, None).expect("load workspace config");
        let resolved = resolved.expect("resolved farm config");

        assert_eq!(written_path, expected_path);
        assert_eq!(resolved.path, expected_path);
        assert_eq!(resolved.scope, FarmConfigScope::Workspace);
        assert_eq!(
            resolved.document.selection.scope,
            FarmConfigScope::Workspace
        );
        assert_eq!(
            resolved.document.selection.farm_d_tag,
            "AAAAAAAAAAAAAAAAAAAAAA"
        );
        assert_eq!(resolved.document.farm.d_tag, "AAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(
            resolved.document.listing_defaults.location.primary,
            "San Francisco, CA"
        );
    }

    #[test]
    fn load_rejects_scope_mismatch() {
        let dir = tempdir().expect("tempdir");
        let paths = sample_paths("repo_local", dir.path());
        let path = workspace_config_path(&paths).expect("workspace farm path");
        let Some(parent) = path.parent() else {
            panic!("workspace farm path should have parent");
        };
        fs::create_dir_all(parent).expect("create workspace farm config dir");
        let contents = toml::to_string_pretty(&sample_document(FarmConfigScope::User))
            .expect("encode mismatched farm config");
        fs::write(&path, contents).expect("write mismatched farm config");

        let error = load_from_paths(&paths, None).expect_err("scope mismatch should fail");
        match error {
            RuntimeError::Config(message) => {
                assert!(message.contains("does not match resolved `workspace` scope"));
            }
            other => panic!("expected config error, got {other:?}"),
        }
    }

    #[test]
    fn load_rejects_unsupported_version() {
        let dir = tempdir().expect("tempdir");
        let paths = sample_paths("interactive_user", dir.path());
        let path = user_config_path(&paths).expect("user farm path");
        let Some(parent) = path.parent() else {
            panic!("user farm path should have parent");
        };
        fs::create_dir_all(parent).expect("create user farm config dir");
        let mut document = sample_document(FarmConfigScope::User);
        document.version = 2;
        let contents = toml::to_string_pretty(&document).expect("encode version mismatch");
        fs::write(&path, contents).expect("write version mismatch config");

        let error = load_from_paths(&paths, None).expect_err("version mismatch should fail");
        match error {
            RuntimeError::Config(message) => {
                assert!(message.contains("farm config version must be 1, got 2"));
            }
            other => panic!("expected config error, got {other:?}"),
        }
    }
}
