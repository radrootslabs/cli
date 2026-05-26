use crate::cli::{TargetCliArgs, TargetOutputFormat};
use crate::out::envelope::{EnvelopeActor, EnvelopeContext, OutputFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutputFormat {
    Human,
    Json,
    Ndjson,
}

impl Default for OperationOutputFormat {
    fn default() -> Self {
        Self::Human
    }
}

impl From<TargetOutputFormat> for OperationOutputFormat {
    fn from(format: TargetOutputFormat) -> Self {
        match format {
            TargetOutputFormat::Human => Self::Human,
            TargetOutputFormat::Json => Self::Json,
            TargetOutputFormat::Ndjson => Self::Ndjson,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationNetworkMode {
    Default,
    Offline,
    Online,
}

impl Default for OperationNetworkMode {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationInputMode {
    PromptingAllowed,
    NoInput,
}

impl Default for OperationInputMode {
    fn default() -> Self {
        Self::PromptingAllowed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationContext {
    pub output_format: OperationOutputFormat,
    pub account_id: Option<String>,
    pub relays: Vec<String>,
    pub network_mode: OperationNetworkMode,
    pub dry_run: bool,
    pub idempotency_key: Option<String>,
    pub correlation_id: Option<String>,
    pub approval_token: Option<String>,
    pub input_mode: OperationInputMode,
    pub quiet: bool,
    pub verbose: bool,
    pub trace: bool,
    pub color: bool,
}

impl OperationContext {
    pub fn from_target_args(args: &TargetCliArgs) -> Self {
        Self {
            output_format: OperationOutputFormat::from(args.format),
            account_id: args.account_id.clone(),
            relays: args.relay.clone(),
            network_mode: if args.offline {
                OperationNetworkMode::Offline
            } else if args.online {
                OperationNetworkMode::Online
            } else {
                OperationNetworkMode::Default
            },
            dry_run: args.dry_run,
            idempotency_key: args.idempotency_key.clone(),
            correlation_id: args.correlation_id.clone(),
            approval_token: args.approval_token.clone(),
            input_mode: if args.no_input {
                OperationInputMode::NoInput
            } else {
                OperationInputMode::PromptingAllowed
            },
            quiet: args.quiet,
            verbose: args.verbose,
            trace: args.trace,
            color: !args.no_color,
        }
    }

    pub fn envelope_context(&self, request_id: impl Into<String>) -> EnvelopeContext {
        let mut context = EnvelopeContext::new(request_id, self.dry_run);
        context.output_format = match self.output_format {
            OperationOutputFormat::Human => OutputFormat::Human,
            OperationOutputFormat::Json => OutputFormat::Json,
            OperationOutputFormat::Ndjson => OutputFormat::Ndjson,
        };
        context.correlation_id = self.correlation_id.clone();
        context.idempotency_key = self.idempotency_key.clone();
        context.actor = self.account_id.as_ref().map(|account_id| EnvelopeActor {
            account_id: account_id.clone(),
            role: "account".to_owned(),
        });
        context
    }

    pub fn requires_approval_token(&self) -> bool {
        !self.dry_run && !self.has_approval_token()
    }

    pub fn has_approval_token(&self) -> bool {
        self.approval_token
            .as_deref()
            .is_some_and(|token| !token.trim().is_empty())
    }
}
