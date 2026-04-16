use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use radroots_events::farm::{RadrootsFarm, RadrootsFarmLocation};
use radroots_events::listing::RadrootsListingLocation;
use radroots_events::profile::RadrootsProfile;
use radroots_events_codec::d_tag::is_d_tag_base64url;

use crate::cli::{FarmScopeArg, FarmScopedArgs, FarmSetupArgs};
use crate::domain::runtime::{
    FarmConfigDocumentView, FarmConfigSummaryView, FarmGetView, FarmListingDefaultsView,
    FarmPublicationView, FarmSelectionView, FarmSetupView, FarmStatusView,
};
use crate::runtime::RuntimeError;
use crate::runtime::accounts::{self, AccountRecordView};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::farm_config::{
    self, FarmConfigDocument, FarmConfigScope, FarmConfigSelection, FarmListingDefaults,
    FarmPublicationStatus, ResolvedFarmConfig, SUPPORTED_FARM_CONFIG_VERSION,
};

const FARM_CONFIG_SOURCE: &str = "farm config · local first";

static D_TAG_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn setup(config: &RuntimeConfig, args: &FarmSetupArgs) -> Result<FarmSetupView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let selected_account = match accounts::resolve_account(config)? {
        Some(account) => account,
        None => {
            return Ok(FarmSetupView {
                state: "unconfigured".to_owned(),
                source: FARM_CONFIG_SOURCE.to_owned(),
                config: None,
                reason: Some("farm setup requires a selected local account".to_owned()),
                actions: vec![
                    "radroots account new".to_owned(),
                    "radroots account whoami".to_owned(),
                ],
            });
        }
    };
    let existing = farm_config::load(config, Some(resolved_scope))?;
    let document = setup_document(args, resolved_scope, &selected_account, existing.as_ref())?;
    let path = farm_config::write(&config.paths, resolved_scope, &document)?;
    let summary = summary_view(
        resolved_scope,
        path.display().to_string(),
        &document,
        Some(
            selected_account
                .record
                .public_identity
                .public_key_hex
                .as_str(),
        ),
    );

    Ok(FarmSetupView {
        state: "configured".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        config: Some(summary),
        reason: None,
        actions: vec![
            "radroots farm status".to_owned(),
            "radroots farm get".to_owned(),
        ],
    })
}

pub fn status(
    config: &RuntimeConfig,
    args: &FarmScopedArgs,
) -> Result<FarmStatusView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmStatusView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            scope: resolved_scope.as_str().to_owned(),
            path: path.display().to_string(),
            config_present: false,
            config_valid: false,
            account_state: "not_checked".to_owned(),
            listing_defaults_state: "missing".to_owned(),
            config: None,
            reason: Some(format!("no farm config found at {}", path.display())),
            actions: vec![setup_action(resolved_scope)],
        });
    };

    let account = configured_account(config, &resolved.document.selection.account)?;
    let account_state = if account.is_some() {
        "ready"
    } else {
        "missing"
    };
    let state = if account.is_some() {
        "ready"
    } else {
        "unconfigured"
    };
    let reason = if account.is_some() {
        None
    } else {
        Some(format!(
            "farm config account `{}` is not present in the local account store",
            resolved.document.selection.account
        ))
    };
    let mut actions = Vec::new();
    if account.is_none() {
        actions.push("radroots account new".to_owned());
        actions.push(setup_action(resolved.scope));
    }
    let account_pubkey = account
        .as_ref()
        .map(|account| account.record.public_identity.public_key_hex.as_str());

    Ok(FarmStatusView {
        state: state.to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        config_valid: true,
        account_state: account_state.to_owned(),
        listing_defaults_state: "ready".to_owned(),
        config: Some(summary_view(
            resolved.scope,
            resolved.path.display().to_string(),
            &resolved.document,
            account_pubkey,
        )),
        reason,
        actions,
    })
}

pub fn get(config: &RuntimeConfig, args: &FarmScopedArgs) -> Result<FarmGetView, RuntimeError> {
    let scope = scope_from_arg(args.scope);
    let resolved_scope = farm_config::resolve_scope(&config.paths, scope)?;
    let path = farm_config::config_path(&config.paths, resolved_scope)?;
    let Some(resolved) = farm_config::load(config, Some(resolved_scope))? else {
        return Ok(FarmGetView {
            state: "unconfigured".to_owned(),
            source: FARM_CONFIG_SOURCE.to_owned(),
            scope: resolved_scope.as_str().to_owned(),
            path: path.display().to_string(),
            config_present: false,
            document: None,
            reason: Some(format!("no farm config found at {}", path.display())),
            actions: vec![setup_action(resolved_scope)],
        });
    };

    Ok(FarmGetView {
        state: "ready".to_owned(),
        source: FARM_CONFIG_SOURCE.to_owned(),
        scope: resolved.scope.as_str().to_owned(),
        path: resolved.path.display().to_string(),
        config_present: true,
        document: Some(document_view(&resolved.document)),
        reason: None,
        actions: Vec::new(),
    })
}

