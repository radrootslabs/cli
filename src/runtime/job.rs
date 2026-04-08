use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::cli::JobWatchArgs;
use crate::domain::runtime::{
    CommandOutput, CommandView, JobGetView, JobListView, JobWatchFrameView, JobWatchView,
};
use crate::runtime::RuntimeError;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::daemon::{self, DaemonRpcError};

pub fn list(config: &RuntimeConfig) -> CommandOutput {
    match daemon::bridge_job_list(config) {
        Ok(jobs) => CommandOutput::success(CommandView::JobList(JobListView {
            state: if jobs.is_empty() {
                "empty".to_owned()
            } else {
                "ready".to_owned()
            },
            source: daemon::bridge_source().to_owned(),
            rpc_url: config.rpc.url.clone(),
            count: jobs.len(),
            reason: None,
            jobs,
            actions: Vec::new(),
        })),
        Err(error) => error_job_list_view(config, error),
    }
}

pub fn get(config: &RuntimeConfig, job_id: &str) -> CommandOutput {
    match daemon::bridge_job(config, job_id) {
        Ok(Some(job)) => CommandOutput::success(CommandView::JobGet(JobGetView {
            state: "ready".to_owned(),
            source: daemon::bridge_source().to_owned(),
            rpc_url: config.rpc.url.clone(),
            lookup: job_id.to_owned(),
            reason: None,
            job: Some(job),
            actions: Vec::new(),
        })),
        Ok(None) => CommandOutput::success(CommandView::JobGet(JobGetView {
            state: "missing".to_owned(),
            source: daemon::bridge_source().to_owned(),
            rpc_url: config.rpc.url.clone(),
            lookup: job_id.to_owned(),
            reason: Some(format!("job `{job_id}` was not found in radrootsd")),
            job: None,
            actions: vec!["radroots job ls".to_owned()],
        })),
        Err(error) => error_job_get_view(config, job_id, error),
    }
}

pub fn watch(config: &RuntimeConfig, args: &JobWatchArgs) -> Result<CommandOutput, RuntimeError> {
    if args.frames == Some(0) {
        return Err(RuntimeError::Config(
            "--frames must be greater than zero when provided".to_owned(),
        ));
    }

    let mut frames = Vec::new();
    let max_frames = args.frames.unwrap_or(usize::MAX);
    loop {
        match daemon::bridge_job(config, args.key.as_str()) {
            Ok(Some(job)) => {
                frames.push(JobWatchFrameView {
                    sequence: frames.len() + 1,
                    observed_at_unix: job.completed_at_unix.unwrap_or(job.requested_at_unix),
                    state: job.state.clone(),
                    terminal: job.terminal,
                    signer: job.signer.clone(),
                    signer_session_id: job.signer_session_id.clone(),
                    summary: job.relay_outcome_summary.clone(),
                });
                if job.terminal || frames.len() >= max_frames {
                    let state = if job.terminal {
                        job.state
                    } else {
                        "watching".to_owned()
                    };
                    return Ok(CommandOutput::success(CommandView::JobWatch(
                        JobWatchView {
                            state,
                            source: daemon::bridge_source().to_owned(),
                            rpc_url: config.rpc.url.clone(),
                            job_id: args.key.clone(),
                            interval_ms: args.interval_ms,
                            reason: None,
                            frames,
                            actions: Vec::new(),
                        },
                    )));
                }
            }
            Ok(None) => {
                return Ok(CommandOutput::success(CommandView::JobWatch(
                    JobWatchView {
                        state: "missing".to_owned(),
                        source: daemon::bridge_source().to_owned(),
                        rpc_url: config.rpc.url.clone(),
                        job_id: args.key.clone(),
                        interval_ms: args.interval_ms,
                        reason: Some(format!("job `{}` was not found in radrootsd", args.key)),
                        frames,
                        actions: vec!["radroots job ls".to_owned()],
                    },
                )));
            }
            Err(error) => {
                return Ok(error_job_watch_view(config, args, frames, error));
            }
        }

        thread::sleep(Duration::from_millis(args.interval_ms));
    }
}

