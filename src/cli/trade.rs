use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Clone, Args)]
pub struct TradeArgs {
    #[command(subcommand)]
    pub command: TradeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeCommand {
    Proposal(TradeProposalArgs),
    Revision(TradeRevisionArgs),
    Candidate(TradeCandidateArgs),
    Cancellation(TradeCancellationArgs),
    Operation(TradeOperationArgs),
    Get(TradeKeyArgs),
    List(TradeListArgs),
    Evidence(TradeEvidenceArgs),
    PrivateArtifact(TradePrivateArtifactArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeProposalArgs {
    #[command(subcommand)]
    pub command: TradeProposalCommand,
}

#[derive(Debug, Clone, Args)]
pub struct TradeRevisionArgs {
    #[command(subcommand)]
    pub command: TradeRevisionCommand,
}

#[derive(Debug, Clone, Args)]
pub struct TradeCandidateArgs {
    #[command(subcommand)]
    pub command: TradeCandidateCommand,
}

#[derive(Debug, Clone, Args)]
pub struct TradeCancellationArgs {
    #[command(subcommand)]
    pub command: TradeCancellationCommand,
}

#[derive(Debug, Clone, Args)]
pub struct TradeOperationArgs {
    #[command(subcommand)]
    pub command: TradeOperationCommand,
}

#[derive(Debug, Clone, Args)]
pub struct TradeEvidenceArgs {
    #[command(subcommand)]
    pub command: TradeEvidenceCommand,
}

#[derive(Debug, Clone, Args)]
pub struct TradePrivateArtifactArgs {
    #[command(subcommand)]
    pub command: TradePrivateArtifactCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeProposalCommand {
    Submit(TradeEnvelopeFileArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeRevisionCommand {
    Propose(TradeEnvelopeFileArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeCandidateCommand {
    Decide(TradeDecisionEnvelopeArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeCancellationCommand {
    Submit(TradeEnvelopeFileArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeOperationCommand {
    Resume(TradeResumeArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradeEvidenceCommand {
    Refresh(TradeKeyArgs),
    Inspect(TradeEvidenceInspectArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TradePrivateArtifactCommand {
    Seal(TradePrivateArtifactSealArgs),
    Open(TradePrivateArtifactOpenArgs),
    Delete(TradePrivateArtifactDeleteArgs),
}

#[derive(Debug, Clone, Args)]
pub struct TradeEnvelopeFileArgs {
    pub file: PathBuf,
}

#[derive(Debug, Clone, Args)]
pub struct TradeDecisionEnvelopeArgs {
    pub file: PathBuf,
    #[arg(long = "acknowledge-private-terms")]
    pub acknowledge_private_terms: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TradeResumeArgs {
    pub file: PathBuf,
    #[arg(long = "operation-kind", value_enum)]
    pub operation_kind: Option<TradeResumeOperationKindArg>,
    #[arg(long = "acknowledge-private-terms")]
    pub acknowledge_private_terms: bool,
}

#[derive(Debug, Clone, Args)]
pub struct TradeKeyArgs {
    pub trade_id: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeListArgs {
    #[arg(long)]
    pub limit: Option<u32>,
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradeEvidenceInspectArgs {
    pub trade_id: Option<String>,
    #[arg(long)]
    pub limit: Option<u32>,
    #[arg(long)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct TradePrivateArtifactSealArgs {
    pub trade_id: Option<String>,
    #[arg(long = "artifact-id")]
    pub artifact_id: Option<String>,
    #[arg(long = "schema-id")]
    pub schema_id: Option<String>,
    #[arg(long)]
    pub input: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = TradePrivateArtifactKindArg::BindingTerms)]
    pub kind: TradePrivateArtifactKindArg,
    #[arg(long = "candidate-id")]
    pub candidate_id: Option<String>,
    #[arg(long = "retention-class")]
    pub retention_class: Option<String>,
    #[arg(long = "expires-at-ms")]
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Args)]
pub struct TradePrivateArtifactOpenArgs {
    pub artifact_id: String,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct TradePrivateArtifactDeleteArgs {
    pub artifact_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TradeResumeOperationKindArg {
    SubmitProposal,
    ProposeRevision,
    DecideCandidate,
    Cancel,
}

impl TradeResumeOperationKindArg {
    pub fn as_sdk_operation_kind(self) -> &'static str {
        match self {
            Self::SubmitProposal => "trade.submit_proposal.v1",
            Self::ProposeRevision => "trade.propose_revision.v1",
            Self::DecideCandidate => "trade.decide_candidate.v1",
            Self::Cancel => "trade.cancel.v1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TradePrivateArtifactKindArg {
    BindingTerms,
    Message,
    ContactBundle,
    DeliveryInstruction,
}

impl TradePrivateArtifactKindArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BindingTerms => "binding_terms",
            Self::Message => "message",
            Self::ContactBundle => "contact_bundle",
            Self::DeliveryInstruction => "delivery_instruction",
        }
    }
}
