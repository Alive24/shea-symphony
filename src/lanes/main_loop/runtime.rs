use std::io;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::runtime_state::{
    detect_runtime_stall, runtime_state_path, RuntimeIssueState, RuntimeRetryState,
    RuntimeStallState, RuntimeState,
};
use jade_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    session_registry_path, AgentSessionRecord, SessionStatus, SessionStatusProbe,
};
use jade_symphony::tracker::TrackerAdapter;

use crate::{
    append_runtime_supervision_event, compact_evidence, current_time_ms,
    DEFAULT_SESSION_STALE_AFTER_MS, DEFAULT_SESSION_STATUS_LINES,
};

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

#[derive(Debug, Clone, PartialEq)]
struct ActiveRuntimeSessionProbe {
    state: RuntimeState,
    session_name: String,
    probe: SessionStatusProbe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeWorkspaceStatus {
    Absent,
    Clean(PathBuf),
    Dirty(PathBuf),
    Unknown { path: PathBuf, reason: String },
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

fn recover_registered_main_sessions(
    adapter: &dyn TrackerAdapter,
    config: &RuntimeConfig,
    now_ms: u64,
    retained_states: &mut Vec<RuntimeState>,
    active_main_workers: &mut usize,
    recoverable_states: &mut Vec<RuntimeRecoveryCandidate>,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = load_session_registry(&session_registry_path(config))?;
    for record in registry.sessions.iter().rev() {
        if !registered_main_runtime_session(record) {
            continue;
        }
        let Some(issue_identifier) = record.issue_identifier.as_deref() else {
            continue;
        };
        if retained_states
            .iter()
            .any(|state| runtime_state_issue_identifier(state) == Some(issue_identifier))
            || recoverable_states.iter().any(|candidate| {
                runtime_state_issue_identifier(&candidate.state) == Some(issue_identifier)
            })
        {
            continue;
        }

        let state = runtime_state_from_session_record(record);
        match runtime_session_probe_from_record(config, record, now_ms) {
            Ok(probe) if session_status_counts_as_active_worker(&probe.status) => {
                if !recover_registry_issue_allows_active_retention(
                    adapter,
                    config,
                    &state,
                    issue_identifier,
                )? {
                    continue;
                }
                *active_main_workers += 1;
                append_runtime_supervision_event(
                    config,
                    Some(&state),
                    "RuntimeRecoverSkippedActiveRegistrySession",
                    &format!(
                        "issue={issue_identifier} session={} status={} source={} evidence={}",
                        record.session_name,
                        probe.status.as_str(),
                        probe.source.as_str(),
                        compact_evidence(&probe.evidence)
                    ),
                )?;
                println!(
                    "run_loop_resume_preflight action=active_registry_session issue={} session={} status={} source={} evidence={}",
                    issue_identifier,
                    record.session_name,
                    probe.status.as_str(),
                    probe.source.as_str(),
                    compact_evidence(&probe.evidence)
                );
                retained_states.push(state);
            }
            Ok(probe) => {
                if !matches!(probe.status, SessionStatus::Stale | SessionStatus::Failed) {
                    continue;
                }
                let Some(issue) =
                    recover_registry_issue_for_restart(adapter, config, &state, issue_identifier)?
                else {
                    continue;
                };
                let reason = format!(
                    "registry_session_recoverable session={} status={} source={} evidence={}",
                    record.session_name,
                    probe.status.as_str(),
                    probe.source.as_str(),
                    compact_evidence(&probe.evidence)
                );
                recover_registered_session_candidate(
                    config,
                    recoverable_states,
                    retained_states,
                    RecoverRegisteredSessionCandidateInput {
                        state: &state,
                        issue,
                        issue_identifier,
                        session_name: &record.session_name,
                        reason: &reason,
                        status: Some((&probe.status, probe.source.as_str())),
                    },
                )?;
            }
            Err(error) => {
                let Some(issue) =
                    recover_registry_issue_for_restart(adapter, config, &state, issue_identifier)?
                else {
                    continue;
                };
                let reason = format!(
                    "registry_session_unavailable session={} reason={}",
                    record.session_name,
                    compact_evidence(&error.to_string())
                );
                recover_registered_session_candidate(
                    config,
                    recoverable_states,
                    retained_states,
                    RecoverRegisteredSessionCandidateInput {
                        state: &state,
                        issue,
                        issue_identifier,
                        session_name: &record.session_name,
                        reason: &reason,
                        status: None,
                    },
                )?;
            }
        }
    }
    Ok(())
}

fn recover_registry_issue_allows_active_retention(
    adapter: &dyn TrackerAdapter,
    config: &RuntimeConfig,
    state: &RuntimeState,
    issue_identifier: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    match adapter.get_issue(issue_identifier) {
        Ok(Some(issue)) => Ok(normalize_state(&issue.state) == "in progress"),
        Ok(None) => Ok(false),
        Err(error) => {
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
                "run_loop_resume_preflight action=recover_registry_read_skipped issue={} reason={}",
                issue_identifier, reason
            );
            Ok(true)
        }
    }
}

fn recover_registry_issue_for_restart(
    adapter: &dyn TrackerAdapter,
    config: &RuntimeConfig,
    state: &RuntimeState,
    issue_identifier: &str,
) -> Result<Option<TrackerIssue>, Box<dyn std::error::Error>> {
    match adapter.get_issue(issue_identifier) {
        Ok(Some(issue)) if normalize_state(&issue.state) == "in progress" => Ok(Some(issue)),
        Ok(_) => Ok(None),
        Err(error) => {
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
                "run_loop_resume_preflight action=recover_registry_read_skipped issue={} reason={}",
                issue_identifier, reason
            );
            Ok(None)
        }
    }
}