fn setup_document(
    args: &FarmSetupArgs,
    scope: FarmConfigScope,
    account: &AccountRecordView,
    existing: Option<&ResolvedFarmConfig>,
) -> Result<FarmConfigDocument, RuntimeError> {
    let existing_document = existing.map(|resolved| &resolved.document);
    let name = required_text(args.name.as_str(), "farm.name")?;
    let location_primary = required_text(args.location.as_str(), "farm.location.primary")?;
    let delivery_method = required_text(
        args.delivery_method.as_str(),
        "listing_defaults.delivery_method",
    )?;
    let farm_d_tag = match args.farm_d_tag.as_deref() {
        Some(value) => required_d_tag(value, "farm_d_tag")?,
        None => existing_document
            .map(|document| document.farm.d_tag.clone())
            .unwrap_or_else(generate_d_tag),
    };
    if !is_d_tag_base64url(farm_d_tag.as_str()) {
        return Err(RuntimeError::Config(
            "farm_d_tag must be a 22-character base64url identifier".to_owned(),
        ));
    }

    let about = optional_arg_or_existing(
        args.about.as_ref(),
        existing_document.and_then(|document| document.profile.about.as_ref()),
    );
    let website = optional_arg_or_existing(
        args.website.as_ref(),
        existing_document.and_then(|document| document.profile.website.as_ref()),
    );
    let picture = optional_arg_or_existing(
        args.picture.as_ref(),
        existing_document.and_then(|document| document.profile.picture.as_ref()),
    );
    let banner = optional_arg_or_existing(
        args.banner.as_ref(),
        existing_document.and_then(|document| document.profile.banner.as_ref()),
    );
    let display_name = optional_arg_or_existing(
        args.display_name.as_ref(),
        existing_document.and_then(|document| document.profile.display_name.as_ref()),
    )
    .or_else(|| Some(name.clone()));
    let city = optional_arg_or_existing(
        args.city.as_ref(),
        existing_document
            .and_then(|document| document.farm.location.as_ref())
            .and_then(|location| location.city.as_ref()),
    );
    let region = optional_arg_or_existing(
        args.region.as_ref(),
        existing_document
            .and_then(|document| document.farm.location.as_ref())
            .and_then(|location| location.region.as_ref()),
    );
    let country = optional_arg_or_existing(
        args.country.as_ref(),
        existing_document
            .and_then(|document| document.farm.location.as_ref())
            .and_then(|location| location.country.as_ref()),
    );
    let publication = existing_document
        .filter(|document| document.farm.d_tag == farm_d_tag)
        .map(|document| document.publication.clone())
        .unwrap_or_default();

    Ok(FarmConfigDocument {
        version: SUPPORTED_FARM_CONFIG_VERSION,
        selection: FarmConfigSelection {
            scope,
            account: account.record.account_id.to_string(),
            farm_d_tag: farm_d_tag.clone(),
        },
        profile: RadrootsProfile {
            name: name.clone(),
            display_name,
            nip05: None,
            about: about.clone(),
            website: website.clone(),
            picture: picture.clone(),
            banner: banner.clone(),
            lud06: None,
            lud16: None,
            bot: None,
        },
        farm: RadrootsFarm {
            d_tag: farm_d_tag,
            name,
            about,
            website,
            picture,
            banner,
            location: Some(RadrootsFarmLocation {
                primary: Some(location_primary.clone()),
                city: city.clone(),
                region: region.clone(),
                country: country.clone(),
                gcs: None,
            }),
            tags: None,
        },
        listing_defaults: FarmListingDefaults {
            delivery_method,
            location: RadrootsListingLocation {
                primary: location_primary,
                city,
                region,
                country,
                lat: None,
                lng: None,
                geohash: None,
            },
        },
        publication,
    })
}

fn configured_account(
    config: &RuntimeConfig,
    account_id: &str,
) -> Result<Option<AccountRecordView>, RuntimeError> {
    let snapshot = accounts::snapshot(config)?;
    Ok(snapshot
        .accounts
        .into_iter()
        .find(|account| account.record.account_id.as_str() == account_id))
}

