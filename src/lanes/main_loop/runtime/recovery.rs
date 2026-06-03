use std::process::Command;

use shea_symphony::codex_app_server;
use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::{normalize_state, TrackerIssue};
use shea_symphony::runtime_state::{detect_runtime_stall, RuntimeState};
use shea_symphony::session_registry::{
    load_session_registry, save_session_record, session_registry_path, unix_timestamp_ms,
    AgentSessionRecord, SessionStatus, SessionStatusProbe,
};
use shea_symphony::tracker::TrackerAdapter;

use crate::lanes::main_loop::compact_evidence;

use super::super::append_runtime_supervision_event;
use super::session_probe::{
    active_session_status_priority, registered_main_runtime_session,
    runtime_session_probe_for_state, runtime_session_probe_from_record,
    runtime_state_from_session_record, session_status_counts_as_active_worker,
};
use super::{runtime_state_issue_identifier, RuntimeRecoveryCandidate};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ActiveRuntimeSessionProbe {
    pub(super) state: RuntimeState,
    pub(super) session_name: String,
    pub(super) probe: SessionStatusProbe,
}

pub(super) fn recover_registered_main_sessions(
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
                let termination = if matches!(probe.status, SessionStatus::Stale) {
                    terminate_stale_codex_app_server_session(config, record)?
                } else {
                    None
                };
                let reason = format!(
                    "registry_session_recoverable session={} status={} source={} evidence={}{}",
                    record.session_name,
                    probe.status.as_str(),
                    probe.source.as_str(),
                    compact_evidence(&probe.evidence),
                    termination
                        .as_deref()
                        .map(|evidence| format!(" {evidence}"))
                        .unwrap_or_default()
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

fn terminate_stale_codex_app_server_session(
    config: &RuntimeConfig,
    record: &AgentSessionRecord,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if record.session_source.as_deref() != Some(codex_app_server::BACKEND_NAME) {
        return Ok(None);
    }
    let Some(process_id) = record.process_id else {
        return Ok(Some("terminate_process_id=missing".into()));
    };

    let output = Command::new("kill")
        .arg("-TERM")
        .arg(process_id.to_string())
        .output();
    let mut updated = record.clone();
    updated.status = SessionStatus::Stale;
    updated.updated_at_ms = unix_timestamp_ms();
    save_session_record(&session_registry_path(config), updated)?;

    match output {
        Ok(output) if output.status.success() => Ok(Some(format!(
            "terminated_process_id={process_id} signal=TERM"
        ))),
        Ok(output) => Ok(Some(format!(
            "terminate_process_id={process_id} failed status={} stderr={}",
            output.status,
            compact_evidence(&String::from_utf8_lossy(&output.stderr))
        ))),
        Err(error) => Ok(Some(format!(
            "terminate_process_id={process_id} failed error={}",
            compact_evidence(&error.to_string())
        ))),
    }
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
                "run_loop_resume_preflight action=recover_registry_read_skipped issue={issue_identifier} reason={reason}"
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
                "run_loop_resume_preflight action=recover_registry_read_skipped issue={issue_identifier} reason={reason}"
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

pub(super) fn active_runtime_session_for_issue(
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

pub(super) fn terminal_runtime_session_for_issue(
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

pub(super) fn runtime_recovery_reason(
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