struct RecoverRegisteredSessionCandidateInput<'a> {
    state: &'a RuntimeState,
    issue: TrackerIssue,
    issue_identifier: &'a str,
    session_name: &'a str,
    reason: &'a str,
    status: Option<(&'a SessionStatus, &'a str)>,
}

fn recover_registered_session_candidate(
    config: &RuntimeConfig,
    recoverable_states: &mut Vec<RuntimeRecoveryCandidate>,
    retained_states: &mut Vec<RuntimeState>,
    input: RecoverRegisteredSessionCandidateInput<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    append_runtime_supervision_event(
        config,
        Some(input.state),
        "RuntimeRecoverable",
        &format!("issue={} reason={}", input.issue_identifier, input.reason),
    )?;
    if let Some((status, source)) = input.status {
        println!(
            "run_loop_resume_preflight action=recoverable_registry_session issue={} session={} status={} source={}",
            input.issue_identifier,
            input.session_name,
            status.as_str(),
            source
        );
    } else {
        println!(
            "run_loop_resume_preflight action=recoverable_registry_session issue={} session={} status=unavailable",
            input.issue_identifier, input.session_name
        );
    }
    recoverable_states.push(RuntimeRecoveryCandidate {
        state: input.state.clone(),
        issue: input.issue,
        reason: input.reason.to_string(),
    });
    retained_states.push(input.state.clone());
    Ok(())
}

