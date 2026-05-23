use jade_symphony::agent::UsageLimitPause;
use jade_symphony::config::RuntimeConfig;
use jade_symphony::handoff::IssueHandoffPlan;
use jade_symphony::lane_claim::LaneClaim;
use jade_symphony::model::TrackerIssue;
use jade_symphony::profiles::selected_execution_profile;
use jade_symphony::runtime_state::{RuntimeIssueState, RuntimeState, RuntimeTransition};
use jade_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    session_registry_path, unix_timestamp_ms, AgentSessionRecord, SessionStatus,
};
use jade_symphony::workspace::{GitIdentityApplyResult, GitIdentityApplyStatus};

use super::{runtime_state_issue_identifier, IssueExecutionResult};
use crate::{compact_evidence, DEFAULT_SESSION_STALE_AFTER_MS, DEFAULT_SESSION_STATUS_LINES};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MainSessionReconciliation {
    Terminal(Box<IssueExecutionResult>),
    Active {
        status: String,
        source: String,
        evidence: String,
    },
}

pub(crate) fn main_session_active_recoverable(status: &str, evidence: &str) -> bool {
    status == "stale"
        || (status == "unknown"
            && (evidence.contains("missing from session registry")
                || evidence.contains("without backend session id")
                || evidence.contains("tmux")
                || evidence.contains("unavailable")))
}

pub(crate) fn run_loop_runtime_state_for_issue(
    existing: Option<&RuntimeState>,
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    event: &str,
    claim: &LaneClaim,
) -> RuntimeState {
    if event == "Resumed" {
        if let Some(existing) = existing
            .filter(|state| {
                runtime_state_issue_identifier(state) == Some(issue.identifier.as_str())
            })
            .filter(|state| state.last_event.as_deref() == Some("SessionTerminal"))
        {
            let mut state = existing.clone();
            state.run_id.get_or_insert_with(|| claim.run.clone());
            return state;
        }
    }

    let profile = selected_execution_profile(&config.profiles).ok().flatten();
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: issue.id.clone(),
            identifier: issue.identifier.clone(),
        },
        &config.backend.kind,
    );
    state.attempt_count = next_runtime_attempt_count(existing, &issue.identifier);
    state.branch_name = issue.branch_name.clone();
    state.lane = Some("main".into());
    state.run_id = Some(claim.run.clone());
    state.profile_id = profile.as_ref().map(|profile| profile.profile_id.clone());
    state.instance_name = profile
        .as_ref()
        .map(|profile| profile.instance_name.clone());
    state.actor_role = Some(config.identity.actor_role.clone());
    state.actor_label = Some(config.identity.actor_label.clone());
    state.git_author = config.identity.git.author();
    state.last_event = Some(event.into());
    state
}

fn next_runtime_attempt_count(existing: Option<&RuntimeState>, issue_identifier: &str) -> u32 {
    existing
        .and_then(|state| {
            state
                .active_issue
                .as_ref()
                .filter(|issue| issue.identifier == issue_identifier)
                .map(|_| state.attempt_count.saturating_add(1))
        })
        .unwrap_or(1)
}

pub(crate) fn run_loop_runtime_state_with_result(
    mut state: RuntimeState,
    result: &IssueExecutionResult,
) -> RuntimeState {
    state.workspace_path = Some(result.workspace_path.clone());
    state.backend = result.backend.clone();
    state.backend_session_id = result.session_id.clone();
    state.run_id = result.run_id.clone();
    state.backend_log_path = result.backend_log_path.clone();
    state.backend_attach_command = result.backend_attach_command.clone();
    state.profile_id = result.profile_id.clone();
    state.instance_name = result.instance_name.clone();
    state.actor_role = Some(result.actor_role.clone());
    state.actor_label = Some(result.actor_label.clone());
    state.git_author = result.git_author.clone();
    state.last_event = Some(if result.pending_session {
        "SessionRunning".into()
    } else if result.success {
        "Completed".into()
    } else {
        "Failed".into()
    });
    state
}

pub(crate) fn run_loop_runtime_state_with_transition(
    mut state: RuntimeState,
    from: Option<String>,
    to: &str,
    reason: &str,
) -> RuntimeState {
    state.last_transition = Some(RuntimeTransition {
        from,
        to: to.into(),
        reason: reason.into(),
    });
    state
}

