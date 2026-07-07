use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Clone, Args)]
pub struct MeshArgs {
    #[command(subcommand)]
    pub command: MeshCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MeshCommand {
    Scope(MeshScopeArgs),
    Status,
    Policy(MeshPolicyArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MeshScopeArgs {
    #[command(subcommand)]
    pub command: MeshScopeCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MeshScopeCommand {
    Get,
    Set(MeshScopeSetArgs),
}

#[derive(Debug, Clone, Args)]
pub struct MeshScopeSetArgs {
    #[arg(long = "scope", value_enum)]
    pub scope: MeshScopeArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MeshScopeArg {
    Disabled,
    LocalPreview,
}

impl MeshScopeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::LocalPreview => "local_preview",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct MeshPolicyArgs {
    #[command(subcommand)]
    pub command: MeshPolicyCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum MeshPolicyCommand {
    Check,
}
