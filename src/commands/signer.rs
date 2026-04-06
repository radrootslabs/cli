use crate::domain::runtime::SignerStatusView;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::signer::resolve_signer_status;

pub fn status(config: &RuntimeConfig) -> SignerStatusView {
    resolve_signer_status(config)
}
