use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, SignerSessionActionView, SignerStatusView,
};
use crate::runtime::config::RuntimeConfig;
use crate::runtime::daemon::DaemonRpcError;
use crate::runtime::signer::resolve_signer_status;

pub fn status(config: &RuntimeConfig) -> CommandOutput {
    let view: SignerStatusView = resolve_signer_status(config);
    match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::SignerStatus(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::SignerStatus(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::SignerStatus(view))
        }
        CommandDisposition::Unsupported => {
            CommandOutput::unsupported(CommandView::SignerStatus(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::SignerStatus(view))
        }
    }
}

pub fn session_list(config: &RuntimeConfig) -> CommandOutput {
    crate::runtime::daemon::signer_sessions(config)
}

pub fn session_show(config: &RuntimeConfig, session_id: &str) -> CommandOutput {
    session_action_output(
        "show",
        crate::runtime::daemon::signer_session_show(config, session_id),
    )
}

pub fn session_connect_bunker(config: &RuntimeConfig, url: &str) -> CommandOutput {
    session_action_output(
        "connect_bunker",
        crate::runtime::daemon::signer_session_connect_bunker(config, url),
    )
}

pub fn session_connect_nostrconnect(
    config: &RuntimeConfig,
    url: &str,
    client_secret_key: &str,
) -> CommandOutput {
    session_action_output(
        "connect_nostrconnect",
        crate::runtime::daemon::signer_session_connect_nostrconnect(config, url, client_secret_key),
    )
}

pub fn session_public_key(config: &RuntimeConfig, session_id: &str) -> CommandOutput {
    session_action_output(
        "public_key",
        crate::runtime::daemon::signer_session_public_key(config, session_id),
    )
}

pub fn session_authorize(config: &RuntimeConfig, session_id: &str) -> CommandOutput {
    session_action_output(
        "authorize",
        crate::runtime::daemon::signer_session_authorize(config, session_id),
    )
}

pub fn session_require_auth(
    config: &RuntimeConfig,
    session_id: &str,
    auth_url: &str,
) -> CommandOutput {
    session_action_output(
        "require_auth",
        crate::runtime::daemon::signer_session_require_auth(config, session_id, auth_url),
    )
}

pub fn session_close(config: &RuntimeConfig, session_id: &str) -> CommandOutput {
    session_action_output(
        "close",
        crate::runtime::daemon::signer_session_close(config, session_id),
    )
}

fn session_action_output(
    action: &str,
    result: Result<SignerSessionActionView, DaemonRpcError>,
) -> CommandOutput {
    match result {
        Ok(view) => CommandOutput::success(CommandView::SignerSessionAction(view)),
        Err(error) => {
            let (disposition, view) = session_action_error_view(action, error);
            match disposition {
                CommandDisposition::Unconfigured => {
                    CommandOutput::unconfigured(CommandView::SignerSessionAction(view))
                }
                CommandDisposition::ExternalUnavailable => {
                    CommandOutput::external_unavailable(CommandView::SignerSessionAction(view))
                }
                CommandDisposition::Unsupported => {
                    CommandOutput::unsupported(CommandView::SignerSessionAction(view))
                }
                CommandDisposition::InternalError => {
                    CommandOutput::internal_error(CommandView::SignerSessionAction(view))
                }
                CommandDisposition::Success => {
                    CommandOutput::success(CommandView::SignerSessionAction(view))
                }
            }
        }
    }
}

fn session_action_error_view(
    action: &str,
    error: DaemonRpcError,
) -> (CommandDisposition, SignerSessionActionView) {
    let (disposition, state, reason) = match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => {
            (CommandDisposition::Unconfigured, "unconfigured", reason)
        }
        DaemonRpcError::External(reason) => (
            CommandDisposition::ExternalUnavailable,
            "unavailable",
            reason,
        ),
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => {
            (CommandDisposition::InternalError, "error", reason)
        }
    };
    (
        disposition,
        SignerSessionActionView {
            action: action.to_owned(),
            state: state.to_owned(),
            source: "daemon signer session rpc · durable write plane".to_owned(),
            session_id: None,
            mode: None,
            remote_signer_pubkey: None,
            client_pubkey: None,
            signer_pubkey: None,
            user_pubkey: None,
            relays: Vec::new(),
            permissions: Vec::new(),
            auth_required: None,
            authorized: None,
            auth_url: None,
            expires_in_secs: None,
            pubkey: None,
            replayed: None,
            required: None,
            closed: None,
            reason: Some(reason),
        },
    )
}
