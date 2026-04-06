use std::process::Command;

use radroots_nostr_signer::prelude::RadrootsNostrLocalSignerCapability;
use serde::Deserialize;

use crate::domain::runtime::{
    IdentityPublicView, LocalSignerStatusView, MycCustodyIdentityView, MycCustodyView,
    MycStatusView,
};
use crate::runtime::config::MycConfig;

pub fn resolve_status(config: &MycConfig) -> MycStatusView {
    let executable = config.executable.display().to_string();
    if config.executable.as_os_str().is_empty() {
        return unavailable_status(
            executable,
            "unconfigured",
            "myc executable path is not configured".to_owned(),
        );
    }

    let output = match Command::new(&config.executable)
        .args(["status", "--view", "full"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "myc executable was not found at {}",
                    config.executable.display()
                ),
            );
        }
        Err(error) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "failed to start myc status command at {}: {error}",
                    config.executable.display()
                ),
            );
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let reason = match output.status.code() {
            Some(code) if stderr.is_empty() => {
                format!("myc status command exited with status code {code}")
            }
            Some(code) => format!("myc status command exited with status code {code}: {stderr}"),
            None if stderr.is_empty() => "myc status command terminated by signal".to_owned(),
            None => format!("myc status command terminated by signal: {stderr}"),
        };
        return unavailable_status(executable, "unavailable", reason);
    }

    let stdout = match String::from_utf8(output.stdout) {
        Ok(stdout) => stdout,
        Err(error) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!("myc status output was not valid UTF-8: {error}"),
            );
        }
    };

    let payload = match serde_json::from_str::<MycStatusPayload>(stdout.as_str()) {
        Ok(payload) => payload,
        Err(error) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!("myc status output was not valid JSON: {error}"),
            );
        }
    };

    let local_signer = payload
        .signer_backend
        .local_signer
        .map(local_signer_status_view);
    let custody = payload.custody.into_view();
    let state = if payload.ready {
        "ready"
    } else {
        match payload.status.as_str() {
            "degraded" => "degraded",
            _ => "unavailable",
        }
    };
    let reason = primary_reason(
        payload.ready,
        payload.status.as_str(),
        payload.reasons.as_slice(),
    );

    MycStatusView {
        executable,
        state: state.to_owned(),
        service_status: Some(payload.status),
        ready: payload.ready,
        reason,
        reasons: payload.reasons,
        local_signer,
        custody,
    }
}

fn local_signer_status_view(
    capability: RadrootsNostrLocalSignerCapability,
) -> LocalSignerStatusView {
    LocalSignerStatusView {
        account_id: capability.account_id.to_string(),
        public_identity: IdentityPublicView::from_public_identity(&capability.public_identity),
        availability: match capability.availability {
            radroots_nostr_signer::prelude::RadrootsNostrLocalSignerAvailability::PublicOnly => {
                "public_only".to_owned()
            }
            radroots_nostr_signer::prelude::RadrootsNostrLocalSignerAvailability::SecretBacked => {
                "secret_backed".to_owned()
            }
        },
        secret_backed: capability.is_secret_backed(),
    }
}

fn primary_reason(ready: bool, service_status: &str, reasons: &[String]) -> Option<String> {
    if ready {
        return None;
    }

    reasons
        .first()
        .cloned()
        .or_else(|| Some(format!("myc reported service status `{service_status}`")))
}

fn unavailable_status(executable: String, state: &str, reason: String) -> MycStatusView {
    MycStatusView {
        executable,
        state: state.to_owned(),
        service_status: None,
        ready: false,
        reason: Some(reason),
        reasons: Vec::new(),
        local_signer: None,
        custody: None,
    }
}

#[derive(Debug, Deserialize)]
struct MycStatusPayload {
    status: String,
    ready: bool,
    #[serde(default)]
    reasons: Vec<String>,
    #[serde(default)]
    signer_backend: MycSignerBackendPayload,
    #[serde(default)]
    custody: MycCustodyPayload,
}

#[derive(Debug, Default, Deserialize)]
struct MycSignerBackendPayload {
    #[serde(default)]
    local_signer: Option<RadrootsNostrLocalSignerCapability>,
}

#[derive(Debug, Default, Deserialize)]
struct MycCustodyPayload {
    #[serde(default)]
    signer: MycCustodyIdentityPayload,
    #[serde(default)]
    user: MycCustodyIdentityPayload,
    #[serde(default)]
    discovery_app: Option<MycCustodyIdentityPayload>,
}

impl MycCustodyPayload {
    fn into_view(self) -> Option<MycCustodyView> {
        if !self.signer.has_data()
            && !self.user.has_data()
            && self
                .discovery_app
                .as_ref()
                .is_none_or(|identity| !identity.has_data())
        {
            return None;
        }

        Some(MycCustodyView {
            signer: self.signer.into_view(),
            user: self.user.into_view(),
            discovery_app: self.discovery_app.and_then(|identity| {
                if identity.has_data() {
                    Some(identity.into_view())
                } else {
                    None
                }
            }),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct MycCustodyIdentityPayload {
    #[serde(default)]
    resolved: bool,
    #[serde(default)]
    selected_account_id: Option<String>,
    #[serde(default)]
    selected_account_state: Option<String>,
    #[serde(default)]
    identity_id: Option<String>,
    #[serde(default)]
    public_key_hex: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

impl MycCustodyIdentityPayload {
    fn has_data(&self) -> bool {
        self.resolved
            || self.selected_account_id.is_some()
            || self.selected_account_state.is_some()
            || self.identity_id.is_some()
            || self.public_key_hex.is_some()
            || self.error.is_some()
    }

    fn into_view(self) -> MycCustodyIdentityView {
        MycCustodyIdentityView {
            resolved: self.resolved,
            selected_account_id: self.selected_account_id,
            selected_account_state: self.selected_account_state,
            identity_id: self.identity_id,
            public_key_hex: self.public_key_hex,
            error: self.error,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::resolve_status;
    use crate::runtime::config::MycConfig;

    #[test]
    fn empty_executable_path_reports_unconfigured_status() {
        let view = resolve_status(&MycConfig {
            executable: PathBuf::new(),
        });

        assert_eq!(view.state, "unconfigured");
        assert_eq!(view.ready, false);
        assert!(
            view.reason
                .as_deref()
                .is_some_and(|value| value.contains("not configured"))
        );
    }
}