fn active_runtime_session_for_issue(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<ActiveRuntimeSessionProbe>, Box<dyn std::error::Error>> {
    let Some(issue_identifier) = runtime_state_issue_identifier(state) else {
        return Ok(None);
    };
    let registry = load_session_registry(&session_registry_path(config))?;
    let mut best: Option<(u8, ActiveRuntimeSessionProbe)> = None;
    for record in registry.sessions.iter().rev() {
        if !registered_main_runtime_session(record)
            || record.issue_identifier.as_deref() != Some(issue_identifier)
        {
            continue;
        }
        let Ok(probe) = runtime_session_probe_from_record(config, record, now_ms) else {
            continue;
        };
        if !session_status_counts_as_active_worker(&probe.status) {
            continue;
        }
        let priority = active_session_status_priority(&probe.status);
        if best
            .as_ref()
            .map(|(best_priority, _)| priority < *best_priority)
            .unwrap_or(true)
        {
            best = Some((
                priority,
                ActiveRuntimeSessionProbe {
                    state: runtime_state_from_session_record(record),
                    session_name: record.session_name.clone(),
                    probe,
                },
            ));
        }
    }
    if let Some((_, active_session)) = best {
        return Ok(Some(active_session));
    }

    let Some(probe) = runtime_session_probe_for_state(config, state, now_ms).unwrap_or(None) else {
        return Ok(None);
    };
    if !session_status_counts_as_active_worker(&probe.status) {
        return Ok(None);
    }
    Ok(Some(ActiveRuntimeSessionProbe {
        state: state.clone(),
        session_name: state
            .backend_session_id
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        probe,
    }))
}

fn terminal_runtime_session_for_issue(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<ActiveRuntimeSessionProbe>, Box<dyn std::error::Error>> {
    let Some(issue_identifier) = runtime_state_issue_identifier(state) else {
        return Ok(None);
    };
    let registry = load_session_registry(&session_registry_path(config))?;
    for record in registry.sessions.iter().rev() {
        if !registered_main_runtime_session(record)
            || record.issue_identifier.as_deref() != Some(issue_identifier)
        {
            continue;
        }
        let Ok(probe) = runtime_session_probe_from_record(config, record, now_ms) else {
            continue;
        };
        if matches!(probe.status, SessionStatus::Completed) {
            let mut state = runtime_state_from_session_record(record);
            state.last_event = Some("SessionTerminal".into());
            return Ok(Some(ActiveRuntimeSessionProbe {
                state,
                session_name: record.session_name.clone(),
                probe,
            }));
        }
        if session_status_counts_as_active_worker(&probe.status) {
            return Ok(None);
        }
    }

    Ok(None)
}

fn registered_main_runtime_session(record: &AgentSessionRecord) -> bool {
    record.lane.eq_ignore_ascii_case("main") && record.backend != "codex-app-manual"
}

pub(crate) fn runtime_state_issue_identifier(state: &RuntimeState) -> Option<&str> {
    state
        .active_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
}

fn runtime_state_from_session_record(record: &AgentSessionRecord) -> RuntimeState {
    let identifier = record
        .issue_identifier
        .clone()
        .unwrap_or_else(|| "unknown".into());
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: record
                .issue_id
                .clone()
                .unwrap_or_else(|| identifier.clone()),
            identifier,
        },
        record.backend.clone(),
    );
    state.workspace_path = Some(record.worktree.clone());
    state.branch_name = record.branch.clone();
    state.backend_session_id = Some(record.session_name.clone());
    state.lane = Some(record.lane.clone());
    state.run_id = record.run_id.clone();
    state.backend_log_path = Some(record.log_path.clone());
    state.backend_attach_command = Some(record.attach_command.clone());
    state.profile_id = record.profile_id.clone();
    state.instance_name = record.instance_name.clone();
    state.actor_role = record.actor_role.clone();
    state.actor_label = record.actor_label.clone();
    state.git_author = record.git_author.clone();
    state.attempt_count = record.attempt;
    state.updated_at_ms = Some(record.updated_at_ms);
    state.last_event = Some("SessionRunning".into());
    state
}

fn runtime_session_probe_for_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<SessionStatusProbe>, Box<dyn std::error::Error>> {
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }
    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(None);
    };
    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(None);
    };
    Ok(Some(runtime_session_probe_from_record(
        config, record, now_ms,
    )?))
}

fn runtime_session_probe_from_record(
    config: &RuntimeConfig,
    record: &AgentSessionRecord,
    now_ms: u64,
) -> Result<SessionStatusProbe, Box<dyn std::error::Error>> {
    let is_tmux_session = record.backend == "tmux";
    let pane_tail = if is_tmux_session {
        Some(
            capture_tmux_pane_tail(
                &config.tmux.command,
                &record.pane_target,
                DEFAULT_SESSION_STATUS_LINES,
            )
            .map_err(|error| {
                io::Error::other(format!(
                    "tmux_pane_unavailable session={} error={}",
                    record.session_name,
                    compact_evidence(&error)
                ))
            })?,
        )
    } else {
        None
    };
    let log_tail = if is_tmux_session {
        read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
    } else {
        None
    };
    Ok(classify_session_record(
        record,
        pane_tail.as_deref(),
        log_tail.as_deref(),
        now_ms,
        DEFAULT_SESSION_STALE_AFTER_MS,
    ))
}

fn session_status_counts_as_active_worker(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Starting
            | SessionStatus::Running
            | SessionStatus::WaitingForTrust
            | SessionStatus::WaitingForApproval
            | SessionStatus::WaitingForHumanInput
            | SessionStatus::UsageLimited
            | SessionStatus::Unknown
            | SessionStatus::UnknownPersisted(_)
    )
}

fn active_session_status_priority(status: &SessionStatus) -> u8 {
    match status {
        SessionStatus::Running => 0,
        SessionStatus::Starting => 1,
        SessionStatus::WaitingForApproval
        | SessionStatus::WaitingForHumanInput
        | SessionStatus::WaitingForTrust => 2,
        SessionStatus::UsageLimited => 3,
        SessionStatus::Unknown | SessionStatus::UnknownPersisted(_) => 4,
        SessionStatus::Completed
        | SessionStatus::Recorded
        | SessionStatus::Failed
        | SessionStatus::Stale => 5,
    }
}

