use serde::Serialize;

#[derive(Debug, Clone)]
pub enum CommandOutput {
    RuntimeShow(RuntimeShowView),
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeShowView {
    pub output_format: String,
    pub logging: LoggingRuntimeView,
    pub identity: IdentityRuntimeView,
    pub signer: SignerRuntimeView,
    pub myc: MycRuntimeView,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoggingRuntimeView {
    pub initialized: bool,
    pub filter: String,
    pub stdout: bool,
    pub directory: Option<String>,
    pub current_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityRuntimeView {
    pub path: String,
    pub allow_generate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignerRuntimeView {
    pub backend: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MycRuntimeView {
    pub executable: String,
}