fn summary_view(
    scope: FarmConfigScope,
    path: String,
    document: &FarmConfigDocument,
    account_pubkey: Option<&str>,
) -> FarmConfigSummaryView {
    FarmConfigSummaryView {
        scope: scope.as_str().to_owned(),
        path,
        selected_account_id: document.selection.account.clone(),
        selected_account_pubkey: account_pubkey.map(str::to_owned),
        farm_d_tag: document.selection.farm_d_tag.clone(),
        name: document.farm.name.clone(),
        location_primary: document
            .farm
            .location
            .as_ref()
            .and_then(|location| location.primary.clone()),
        delivery_method: document.listing_defaults.delivery_method.clone(),
        publication: publication_view(&document.publication),
    }
}

fn document_view(document: &FarmConfigDocument) -> FarmConfigDocumentView {
    FarmConfigDocumentView {
        selection: FarmSelectionView {
            scope: document.selection.scope.as_str().to_owned(),
            account: document.selection.account.clone(),
            farm_d_tag: document.selection.farm_d_tag.clone(),
        },
        profile: document.profile.clone(),
        farm: document.farm.clone(),
        listing_defaults: FarmListingDefaultsView {
            delivery_method: document.listing_defaults.delivery_method.clone(),
            location: document.listing_defaults.location.clone(),
        },
        publication: publication_view(&document.publication),
    }
}

fn publication_view(publication: &FarmPublicationStatus) -> FarmPublicationView {
    FarmPublicationView {
        profile_state: publish_state(
            publication.profile_event_id.as_deref(),
            publication.profile_published_at,
        )
        .to_owned(),
        farm_state: publish_state(
            publication.farm_event_id.as_deref(),
            publication.farm_published_at,
        )
        .to_owned(),
        profile_event_id: publication.profile_event_id.clone(),
        farm_event_id: publication.farm_event_id.clone(),
        profile_published_at: publication.profile_published_at,
        farm_published_at: publication.farm_published_at,
    }
}

fn publish_state(event_id: Option<&str>, published_at: Option<u64>) -> &'static str {
    if event_id.is_some_and(|value| !value.trim().is_empty()) || published_at.is_some() {
        "published"
    } else {
        "not_published"
    }
}

fn setup_action(scope: FarmConfigScope) -> String {
    format!(
        "radroots farm setup --scope {} --name <farm-name> --location <place>",
        scope.as_str()
    )
}

fn scope_from_arg(scope: Option<FarmScopeArg>) -> Option<FarmConfigScope> {
    scope.map(|scope| match scope {
        FarmScopeArg::User => FarmConfigScope::User,
        FarmScopeArg::Workspace => FarmConfigScope::Workspace,
    })
}

fn required_d_tag(value: &str, field: &str) -> Result<String, RuntimeError> {
    let value = required_text(value, field)?;
    if !is_d_tag_base64url(value.as_str()) {
        return Err(RuntimeError::Config(format!(
            "{field} must be a 22-character base64url identifier"
        )));
    }
    Ok(value)
}

fn required_text(value: &str, field: &str) -> Result<String, RuntimeError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::Config(format!("{field} must not be empty")));
    }
    Ok(trimmed.to_owned())
}

fn optional_arg_or_existing(arg: Option<&String>, existing: Option<&String>) -> Option<String> {
    arg.and_then(|value| non_empty(value.as_str()))
        .or_else(|| existing.and_then(|value| non_empty(value.as_str())))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn generate_d_tag() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = D_TAG_COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    encode_base64url_no_pad((nanos ^ counter).to_be_bytes())
}

fn encode_base64url_no_pad(bytes: [u8; 16]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity(22);
    let mut index = 0usize;
    while index + 3 <= bytes.len() {
        let block = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
        output.push(ALPHABET[(block & 0x3f) as usize] as char);
        index += 3;
    }
    let remaining = bytes.len() - index;
    if remaining == 1 {
        let block = (bytes[index] as u32) << 16;
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
    } else if remaining == 2 {
        let block = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        output.push(ALPHABET[((block >> 18) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 12) & 0x3f) as usize] as char);
        output.push(ALPHABET[((block >> 6) & 0x3f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::generate_d_tag;
    use radroots_events_codec::d_tag::is_d_tag_base64url;

    #[test]
    fn generated_farm_d_tag_is_valid_base64url() {
        assert!(is_d_tag_base64url(&generate_d_tag()));
    }
}
