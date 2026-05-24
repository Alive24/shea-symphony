use crate::model::{normalize_state, SessionStatusSnapshot, TrackerIssue};
use crate::runtime_state::{detect_runtime_stall, RuntimeState};

use super::{
    find_issue_by_ref, issue_refs_match, violation, AuditSeverity, ProjectAuditViolation,
    ProjectDoctorContext,
};

pub(super) fn audit_runtime_consistency(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    context: &ProjectDoctorContext,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    let runtime_states = context_runtime_states(context);
    if runtime_states.is_empty() {
        return;
    }
    let matching_runtime_states = runtime_states
        .iter()
        .copied()
        .filter(|runtime_state| {
            runtime_state
                .active_issue
                .as_ref()
                .map(|active_issue| issue_refs_match(&active_issue.identifier, &issue.identifier))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let runtime_matches_issue = !matching_runtime_states.is_empty();

    if runtime_matches_issue && normalized_issue_state != "in progress" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "runtime_state_tracker_mismatch",
            "Runtime state still points at this issue, but the tracker state is not In Progress.",
            "Inspect the runtime state and tracker evidence before clearing or resuming the active issue.",
        ));
    }

    if normalized_issue_state == "in progress" {
        for runtime_state in &matching_runtime_states {
            if let Some(stall) =
                detect_runtime_stall(runtime_state, context.now_ms, context.stale_after_ms)
            {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "runtime_state_stale",
                    &format!("Runtime state is stale: {}.", stall.reason),
                    "Use `doctor repair <issue>` to choose resume, no-op, or escalation before another worker claims it.",
                ));
            }
        }
    }

    if !runtime_matches_issue && normalized_issue_state == "in progress" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "runtime_active_issue_disagrees",
            "Issue is In Progress but runtime state points at a different active issue.",
            "Inspect both issues before dispatching or resetting ownership metadata.",
        ));
    }

    if runtime_matches_issue
        && matches!(normalized_issue_state, "todo" | "need to clarify")
        && matching_runtime_states
            .iter()
            .any(|runtime_state| runtime_state.workspace_path.is_some())
    {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "runtime_worktree_tracker_mismatch",
            "Runtime state has a workspace for this queued issue.",
            "Inspect the worktree and either resume the active work or move the issue to Need Human Input with evidence.",
        ));
    }

    for runtime_state in matching_runtime_states {
        if let (Some(runtime_branch), Some(tracker_branch)) = (
            runtime_state.branch_name.as_deref(),
            issue.branch_name.as_deref(),
        ) {
            if runtime_branch != tracker_branch {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "runtime_branch_mismatch",
                    "Runtime state branch and tracker branch disagree.",
                    "Inspect the runtime workspace, tracker branch, and PR evidence before repairing state.",
                ));
            }
        }
    }
}

pub(super) fn context_runtime_states(context: &ProjectDoctorContext) -> Vec<&RuntimeState> {
    if !context.runtime_states.is_empty() {
        context.runtime_states.iter().collect()
    } else {
        context.runtime_state.iter().collect()
    }
}

