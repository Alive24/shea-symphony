use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::runtime_state::{
    detect_runtime_stall, RuntimeRetryState, RuntimeStallState, RuntimeState,
};
use jade_symphony::tracker::TrackerAdapter;

use crate::compact_evidence;

use super::append_runtime_supervision_event;

mod archive;
mod recovery;
mod session_probe;

use archive::{archive_runtime_state, stale_runtime_state_action};
use recovery::{
    active_runtime_session_for_issue, recover_registered_main_sessions, runtime_recovery_reason,
    terminal_runtime_session_for_issue,
};
use session_probe::session_status_counts_as_active_worker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResumePreflightAction {
    Continue,
    ArchiveStale {
        issue_identifier: String,
        tracker_state: String,
        archive_reason: String,
    },
    RetryLater {
        issue_identifier: String,
        retry: RuntimeRetryState,
        due_in_ms: u64,
    },
    Stalled {
        issue_identifier: String,
        stall: RuntimeStallState,
    },
    Block {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RuntimePreflightSummary {
    pub(crate) retained_states: Vec<RuntimeState>,
    pub(crate) active_main_workers: usize,
    pub(crate) recoverable_states: Vec<RuntimeRecoveryCandidate>,
    pub(crate) blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RuntimeRecoveryCandidate {
    pub(crate) state: RuntimeState,
    pub(crate) issue: TrackerIssue,
    pub(crate) reason: String,
}

pub(crate) fn run_loop_resume_preflight(
    adapter: &dyn TrackerAdapter,
    config: &RuntimeConfig,
    state: Option<&RuntimeState>,
    now_ms: u64,
) -> Result<ResumePreflightAction, Box<dyn std::error::Error>> {
    let Some(state) = state else {
        return Ok(ResumePreflightAction::Continue);
    };
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(ResumePreflightAction::Continue);
    };

    let Some(issue) = adapter.get_issue(&active_issue.identifier)? else {
        return Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references missing issue {}",
                active_issue.identifier
            ),
        });
    };
    let normalized_state = normalize_state(&issue.state);

    if normalized_state != "in progress" {
        return stale_runtime_state_action(state, &issue, &normalized_state, config);
    }

    if let Some(retry) = state.retry.clone() {
        let due_in_ms = retry.due_in_ms(now_ms);
        if due_in_ms > 0 {
            return Ok(ResumePreflightAction::RetryLater {
                issue_identifier: active_issue.identifier.clone(),
                retry,
                due_in_ms,
            });
        }
        return Ok(ResumePreflightAction::Continue);
    }

    if let Some(stall) = detect_runtime_stall(state, now_ms, config.codex.stall_timeout_ms) {
        return Ok(ResumePreflightAction::Stalled {
            issue_identifier: active_issue.identifier.clone(),
            stall,
        });
    }

    Ok(ResumePreflightAction::Continue)
}

