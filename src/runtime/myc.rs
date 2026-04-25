use std::io::Read;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use radroots_nostr_signer::prelude::{
    RadrootsNostrLocalSignerCapability, RadrootsNostrRemoteSessionSignerCapability,
};
use serde::Deserialize;

use crate::domain::runtime::{
    IdentityPublicView, LocalSignerStatusView, MycCustodyIdentityView, MycCustodyView,
    MycRemoteSessionView, MycStatusView,
};
use crate::runtime::config::MycConfig;

const MYC_SIGNER_STATUS_CONTRACT_VERSION: u32 = 1;
const MYC_STATUS_VIEW: &str = "signer";
const MYC_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub fn resolve_status(config: &MycConfig) -> MycStatusView {
    let executable = config.executable.display().to_string();
    if config.executable.as_os_str().is_empty() {
        return unavailable_status(
            executable,
            "unconfigured",
            "myc executable path is not configured".to_owned(),
        );
    }

    let output = match run_status_command(config) {
        Ok(output) => output,
        Err(MycCommandError::NotFound) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "myc executable was not found at {}",
                    config.executable.display()
                ),
            );
        }
        Err(MycCommandError::Start(error)) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "failed to start myc status command at {}: {error}",
                    config.executable.display()
                ),
            );
        }
        Err(MycCommandError::Timeout) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "myc status command timed out after {}ms",
                    config.status_timeout_ms
                ),
            );
        }
        Err(MycCommandError::Wait(error)) | Err(MycCommandError::Read(error)) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "failed to capture myc status command output at {}: {error}",
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

    let payload_value = match serde_json::from_str::<serde_json::Value>(stdout.as_str()) {
        Ok(payload) => payload,
        Err(error) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!("myc signer status output was not valid JSON: {error}"),
            );
        }
    };
    let payload = match serde_json::from_value::<MycStatusPayload>(payload_value) {
        Ok(payload) => payload,
        Err(error) => {
            return unavailable_status(
                executable,
                "unavailable",
                format!(
                    "myc signer status output did not match contract version {MYC_SIGNER_STATUS_CONTRACT_VERSION}: {error}"
                ),
            );
        }
    };

    let MycStatusPayload {
        status_contract_version,
        status,
        ready,
        reasons,
        signer_backend,
        custody,
    } = payload;
    if status_contract_version != MYC_SIGNER_STATUS_CONTRACT_VERSION {
        return unavailable_status(
            executable,
            "unavailable",
            format!(
                "myc signer status contract version {status_contract_version} is incompatible with cli expected {MYC_SIGNER_STATUS_CONTRACT_VERSION}"
            ),
        );
    }
    let MycSignerBackendPayload {
        local_signer,
        remote_session_count,
        remote_sessions,
    } = signer_backend;

    let remote_sessions = remote_sessions
        .iter()
        .map(remote_session_status_view)
        .collect::<Vec<_>>();
    let local_signer = local_signer.map(local_signer_status_view);
    let remote_session_count = remote_session_count.max(remote_sessions.len());
    let custody = custody.into_view();
    let state = if ready {
        "ready"
    } else {
        match status.as_str() {
            "degraded" => "degraded",
            _ => "unavailable",
        }
    };
    let reason = primary_reason(ready, status.as_str(), reasons.as_slice());

    MycStatusView {
        executable,
        state: state.to_owned(),
        source: "myc status command · local first".to_owned(),
        service_status: Some(status),
        ready,
        reason,
        reasons,
        remote_session_count,
        local_signer,
        remote_sessions,
        custody,
    }
}

fn run_status_command(config: &MycConfig) -> Result<Output, MycCommandError> {
    let mut child = Command::new(&config.executable)
        .args(["status", "--view", MYC_STATUS_VIEW])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => MycCommandError::NotFound,
            _ => MycCommandError::Start(error),
        })?;

    let started_at = Instant::now();
    let status_timeout = Duration::from_millis(config.status_timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return collect_output(child, status),
            Ok(None) => {
                if started_at.elapsed() >= status_timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MycCommandError::Timeout);
                }
                thread::sleep(MYC_STATUS_POLL_INTERVAL);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MycCommandError::Wait(error));
            }
        }
    }
}

fn collect_output(mut child: Child, status: ExitStatus) -> Result<Output, MycCommandError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .map_err(MycCommandError::Read)?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)
            .map_err(MycCommandError::Read)?;
    }

    Ok(Output {
        status,
        stdout,
        stderr,
    })
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
        backend: "myc".to_owned(),
        used_fallback: false,
    }
}

fn remote_session_status_view(
    capability: &RadrootsNostrRemoteSessionSignerCapability,
) -> MycRemoteSessionView {
    MycRemoteSessionView {
        connection_id: capability.connection_id.to_string(),
        signer_identity: IdentityPublicView::from_public_identity(&capability.signer_identity),
        user_identity: IdentityPublicView::from_public_identity(&capability.user_identity),
        relay_count: capability.relays.len(),
        permissions: capability
            .permissions
            .as_slice()
            .iter()
            .map(ToString::to_string)
            .collect(),
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
        source: "myc status command · local first".to_owned(),
        service_status: None,
        ready: false,
        reason: Some(reason),
        reasons: Vec::new(),
        remote_session_count: 0,
        local_signer: None,
        remote_sessions: Vec::new(),
        custody: None,
    }
}

enum MycCommandError {
    NotFound,
    Start(std::io::Error),
    Wait(std::io::Error),
    Read(std::io::Error),
    Timeout,
}

#[derive(Debug, Deserialize)]
struct MycStatusPayload {
    status_contract_version: u32,
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
    #[serde(default)]
    remote_session_count: usize,
    #[serde(default)]
    remote_sessions: Vec<RadrootsNostrRemoteSessionSignerCapability>,
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
            status_timeout_ms: 2_000,
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
