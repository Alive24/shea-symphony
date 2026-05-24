use std::io;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::runtime_state::{RuntimeIssueState, RuntimeState};
use shea_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    session_registry_path, AgentSessionRecord, SessionStatus, SessionStatusProbe,
};

use crate::lanes::main_loop::compact_evidence;
use crate::orchestration::{DEFAULT_SESSION_STALE_AFTER_MS, DEFAULT_SESSION_STATUS_LINES};

pub(super) fn registered_main_runtime_session(record: &AgentSessionRecord) -> bool {
    record.lane.eq_ignore_ascii_case("main") && record.backend != "codex-app-manual"
}

pub(super) fn runtime_state_from_session_record(record: &AgentSessionRecord) -> RuntimeState {
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

pub(super) fn runtime_session_probe_for_state(
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

pub(super) fn runtime_session_probe_from_record(
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

pub(super) fn session_status_counts_as_active_worker(status: &SessionStatus) -> bool {
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

pub(super) fn active_session_status_priority(status: &SessionStatus) -> u8 {
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