pub(crate) fn run_loop_resume_preflight_many(
    adapter: &dyn TrackerAdapter,
    config: &RuntimeConfig,
    states: &[RuntimeState],
    now_ms: u64,
    recover: bool,
) -> Result<RuntimePreflightSummary, Box<dyn std::error::Error>> {
    let mut retained_states = Vec::new();
    let mut active_main_workers = 0usize;
    let mut recoverable_states = Vec::new();
    let mut blocked = None;

    for state in states {
        if !runtime_state_is_main_lane(state) {
            retained_states.push(state.clone());
            continue;
        }

        if recover {
            let in_progress_issue = match runtime_state_in_progress_issue(adapter, state) {
                Ok(issue) => issue,
                Err(error) => {
                    let issue_identifier = state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.as_str())
                        .unwrap_or("unknown");
                    let reason = format!(
                        "tracker_read_failed: {}",
                        compact_evidence(&error.to_string())
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(state),
                        "RuntimeRecoverReadSkipped",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recover_read_skipped issue={} reason={}",
                        issue_identifier, reason
                    );
                    retained_states.push(state.clone());
                    continue;
                }
            };
            if let Some(issue) = in_progress_issue {
                if let Some(active_session) =
                    active_runtime_session_for_issue(config, state, now_ms)?
                {
                    if session_status_counts_as_active_worker(&active_session.probe.status) {
                        let issue_identifier = state
                            .active_issue
                            .as_ref()
                            .map(|issue| issue.identifier.as_str())
                            .unwrap_or("unknown");
                        active_main_workers += 1;
                        append_runtime_supervision_event(
                            config,
                            Some(state),
                            "RuntimeRecoverSkippedActiveSession",
                            &format!(
                                "issue={issue_identifier} session={} status={} source={} evidence={}",
                                active_session.session_name,
                                active_session.probe.status.as_str(),
                                active_session.probe.source.as_str(),
                                compact_evidence(&active_session.probe.evidence)
                            ),
                        )?;
                        println!(
                            "run_loop_resume_preflight action=active_session issue={} session={} status={} source={} evidence={}",
                            issue_identifier,
                            active_session.session_name,
                            active_session.probe.status.as_str(),
                            active_session.probe.source.as_str(),
                            compact_evidence(&active_session.probe.evidence)
                        );
                        retained_states.push(active_session.state);
                        continue;
                    }
                }
                if let Some(terminal_session) =
                    terminal_runtime_session_for_issue(config, state, now_ms)?
                {
                    let issue_identifier = state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.as_str())
                        .unwrap_or("unknown");
                    let reason = format!(
                        "registry_session_terminal session={} status={} source={} evidence={}",
                        terminal_session.session_name,
                        terminal_session.probe.status.as_str(),
                        terminal_session.probe.source.as_str(),
                        compact_evidence(&terminal_session.probe.evidence)
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(&terminal_session.state),
                        "RuntimeRecoverableTerminalSession",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recoverable_terminal_session issue={} session={} status={} source={}",
                        issue_identifier,
                        terminal_session.session_name,
                        terminal_session.probe.status.as_str(),
                        terminal_session.probe.source.as_str()
                    );
                    recoverable_states.push(RuntimeRecoveryCandidate {
                        state: terminal_session.state.clone(),
                        issue,
                        reason,
                    });
                    retained_states.push(terminal_session.state);
                    continue;
                }
                if let Some(reason) = runtime_recovery_reason(config, state, now_ms)? {
                    let issue_identifier = state
                        .active_issue
                        .as_ref()
                        .map(|issue| issue.identifier.as_str())
                        .unwrap_or("unknown");
                    append_runtime_supervision_event(
                        config,
                        Some(state),
                        "RuntimeRecoverable",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recoverable issue={} reason={}",
                        issue_identifier, reason
                    );
                    recoverable_states.push(RuntimeRecoveryCandidate {
                        state: state.clone(),
                        issue,
                        reason,
                    });
                    retained_states.push(state.clone());
                    continue;
                }
            }
        }

        let action = run_loop_resume_preflight(adapter, config, Some(state), now_ms)?;
        match action {
            ResumePreflightAction::Continue => {
                if runtime_state_points_at_in_progress_issue(adapter, state)? {
                    active_main_workers += 1;
                }
                retained_states.push(state.clone());
            }
            ResumePreflightAction::ArchiveStale {
                issue_identifier,
                tracker_state,
                archive_reason,
            } => {
                let archive_path = archive_runtime_state(config, state, &archive_reason)?;
                append_runtime_supervision_event(
                    config,
                    Some(state),
                    "RuntimeStateArchived",
                    &format!(
                        "issue={issue_identifier} tracker_state={tracker_state} reason={archive_reason} archive_path={}",
                        archive_path.display()
                    ),
                )?;
                println!(
                    "run_loop_resume_preflight action=archive issue={} tracker_state={:?} reason={} archive_path={}",
                    issue_identifier,
                    tracker_state,
                    archive_reason,
                    archive_path.display()
                );
            }
            ResumePreflightAction::RetryLater {
                issue_identifier,
                retry,
                due_in_ms,
            } => {
                active_main_workers += 1;
                append_runtime_supervision_event(
                    config,
                    Some(state),
                    "RetryDeferred",
                    &format!(
                        "issue={issue_identifier} attempt={} due_in_ms={} error={}",
                        retry.attempt, due_in_ms, retry.error
                    ),
                )?;
                println!(
                    "run_loop_resume_preflight action=retry_backoff issue={} due_in_ms={} attempt={}",
                    issue_identifier, due_in_ms, retry.attempt
                );
                retained_states.push(state.clone());
            }
            ResumePreflightAction::Stalled {
                issue_identifier,
                stall,
            } => {
                if recover {
                    let Some(issue) = runtime_state_in_progress_issue(adapter, state)? else {
                        retained_states.push(state.clone());
                        continue;
                    };
                    let reason = format!(
                        "runtime_stalled stalled_for_ms={} reason={}",
                        stall.stalled_for_ms, stall.reason
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(state),
                        "RuntimeRecoverable",
                        &format!("issue={issue_identifier} reason={reason}"),
                    )?;
                    println!(
                        "run_loop_resume_preflight action=recoverable issue={} stalled_for_ms={}",
                        issue_identifier, stall.stalled_for_ms
                    );
                    recoverable_states.push(RuntimeRecoveryCandidate {
                        state: state.clone(),
                        issue,
                        reason,
                    });
                    retained_states.push(state.clone());
                    continue;
                }
                active_main_workers += 1;
                append_runtime_supervision_event(
                    config,
                    Some(state),
                    "RuntimeStalled",
                    &format!(
                        "issue={issue_identifier} stalled_for_ms={} reason={}",
                        stall.stalled_for_ms, stall.reason
                    ),
                )?;
                let reason = format!(
                    "runtime_stalled issue={} stalled_for_ms={}",
                    issue_identifier, stall.stalled_for_ms
                );
                println!("run_loop_resume_preflight action={reason}");
                retained_states.push(state.clone());
                blocked.get_or_insert(reason);
            }
            ResumePreflightAction::Block { reason } => {
                append_runtime_supervision_event(config, Some(state), "ResumeBlocked", &reason)?;
                retained_states.push(state.clone());
                blocked.get_or_insert(reason);
            }
        }
    }

    if recover {
        recover_registered_main_sessions(
            adapter,
            config,
            now_ms,
            &mut retained_states,
            &mut active_main_workers,
            &mut recoverable_states,
        )?;
    }

    Ok(RuntimePreflightSummary {
        retained_states,
        active_main_workers,
        recoverable_states,
        blocked,
    })
}

pub(crate) fn runtime_state_issue_identifier(state: &RuntimeState) -> Option<&str> {
    state
        .active_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
}

fn runtime_state_is_main_lane(state: &RuntimeState) -> bool {
    state
        .lane
        .as_deref()
        .map(|lane| lane.eq_ignore_ascii_case("main"))
        .unwrap_or(true)
}

fn runtime_state_points_at_in_progress_issue(
    adapter: &dyn TrackerAdapter,
    state: &RuntimeState,
) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(runtime_state_in_progress_issue(adapter, state)?.is_some())
}

fn runtime_state_in_progress_issue(
    adapter: &dyn TrackerAdapter,
    state: &RuntimeState,
) -> Result<Option<TrackerIssue>, Box<dyn std::error::Error>> {
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(None);
    };
    let Some(issue) = adapter.get_issue(&active_issue.identifier)? else {
        return Ok(None);
    };
    Ok((normalize_state(&issue.state) == "in progress").then_some(issue))
}
