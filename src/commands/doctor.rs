use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, DoctorCheckView, DoctorView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::{RuntimeConfig, SignerBackend};
use crate::runtime::logging::LoggingState;
use crate::runtime::signer::resolve_signer_status;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DoctorSeverity {
    Ok,
    Warn,
    ExternalFail,
    InternalFail,
}

impl DoctorSeverity {
    fn status(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::ExternalFail | Self::InternalFail => "fail",
        }
    }

    fn command_disposition(self) -> CommandDisposition {
        match self {
            Self::Ok => CommandDisposition::Success,
            Self::Warn => CommandDisposition::Unconfigured,
            Self::ExternalFail => CommandDisposition::ExternalUnavailable,
            Self::InternalFail => CommandDisposition::InternalError,
        }
    }
}

struct EvaluatedCheck {
    severity: DoctorSeverity,
    view: DoctorCheckView,
    action: Option<&'static str>,
}

pub fn report(
    config: &RuntimeConfig,
    logging: &LoggingState,
) -> Result<CommandOutput, RuntimeError> {
    let mut checks = Vec::new();
    checks.push(config_check(config));
    checks.push(account_check(config)?);

    let signer = resolve_signer_status(config);
    checks.push(signer_check(&signer));

    if matches!(config.signer.backend, SignerBackend::Myc) {
        if let Some(myc) = signer.myc.as_ref() {
            checks.push(myc_check(myc));
        }
    }

    checks.push(logging_check(config, logging));

    let severity = checks
        .iter()
        .map(|check| check.severity)
        .max()
        .unwrap_or(DoctorSeverity::Ok);
    let actions = collect_actions(&checks);
    let view = DoctorView {
        ok: severity == DoctorSeverity::Ok,
        state: severity.status().to_owned(),
        checks: checks.into_iter().map(|check| check.view).collect(),
        source: match config.signer.backend {
            SignerBackend::Local => "local diagnostics".to_owned(),
            SignerBackend::Myc => "local diagnostics + myc status command".to_owned(),
        },
        actions,
    };

    Ok(match severity.command_disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::Doctor(view)),
        CommandDisposition::Unconfigured => CommandOutput::unconfigured(CommandView::Doctor(view)),
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::Doctor(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::Doctor(view))
        }
    })
}

fn config_check(config: &RuntimeConfig) -> EvaluatedCheck {
    let detail = match (
        config.paths.user_config_path.exists(),
        config.paths.workspace_config_path.exists(),
    ) {
        (false, false) => "defaults active".to_owned(),
        (true, false) => "user config root present".to_owned(),
        (false, true) => "workspace config root present".to_owned(),
        (true, true) => "user and workspace config roots present".to_owned(),
    };

    EvaluatedCheck {
        severity: DoctorSeverity::Ok,
        view: DoctorCheckView {
            name: "config".to_owned(),
            status: "ok".to_owned(),
            detail,
        },
        action: None,
    }
}

fn account_check(config: &RuntimeConfig) -> Result<EvaluatedCheck, RuntimeError> {
    let snapshot = crate::runtime::accounts::snapshot(config)?;
    if snapshot.accounts.is_empty() {
        return Ok(EvaluatedCheck {
            severity: DoctorSeverity::Warn,
            view: DoctorCheckView {
                name: "account".to_owned(),
                status: "warn".to_owned(),
                detail: format!(
                    "no local accounts found in {}",
                    config.account.store_path.display()
                ),
            },
            action: Some("radroots account new"),
        });
    }

    match crate::runtime::accounts::resolve_account(config)? {
        Some(account) => {
            let detail = if account.selected {
                format!("{} selected", account.record.account_id)
            } else {
                format!("{} resolved by selector", account.record.account_id)
            };
            Ok(EvaluatedCheck {
                severity: DoctorSeverity::Ok,
                view: DoctorCheckView {
                    name: "account".to_owned(),
                    status: "ok".to_owned(),
                    detail,
                },
                action: None,
            })
        }
        None => Ok(EvaluatedCheck {
            severity: DoctorSeverity::Warn,
            view: DoctorCheckView {
                name: "account".to_owned(),
                status: "warn".to_owned(),
                detail: format!(
                    "accounts exist but no local account is selected in {}",
                    config.account.store_path.display()
                ),
            },
            action: Some("radroots account ls"),
        }),
    }
}

fn signer_check(signer: &crate::domain::runtime::SignerStatusView) -> EvaluatedCheck {
    let (severity, detail, action) = match signer.state.as_str() {
        "ready" => (
            DoctorSeverity::Ok,
            format!("{} ready", signer.backend),
            None,
        ),
        "unconfigured" => (
            DoctorSeverity::Warn,
            signer
                .reason
                .clone()
                .unwrap_or_else(|| format!("{} signer is not configured", signer.backend)),
            Some("radroots signer status"),
        ),
        "degraded" | "unavailable" => (
            DoctorSeverity::ExternalFail,
            signer
                .reason
                .clone()
                .unwrap_or_else(|| format!("{} signer is unavailable", signer.backend)),
            Some(if signer.backend == "myc" {
                "radroots myc status"
            } else {
                "radroots signer status"
            }),
        ),
        _ => (
            DoctorSeverity::InternalFail,
            signer
                .reason
                .clone()
                .unwrap_or_else(|| format!("{} signer reported an internal error", signer.backend)),
            Some("radroots signer status --json"),
        ),
    };

    EvaluatedCheck {
        severity,
        view: DoctorCheckView {
            name: "signer".to_owned(),
            status: severity.status().to_owned(),
            detail,
        },
        action,
    }
}

fn myc_check(myc: &crate::domain::runtime::MycStatusView) -> EvaluatedCheck {
    let (severity, detail, action) = match myc.state.as_str() {
        "ready" => (
            DoctorSeverity::Ok,
            myc.service_status
                .clone()
                .unwrap_or_else(|| "service ready".to_owned()),
            None,
        ),
        "unconfigured" => (
            DoctorSeverity::Warn,
            myc.reason
                .clone()
                .unwrap_or_else(|| "myc is not configured".to_owned()),
            Some("radroots myc status"),
        ),
        _ => (
            DoctorSeverity::ExternalFail,
            myc.reason
                .clone()
                .unwrap_or_else(|| "myc is unavailable".to_owned()),
            Some("radroots myc status"),
        ),
    };

    EvaluatedCheck {
        severity,
        view: DoctorCheckView {
            name: "myc".to_owned(),
            status: severity.status().to_owned(),
            detail,
        },
        action,
    }
}

fn logging_check(config: &RuntimeConfig, logging: &LoggingState) -> EvaluatedCheck {
    let detail = match (config.logging.stdout, logging.current_file.as_ref()) {
        (true, Some(path)) => format!("stdout + file {}", path.display()),
        (true, None) => "stdout only".to_owned(),
        (false, Some(path)) => format!("file {}", path.display()),
        (false, None) => "stdout off · no file sink".to_owned(),
    };

    EvaluatedCheck {
        severity: DoctorSeverity::Ok,
        view: DoctorCheckView {
            name: "logging".to_owned(),
            status: "ok".to_owned(),
            detail,
        },
        action: None,
    }
}

fn collect_actions(checks: &[EvaluatedCheck]) -> Vec<String> {
    let mut actions = Vec::new();
    for action in checks.iter().filter_map(|check| check.action) {
        if !actions.iter().any(|existing| existing == action) {
            actions.push(action.to_owned());
        }
    }
    actions
}