pub(super) fn audit_session_consistency(
    issues: &[TrackerIssue],
    context: &ProjectDoctorContext,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    for session in &context.sessions {
        let status = session.status.trim();
        let issue = session
            .issue_identifier
            .as_deref()
            .and_then(|identifier| find_issue_by_ref(issues, identifier));

        if session
            .issue_identifier
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            violations.push(session_violation(
                session,
                "tmux_session_missing_issue",
                &format!(
                    "Registered {} session has no issue identifier.",
                    session_backend(session)
                ),
                session_inspection_suggestion(session),
            ));
            continue;
        }

        let Some(issue) = issue else {
            violations.push(session_violation(
                session,
                "tmux_session_orphaned_issue",
                &format!(
                    "Registered {} session points at an issue that is not present in the current Project read.",
                    session_backend(session)
                ),
                "Inspect the registry entry, tracker state, and worktree before cleaning or reassigning the session.",
            ));
            continue;
        };

        if normalize_state(&issue.state) == "done" {
            continue;
        }

        if status == "stale" {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "tmux_session_stale",
                &format!(
                    "Registered {} session `{}` is stale: {}.",
                    session_backend(session),
                    session.session_id,
                    session.evidence
                ),
                session_inspection_suggestion(session),
            ));
        } else if session_status_needs_operator(status) {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "tmux_session_needs_operator_attention",
                &format!(
                    "Registered {} session `{}` is `{}` from {} evidence: {}.",
                    session_backend(session),
                    session.session_id,
                    session.status,
                    session.evidence_source,
                    session.evidence
                ),
                session_inspection_suggestion(session),
            ));
        }

        if session_status_active(status) && normalize_state(&issue.state) == "done" {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "tmux_session_active_for_terminal_issue",
                &format!(
                    "Registered {} session `{}` still appears active for a Done issue.",
                    session_backend(session),
                    session.session_id
                ),
                "Confirm the session is finished and evidence is preserved before cleanup.",
            ));
        }
    }

    for runtime_state in context_runtime_states(context) {
        let Some(active_issue) = runtime_state.active_issue.as_ref() else {
            continue;
        };
        let Some(runtime_session_id) = runtime_state.backend_session_id.as_deref() else {
            continue;
        };
        let Some(issue) = find_issue_by_ref(issues, &active_issue.identifier) else {
            continue;
        };
        if normalize_state(&issue.state) == "done" {
            continue;
        }
        let matching_session = context
            .sessions
            .iter()
            .find(|session| session.session_id == runtime_session_id);
        let Some(session) = matching_session else {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "runtime_session_missing_registry",
                "Runtime state references a backend session that is missing from the session registry.",
                "Inspect runtime-state.json and backend artifacts before clearing or retrying the run.",
            ));
            continue;
        };
        if session
            .issue_identifier
            .as_deref()
            .is_some_and(|identifier| !issue_refs_match(identifier, &active_issue.identifier))
        {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "runtime_session_issue_mismatch",
                "Runtime state active issue and registered backend session issue do not match.",
                "Inspect both records before dispatching another worker or cleaning artifacts.",
            ));
        }
    }
}

fn session_backend(session: &SessionStatusSnapshot) -> &str {
    let backend = session.backend.trim();
    if backend.is_empty() {
        "runtime"
    } else {
        backend
    }
}

fn session_inspection_suggestion(session: &SessionStatusSnapshot) -> &'static str {
    if session.backend == "tmux" {
        "Use the recorded attach command or log path to decide whether to resume, retry, or route the issue with evidence."
    } else {
        "Inspect the recorded runtime artifacts, event log, and tracker evidence before clearing, retrying, or routing the issue."
    }
}

fn session_status_active(status: &str) -> bool {
    matches!(
        status,
        "starting"
            | "running"
            | "waiting_for_trust"
            | "waiting_for_approval"
            | "waiting_for_human_input"
            | "usage_limited"
            | "unknown"
    )
}

fn session_status_needs_operator(status: &str) -> bool {
    matches!(
        status,
        "waiting_for_trust"
            | "waiting_for_approval"
            | "waiting_for_human_input"
            | "usage_limited"
            | "failed"
            | "unknown"
    )
}

fn session_violation(
    session: &SessionStatusSnapshot,
    code: &str,
    message: &str,
    suggestion: &str,
) -> ProjectAuditViolation {
    ProjectAuditViolation {
        issue_ref: format!("session:{}", session.session_id),
        title: session
            .issue_title
            .clone()
            .unwrap_or_else(|| "Unattributed tmux session".into()),
        state: session.status.clone(),
        severity: AuditSeverity::Warning,
        code: code.into(),
        message: message.into(),
        suggestion: suggestion.into(),
    }
}