pub fn format_timestamp(unix: u64) -> String {
    DateTime::<Utc>::from_timestamp(unix as i64, 0)
        .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| unix.to_string())
}

pub fn format_clock(unix: u64) -> String {
    DateTime::<Utc>::from_timestamp(unix as i64, 0)
        .map(|value| value.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| unix.to_string())
}

fn error_job_list_view(config: &RuntimeConfig, error: DaemonRpcError) -> CommandOutput {
    match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => {
            CommandOutput::unconfigured(CommandView::JobList(JobListView {
                state: "unconfigured".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                count: 0,
                reason: Some(reason),
                jobs: Vec::new(),
                actions: vec![
                    "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                    "start radrootsd with bridge ingress enabled".to_owned(),
                ],
            }))
        }
        DaemonRpcError::External(reason) => {
            CommandOutput::external_unavailable(CommandView::JobList(JobListView {
                state: "unavailable".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                count: 0,
                reason: Some(reason),
                jobs: Vec::new(),
                actions: vec!["start radrootsd and verify the rpc url".to_owned()],
            }))
        }
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => {
            CommandOutput::internal_error(CommandView::JobList(JobListView {
                state: "error".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                count: 0,
                reason: Some(reason),
                jobs: Vec::new(),
                actions: Vec::new(),
            }))
        }
    }
}

fn error_job_get_view(
    config: &RuntimeConfig,
    job_id: &str,
    error: DaemonRpcError,
) -> CommandOutput {
    match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => {
            CommandOutput::unconfigured(CommandView::JobGet(JobGetView {
                state: "unconfigured".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                lookup: job_id.to_owned(),
                reason: Some(reason),
                job: None,
                actions: vec![
                    "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                    "start radrootsd with bridge ingress enabled".to_owned(),
                ],
            }))
        }
        DaemonRpcError::External(reason) => {
            CommandOutput::external_unavailable(CommandView::JobGet(JobGetView {
                state: "unavailable".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                lookup: job_id.to_owned(),
                reason: Some(reason),
                job: None,
                actions: vec!["start radrootsd and verify the rpc url".to_owned()],
            }))
        }
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => {
            CommandOutput::internal_error(CommandView::JobGet(JobGetView {
                state: "error".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                lookup: job_id.to_owned(),
                reason: Some(reason),
                job: None,
                actions: Vec::new(),
            }))
        }
    }
}

fn error_job_watch_view(
    config: &RuntimeConfig,
    args: &JobWatchArgs,
    frames: Vec<JobWatchFrameView>,
    error: DaemonRpcError,
) -> CommandOutput {
    match error {
        DaemonRpcError::Unconfigured(reason)
        | DaemonRpcError::Unauthorized(reason)
        | DaemonRpcError::MethodUnavailable(reason) => {
            CommandOutput::unconfigured(CommandView::JobWatch(JobWatchView {
                state: "unconfigured".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                job_id: args.key.clone(),
                interval_ms: args.interval_ms,
                reason: Some(reason),
                frames,
                actions: vec![
                    "set RADROOTS_RPC_BEARER_TOKEN in .env or your shell".to_owned(),
                    "start radrootsd with bridge ingress enabled".to_owned(),
                ],
            }))
        }
        DaemonRpcError::External(reason) => {
            CommandOutput::external_unavailable(CommandView::JobWatch(JobWatchView {
                state: "unavailable".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                job_id: args.key.clone(),
                interval_ms: args.interval_ms,
                reason: Some(reason),
                frames,
                actions: vec!["start radrootsd and verify the rpc url".to_owned()],
            }))
        }
        DaemonRpcError::InvalidResponse(reason)
        | DaemonRpcError::Remote(reason)
        | DaemonRpcError::UnknownJob(reason) => {
            CommandOutput::internal_error(CommandView::JobWatch(JobWatchView {
                state: "error".to_owned(),
                source: daemon::bridge_source().to_owned(),
                rpc_url: config.rpc.url.clone(),
                job_id: args.key.clone(),
                interval_ms: args.interval_ms,
                reason: Some(reason),
                frames,
                actions: Vec::new(),
            }))
        }
    }
}
