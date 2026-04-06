use crate::domain::runtime::MycStatusView;
use crate::runtime::config::RuntimeConfig;

pub fn status(config: &RuntimeConfig) -> MycStatusView {
    crate::runtime::myc::resolve_status(&config.myc)
}
