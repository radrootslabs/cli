use clap::{Args, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct HealthArgs {
    #[command(subcommand)]
    pub command: HealthCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum HealthCommand {
    Status(HealthStatusArgs),
    Check(HealthCheckArgs),
}

#[derive(Debug, Clone, Args)]
pub struct HealthStatusArgs {
    #[command(subcommand)]
    pub command: HealthStatusCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HealthStatusCommand {
    Get,
}

#[derive(Debug, Clone, Args)]
pub struct HealthCheckArgs {
    #[command(subcommand)]
    pub command: HealthCheckCommand,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum HealthCheckCommand {
    Run,
}
