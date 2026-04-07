use crate::domain::runtime::{
    CommandDisposition, CommandOutput, CommandView, IdentityInitView, IdentityPublicView,
    IdentityShowView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::identity::{initialize_identity, load_identity};
use radroots_identity::IdentityError;

pub fn init(config: &RuntimeConfig) -> Result<IdentityInitView, RuntimeError> {
    let identity = initialize_identity(&config.identity)?;
    Ok(IdentityInitView {
        path: identity.path.display().to_string(),
        created: identity.created,
        public_identity: IdentityPublicView::from_public_identity(&identity.public_identity),
    })
}

pub fn show(config: &RuntimeConfig) -> Result<CommandOutput, RuntimeError> {
    let view = match load_identity(&config.identity) {
        Ok(identity) => IdentityShowView {
            path: identity.path.display().to_string(),
            state: "ready".to_owned(),
            reason: None,
            public_identity: Some(IdentityPublicView::from_public_identity(
                &identity.public_identity,
            )),
        },
        Err(RuntimeError::Identity(IdentityError::NotFound(path))) => IdentityShowView {
            path: path.display().to_string(),
            state: "unconfigured".to_owned(),
            reason: Some(format!(
                "local identity file was not found at {}",
                path.display()
            )),
            public_identity: None,
        },
        Err(error) => return Err(error),
    };

    Ok(match view.disposition() {
        CommandDisposition::Success => CommandOutput::success(CommandView::IdentityShow(view)),
        CommandDisposition::Unconfigured => {
            CommandOutput::unconfigured(CommandView::IdentityShow(view))
        }
        CommandDisposition::ExternalUnavailable => {
            CommandOutput::external_unavailable(CommandView::IdentityShow(view))
        }
        CommandDisposition::InternalError => {
            CommandOutput::internal_error(CommandView::IdentityShow(view))
        }
    })
}
