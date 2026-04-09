use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::runtime::config::{
    CapabilityBindingTargetKind, HyfConfig, INFERENCE_HYF_STDIO_CAPABILITY, RuntimeConfig,
};

const HYF_STATUS_TIMEOUT: Duration = Duration::from_secs(1);
const HYF_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const HYF_STATUS_REQUEST_ID: &str = "cli-doctor-hyf-status";
const HYF_PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyfStatusView {
    pub executable: String,
    pub state: String,
    pub source: String,
    pub reason: Option<String>,
    pub protocol_version: Option<u64>,
    pub deterministic_available: Option<bool>,
}

pub fn resolve_runtime_status(config: &RuntimeConfig) -> HyfStatusView {
    if !config.hyf.enabled {
        return resolve_status(&config.hyf);
    }

    let Some(binding) = config.capability_binding(INFERENCE_HYF_STDIO_CAPABILITY) else {
        return resolve_status(&config.hyf);
    };

    match binding.target_kind {
        CapabilityBindingTargetKind::ExplicitEndpoint => {
            let mut hyf = config.hyf.clone();
            hyf.executable = binding.target.clone().into();
            resolve_status(&hyf)
        }
        CapabilityBindingTargetKind::ManagedInstance => unavailable_status(
            config.hyf.executable.display().to_string(),
            format!(
                "configured hyf binding target `{}` uses unsupported target_kind `managed_instance`; use `explicit_endpoint` for `inference.hyf_stdio`",
                binding.target
            ),
            None,
            None,
        ),
    }
}

pub fn resolve_status(config: &HyfConfig) -> HyfStatusView {
    let executable = config.executable.display().to_string();
    if !config.enabled {
        return HyfStatusView {
            executable,
            state: "disabled".to_owned(),
            source: "hyf status control request · local first".to_owned(),
            reason: Some("disabled by config".to_owned()),
            protocol_version: None,
            deterministic_available: None,
        };
    }

    if config.executable.as_os_str().is_empty() {
        return unavailable_status(
            executable,
            "hyf executable path is not configured".to_owned(),
            None,
            None,
        );
    }

    let output = match run_status_command(config) {
        Ok(output) => output,
        Err(HyfCommandError::NotFound) => {
            return unavailable_status(
                executable,
                format!(
                    "hyf executable was not found at {}",
                    config.executable.display()
                ),
                None,
                None,
            );
        }
        Err(HyfCommandError::Start(error)) => {
            return unavailable_status(
                executable,
                format!(
                    "failed to start hyf control request at {}: {error}",
                    config.executable.display()
                ),
                None,
                None,
            );
        }
        Err(HyfCommandError::Write(error)) => {
            return unavailable_status(
                executable,
                format!("failed to write hyf control request stdin: {error}"),
                None,
                None,
            );
        }
        Err(HyfCommandError::Timeout) => {
            return unavailable_status(
                executable,
                format!(
                    "hyf status control request timed out after {}ms",
                    HYF_STATUS_TIMEOUT.as_millis()
                ),
                None,
                None,
            );
        }
        Err(HyfCommandError::Wait(error)) | Err(HyfCommandError::Read(error)) => {
            return unavailable_status(
                executable,
                format!("failed to capture hyf status control output: {error}"),
                None,
                None,
            );
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let reason = match output.status.code() {
            Some(code) if stderr.is_empty() => {
                format!("hyf status control request exited with status code {code}")
            }
            Some(code) => {
                format!("hyf status control request exited with status code {code}: {stderr}")
            }
            None if stderr.is_empty() => {
                "hyf status control request terminated by signal".to_owned()
            }
            None => format!("hyf status control request terminated by signal: {stderr}"),
        };
        return unavailable_status(executable, reason, None, None);
    }

    let stdout = match String::from_utf8(output.stdout) {
        Ok(stdout) => stdout,
        Err(error) => {
            return unavailable_status(
                executable,
                format!("hyf status output was not valid UTF-8: {error}"),
                None,
                None,
            );
        }
    };

    let payload: Value = match serde_json::from_str(stdout.as_str()) {
        Ok(payload) => payload,
        Err(error) => {
            return unavailable_status(
                executable,
                format!("hyf status output was not valid JSON: {error}"),
                None,
                None,
            );
        }
    };

    let response_version = payload.get("version").and_then(Value::as_u64);
    let request_id = payload.get("request_id").and_then(Value::as_str);
    let protocol_version = payload
        .get("output")
        .and_then(|output| output.get("build_identity"))
        .and_then(|identity| identity.get("protocol_version"))
        .and_then(Value::as_u64);
    let deterministic_available = payload
        .get("output")
        .and_then(|output| output.get("enabled_execution_modes"))
        .and_then(|modes| modes.get("deterministic"))
        .and_then(Value::as_bool);

    if response_version != Some(HYF_PROTOCOL_VERSION) {
        return unavailable_status(
            executable,
            format!(
                "hyf status response version {:?} is incompatible with cli expected {}",
                response_version, HYF_PROTOCOL_VERSION
            ),
            protocol_version,
            deterministic_available,
        );
    }

    if request_id != Some(HYF_STATUS_REQUEST_ID) {
        return unavailable_status(
            executable,
            "hyf status response did not preserve the control request id".to_owned(),
            protocol_version,
            deterministic_available,
        );
    }

    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        let reason = payload
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(Value::as_str)
            .map(|code| format!("hyf status control request returned error code {code}"))
            .unwrap_or_else(|| {
                "hyf status control request returned an invalid error response".to_owned()
            });
        return unavailable_status(
            executable,
            reason,
            protocol_version,
            deterministic_available,
        );
    }

    if protocol_version != Some(HYF_PROTOCOL_VERSION) {
        return unavailable_status(
            executable,
            format!(
                "hyf protocol version {:?} is incompatible with cli expected {}",
                protocol_version, HYF_PROTOCOL_VERSION
            ),
            protocol_version,
            deterministic_available,
        );
    }

    if deterministic_available != Some(true) {
        return unavailable_status(
            executable,
            "hyf deterministic execution is unavailable".to_owned(),
            protocol_version,
            deterministic_available,
        );
    }

    HyfStatusView {
        executable,
        state: "ready".to_owned(),
        source: "hyf status control request · local first".to_owned(),
        reason: Some("healthy · protocol 1 · deterministic available".to_owned()),
        protocol_version,
        deterministic_available,
    }
}

