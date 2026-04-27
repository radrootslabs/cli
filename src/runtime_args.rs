use std::path::PathBuf;

use crate::runtime::config::OutputFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOutputFormatArg {
    Human,
    Json,
    Ndjson,
}

impl RuntimeOutputFormatArg {
    pub fn as_output_format(self) -> OutputFormat {
        match self {
            Self::Human => OutputFormat::Human,
            Self::Json => OutputFormat::Json,
            Self::Ndjson => OutputFormat::Ndjson,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeInvocationArgs {
    pub output_format: Option<RuntimeOutputFormatArg>,
    pub json: bool,
    pub ndjson: bool,
    pub env_file: Option<PathBuf>,
    pub quiet: bool,
    pub verbose: bool,
    pub trace: bool,
    pub dry_run: bool,
    pub no_color: bool,
    pub no_input: bool,
    pub yes: bool,
    pub log_filter: Option<String>,
    pub log_dir: Option<PathBuf>,
    pub log_stdout: bool,
    pub no_log_stdout: bool,
    pub account: Option<String>,
    pub identity_path: Option<PathBuf>,
    pub signer: Option<String>,
    pub relay: Vec<String>,
    pub myc_executable: Option<PathBuf>,
    pub myc_status_timeout_ms: Option<u64>,
    pub hyf_enabled: bool,
    pub no_hyf_enabled: bool,
    pub hyf_executable: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub enum LocalExportFormatArg {
    Json,
    Ndjson,
}

impl LocalExportFormatArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Ndjson => "ndjson",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncWatchArgs {
    pub frames: usize,
    pub interval_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FindQueryArgs {
    pub query: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FarmScopeArg {
    User,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FarmFieldArg {
    Name,
    DisplayName,
    About,
    Website,
    Picture,
    Banner,
    Location,
    City,
    Region,
    Country,
    Delivery,
}

#[derive(Debug, Clone, Default)]
pub struct FarmScopedArgs {
    pub scope: Option<FarmScopeArg>,
}

#[derive(Debug, Clone, Default)]
pub struct FarmCreateArgs {
    pub scope: Option<FarmScopeArg>,
    pub farm_d_tag: Option<String>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub website: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub location: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub delivery_method: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FarmUpdateArgs {
    pub scope: Option<FarmScopeArg>,
    pub field: FarmFieldArg,
    pub value: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FarmPublishArgs {
    pub scope: Option<FarmScopeArg>,
    pub idempotency_key: Option<String>,
    pub signer_session_id: Option<String>,
    pub print_event: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ListingCreateArgs {
    pub output: Option<PathBuf>,
    pub key: Option<String>,
    pub title: Option<String>,
    pub category: Option<String>,
    pub summary: Option<String>,
    pub bin_id: Option<String>,
    pub quantity_amount: Option<String>,
    pub quantity_unit: Option<String>,
    pub price_amount: Option<String>,
    pub price_currency: Option<String>,
    pub price_per_amount: Option<String>,
    pub price_per_unit: Option<String>,
    pub available: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ListingFileArgs {
    pub file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ListingMutationArgs {
    pub file: PathBuf,
    pub idempotency_key: Option<String>,
    pub signer_session_id: Option<String>,
    pub print_event: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OrderDraftCreateArgs {
    pub listing: Option<String>,
    pub listing_addr: Option<String>,
    pub bin_id: Option<String>,
    pub bin_count: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct OrderSubmitArgs {
    pub key: String,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OrderWatchArgs {
    pub key: String,
    pub frames: Option<usize>,
    pub interval_ms: u64,
}

#[derive(Debug, Clone)]
pub struct RecordLookupArgs {
    pub key: String,
}