fn runtime_recovery_reason(
    config: &RuntimeConfig,
    state: &RuntimeState,
    now_ms: u64,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if let Some(retry) = &state.retry {
        if retry.due_in_ms(now_ms) > 0 {
            return Ok(None);
        }
        return Ok(Some(format!(
            "retry_due attempt={} error={}",
            retry.attempt,
            compact_evidence(&retry.error)
        )));
    }

    if let Some(stall) = detect_runtime_stall(state, now_ms, config.codex.stall_timeout_ms) {
        return Ok(Some(format!(
            "runtime_stalled stalled_for_ms={} reason={}",
            stall.stalled_for_ms, stall.reason
        )));
    }

    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(Some("session_running_without_backend_session_id".into()));
    };

    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(Some(format!(
            "session_missing_from_registry session={session_id}"
        )));
    };

    match runtime_session_probe_from_record(config, record, now_ms) {
        Ok(probe) if matches!(probe.status, SessionStatus::Stale | SessionStatus::Failed) => {
            return Ok(Some(format!(
                "session_{} source={} evidence={}",
                probe.status.as_str(),
                probe.source.as_str(),
                compact_evidence(&probe.evidence)
            )));
        }
        Ok(_) => {}
        Err(error) => {
            return Ok(Some(compact_evidence(&error.to_string())));
        }
    }

    Ok(None)
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

fn stale_runtime_state_action(
    state: &RuntimeState,
    issue: &TrackerIssue,
    normalized_state: &str,
    config: &RuntimeConfig,
) -> Result<ResumePreflightAction, Box<dyn std::error::Error>> {
    let active_issue = state
        .active_issue
        .as_ref()
        .ok_or("runtime state has no active issue")?;
    let archive_reason = if config
        .terminal_state_set()
        .iter()
        .any(|state| state == normalized_state)
    {
        "tracker_state_terminal"
    } else if matches!(normalized_state, "agent review" | "human review") {
        "tracker_state_handoff"
    } else {
        "tracker_state_non_active"
    };

    match runtime_workspace_status(state)? {
        RuntimeWorkspaceStatus::Absent | RuntimeWorkspaceStatus::Clean(_) => {
            Ok(ResumePreflightAction::ArchiveStale {
                issue_identifier: active_issue.identifier.clone(),
                tracker_state: issue.state.clone(),
                archive_reason: archive_reason.into(),
            })
        }
        RuntimeWorkspaceStatus::Dirty(path) => Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}; workspace is dirty at {}",
                active_issue.identifier,
                issue.state,
                path.display()
            ),
        }),
        RuntimeWorkspaceStatus::Unknown { path, reason } => Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}; workspace status is unknown at {}: {}",
                active_issue.identifier,
                issue.state,
                path.display(),
                reason
            ),
        }),
    }
}

fn runtime_workspace_status(
    state: &RuntimeState,
) -> Result<RuntimeWorkspaceStatus, Box<dyn std::error::Error>> {
    let Some(path) = state.workspace_path.as_ref() else {
        return Ok(RuntimeWorkspaceStatus::Absent);
    };
    if !path.exists() {
        return Ok(RuntimeWorkspaceStatus::Absent);
    }
    if !path.is_dir() {
        return Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: "workspace path is not a directory".into(),
        });
    }

    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(path)
        .arg("status")
        .arg("--porcelain")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            if output.stdout.is_empty() {
                Ok(RuntimeWorkspaceStatus::Clean(path.clone()))
            } else {
                Ok(RuntimeWorkspaceStatus::Dirty(path.clone()))
            }
        }
        Ok(output) => Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(error) => Ok(RuntimeWorkspaceStatus::Unknown {
            path: path.clone(),
            reason: error.to_string(),
        }),
    }
}

fn archive_runtime_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
    reason: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let runtime_path = runtime_state_path(config);
    let archive_dir = runtime_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("archive");
    std::fs::create_dir_all(&archive_dir)?;
    let issue_ref = state
        .active_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
        .unwrap_or("unknown");
    let archive_path = archive_dir.join(format!(
        "runtime-state-{}-{}-{}.json",
        current_time_ms(),
        sanitize_archive_segment(issue_ref),
        sanitize_archive_segment(reason)
    ));
    std::fs::write(&archive_path, serde_json::to_string_pretty(state)?)?;
    Ok(archive_path)
}

fn sanitize_archive_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('-').to_string()
}