fn run_status_command(config: &HyfConfig) -> Result<Output, HyfCommandError> {
    let mut child = Command::new(&config.executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => HyfCommandError::NotFound,
            _ => HyfCommandError::Start(error),
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let request = json!({
            "version": HYF_PROTOCOL_VERSION,
            "request_id": HYF_STATUS_REQUEST_ID,
            "trace_id": HYF_STATUS_REQUEST_ID,
            "capability": "sys.status",
            "input": {}
        });
        writeln!(stdin, "{request}").map_err(HyfCommandError::Write)?;
    }

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return collect_output(child, status),
            Ok(None) => {
                if started_at.elapsed() >= HYF_STATUS_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(HyfCommandError::Timeout);
                }
                thread::sleep(HYF_STATUS_POLL_INTERVAL);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HyfCommandError::Wait(error));
            }
        }
    }
}

fn collect_output(mut child: Child, status: ExitStatus) -> Result<Output, HyfCommandError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .map_err(HyfCommandError::Read)?;
    }
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_end(&mut stderr)
            .map_err(HyfCommandError::Read)?;
    }

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn unavailable_status(
    executable: String,
    reason: String,
    protocol_version: Option<u64>,
    deterministic_available: Option<bool>,
) -> HyfStatusView {
    HyfStatusView {
        executable,
        state: "unavailable".to_owned(),
        source: "hyf status control request · local first".to_owned(),
        reason: Some(reason),
        protocol_version,
        deterministic_available,
    }
}

enum HyfCommandError {
    NotFound,
    Start(std::io::Error),
    Write(std::io::Error),
    Wait(std::io::Error),
    Read(std::io::Error),
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::{HYF_PROTOCOL_VERSION, resolve_status};
    use crate::runtime::config::HyfConfig;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn disabled_hyf_reports_disabled_state_without_spawning() {
        let view = resolve_status(&HyfConfig {
            enabled: false,
            executable: "hyfd".into(),
        });
        assert_eq!(view.state, "disabled");
        assert_eq!(view.reason.as_deref(), Some("disabled by config"));
    }

    #[test]
    fn healthy_hyf_status_reports_ready() {
        let dir = tempdir().expect("tempdir");
        let executable = write_script(
            dir.path(),
            format!(
                "#!/bin/sh\nread -r _request || exit 64\ncat <<'JSON'\n{{\"version\":{HYF_PROTOCOL_VERSION},\"request_id\":\"cli-doctor-hyf-status\",\"trace_id\":\"cli-doctor-hyf-status\",\"ok\":true,\"output\":{{\"build_identity\":{{\"protocol_version\":{HYF_PROTOCOL_VERSION}}},\"enabled_execution_modes\":{{\"deterministic\":true}}}}}}\nJSON\n"
            )
            .as_str(),
        );

        let view = resolve_status(&HyfConfig {
            enabled: true,
            executable,
        });
        assert_eq!(view.state, "ready");
        assert_eq!(view.protocol_version, Some(HYF_PROTOCOL_VERSION));
        assert_eq!(view.deterministic_available, Some(true));
    }

    #[test]
    fn incompatible_hyf_status_reports_unavailable() {
        let dir = tempdir().expect("tempdir");
        let executable = write_script(
            dir.path(),
            "#!/bin/sh\nread -r _request || exit 64\ncat <<'JSON'\n{\"version\":1,\"request_id\":\"cli-doctor-hyf-status\",\"trace_id\":\"cli-doctor-hyf-status\",\"ok\":true,\"output\":{\"build_identity\":{\"protocol_version\":2},\"enabled_execution_modes\":{\"deterministic\":true}}}\nJSON\n",
        );

        let view = resolve_status(&HyfConfig {
            enabled: true,
            executable,
        });
        assert_eq!(view.state, "unavailable");
        assert!(
            view.reason
                .as_deref()
                .is_some_and(|reason| reason.contains("incompatible"))
        );
    }

    fn write_script(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
        let path = dir.join("fake-hyfd");
        fs::write(&path, script).expect("write fake hyfd");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod fake hyfd");
        path
    }
}