pub(crate) fn reconcile_pending_main_session(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
    state: &RuntimeState,
) -> Result<Option<MainSessionReconciliation>, Box<dyn std::error::Error>> {
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(None);
    };
    if active_issue.identifier != issue.identifier {
        return Ok(None);
    }
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    let Some(session_id) = state.backend_session_id.as_deref() else {
        return Ok(Some(MainSessionReconciliation::Active {
            status: "unknown".into(),
            source: "runtime".into(),
            evidence: "runtime state records SessionRunning without backend session id".into(),
        }));
    };

    let registry = load_session_registry(&session_registry_path(config))?;
    let Some(record) = registry
        .sessions
        .iter()
        .rev()
        .find(|record| record.session_name == session_id)
    else {
        return Ok(Some(MainSessionReconciliation::Active {
            status: "unknown".into(),
            source: "runtime".into(),
            evidence: format!("runtime session {session_id} is missing from session registry"),
        }));
    };

    let is_tmux_session = record.backend == "tmux";
    let pane_tail = if is_tmux_session {
        match capture_tmux_pane_tail(
            &config.tmux.command,
            &record.pane_target,
            DEFAULT_SESSION_STATUS_LINES,
        ) {
            Ok(tail) => Some(tail),
            Err(error) => {
                return Ok(Some(MainSessionReconciliation::Active {
                    status: "unknown".into(),
                    source: "tmux".into(),
                    evidence: format!(
                        "tmux pane unavailable for session {session_id}: {}",
                        compact_evidence(&error)
                    ),
                }))
            }
        }
    } else {
        None
    };
    let log_tail = if is_tmux_session {
        read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
    } else {
        None
    };
    let probe = classify_session_record(
        record,
        pane_tail.as_deref(),
        log_tail.as_deref(),
        unix_timestamp_ms(),
        DEFAULT_SESSION_STALE_AFTER_MS,
    );

    match probe.status {
        SessionStatus::Completed => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                true,
                None,
                probe.evidence,
            ),
        )))),
        SessionStatus::Failed => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                false,
                None,
                format!("main session failed: {}", probe.evidence),
            ),
        )))),
        SessionStatus::UsageLimited => Ok(Some(MainSessionReconciliation::Terminal(Box::new(
            result_from_reconciled_main_session(
                config,
                handoff,
                state,
                record,
                false,
                Some(UsageLimitPause {
                    classifier: "usage_limit".into(),
                    evidence: probe.evidence.clone(),
                }),
                format!("main session usage-limited: {}", probe.evidence),
            ),
        )))),
        _ => Ok(Some(MainSessionReconciliation::Active {
            status: probe.status.as_str().into(),
            source: probe.source.as_str().into(),
            evidence: probe.evidence,
        })),
    }
}

fn result_from_reconciled_main_session(
    config: &RuntimeConfig,
    handoff: &IssueHandoffPlan,
    state: &RuntimeState,
    record: &AgentSessionRecord,
    success: bool,
    usage_limit_pause: Option<UsageLimitPause>,
    message: String,
) -> IssueExecutionResult {
    IssueExecutionResult {
        workspace_path: state
            .workspace_path
            .clone()
            .unwrap_or_else(|| handoff.workspace_path.clone()),
        backend: state.backend.clone(),
        profile_id: state.profile_id.clone(),
        instance_name: state.instance_name.clone(),
        success,
        pending_session: false,
        session_id: state
            .backend_session_id
            .clone()
            .or_else(|| Some(record.session_name.clone())),
        run_id: state.run_id.clone().or_else(|| record.run_id.clone()),
        backend_log_path: state
            .backend_log_path
            .clone()
            .or_else(|| Some(record.log_path.clone())),
        backend_attach_command: state
            .backend_attach_command
            .clone()
            .or_else(|| Some(record.attach_command.clone())),
        message,
        usage_limit_pause,
        prompt_artifact_path: Some(record.prompt_artifact_path.clone()),
        actor_role: state
            .actor_role
            .clone()
            .unwrap_or_else(|| config.identity.actor_role.clone()),
        actor_label: state
            .actor_label
            .clone()
            .unwrap_or_else(|| config.identity.actor_label.clone()),
        git_author: state
            .git_author
            .clone()
            .or_else(|| config.identity.git.author()),
        git_identity: GitIdentityApplyResult {
            status: GitIdentityApplyStatus::NotConfigured,
            author: state.git_author.clone(),
            applied_keys: Vec::new(),
        },
        live_handoff: None,
        handoff_verification: None,
    }
}
