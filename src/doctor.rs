use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod skills;
mod topology;

use crate::lane_claim::{LaneClaim, LaneClaimState};
use crate::model::{normalize_state, SessionStatusSnapshot, TrackerIssue};
use crate::runtime_state::{detect_runtime_stall, RuntimeState};
use topology::audit_parent_subissue_topology;

pub use skills::{
    append_local_skill_install_doctor_violations, default_jade_symphony_skill_targets,
    SkillInstallTarget,
};

pub const HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE: &str = "human_review_missing_review_evidence";
pub const AGENT_REVIEW_DRAFT_PR: &str = "agent_review_draft_pr";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditSeverity {
    Warning,
    Blocker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAuditViolation {
    pub issue_ref: String,
    pub title: String,
    pub state: String,
    pub severity: AuditSeverity,
    pub code: String,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAuditReport {
    pub total_issues: usize,
    pub violations: Vec<ProjectAuditViolation>,
    #[serde(default)]
    pub integration_gaps: Vec<String>,
    #[serde(default)]
    pub skill_readiness_summary: Option<String>,
}

impl ProjectAuditReport {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn blocker_count(&self) -> usize {
        self.violations
            .iter()
            .filter(|violation| violation.severity == AuditSeverity::Blocker)
            .count()
    }
}

#[derive(Debug, Clone)]
pub struct ProjectDoctorContext {
    pub runtime_state: Option<RuntimeState>,
    pub runtime_states: Vec<RuntimeState>,
    pub sessions: Vec<SessionStatusSnapshot>,
    pub now_ms: u64,
    pub stale_after_ms: u64,
}

pub fn audit_project_issues(issues: &[TrackerIssue]) -> ProjectAuditReport {
    audit_project_issues_with_context(issues, None)
}

pub fn audit_project_issues_with_context(
    issues: &[TrackerIssue],
    context: Option<&ProjectDoctorContext>,
) -> ProjectAuditReport {
    let mut violations = Vec::new();
    for issue in issues {
        let state = issue.normalized_state();
        audit_terminal_state_mismatch(issue, &state, &mut violations);

        if state == "done" {
            continue;
        }

        match state.as_str() {
            "agent review" if !has_pr_url(issue) && !has_handoff_evidence(issue) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Blocker,
                    "agent_review_missing_pr_handoff",
                    "Agent Review issue has no linked PR URL or handoff evidence.",
                    "Move back to Rework or Need Human Input with a workpad diagnostic, or repair the missing PR link.",
                ));
            }
            "agent review" if has_draft_pr(issue) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Blocker,
                    AGENT_REVIEW_DRAFT_PR,
                    "Agent Review issue has a linked draft PR.",
                    "Confirm handoff evidence, then run `doctor repair <issue> --mark-pr-ready --confirm-handoff-ready --write`; auto-fix will not mark PRs ready.",
                ));
            }
            "human review" if !has_review_pass_evidence(issue) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Blocker,
                    HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE,
                    "Human Review issue has no independent review pass evidence.",
                    "Return to Agent Review until Review Agent pass evidence is recorded.",
                ));
            }
            "merging" if reliable_pr_targets(issue).is_empty() => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Blocker,
                    "merging_missing_pr_target",
                    "Merging issue has no reliable PR target.",
                    "Record exactly one PR link in the Project field, issue closing reference, or Jade Symphony workpad before attempting to land.",
                ));
            }
            "merging" if reliable_pr_targets(issue).len() > 1 => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Blocker,
                    "merging_ambiguous_pr_target",
                    "Merging issue has multiple candidate PR targets.",
                    "Choose the correct PR and remove or supersede stale PR evidence before attempting to land.",
                ));
            }
            "merging" if has_dirty_or_conflicted_pr(issue) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Blocker,
                    "merging_pr_not_clean",
                    "Merging issue has a dirty, conflicted, or stale PR.",
                    "Move to Rework with review freshness evidence before attempting to land.",
                ));
            }
            "in progress" if !has_runtime_owner_metadata(issue) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "in_progress_missing_runtime_owner",
                    "In Progress issue has no visible runtime ownership metadata.",
                    "Confirm the active workspace/session before dispatching another worker.",
                ));
            }
            "todo" | "need to clarify"
                if has_pr_url(issue)
                    && !issue.project_fields.contains_key("pr_status_explanation") =>
            {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "queued_issue_has_pr",
                    "Queued or clarification issue already has a linked PR without explanation.",
                    "Add workpad context or move the issue to the state matching the PR.",
                ));
            }
            _ => {}
        }

        audit_lane_claim_fields(issue, &state, context, &mut violations);

        if state == "todo" && active_claimed_main_agent(issue).is_some() {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "todo_main_agent_claimed",
                "Todo issue already has a Main Agent claim marker.",
                "Treat it as partially claimed or interrupted work; inspect with `doctor repair <issue>` before dispatching another worker.",
            ));
        }

        if state == "in progress" && has_pr_url(issue) {
            violations.push(violation(
                issue,
                AuditSeverity::Warning,
                "in_progress_has_pr_evidence",
                "In Progress issue already has PR evidence.",
                "Inspect whether the work should be handed off to Agent Review or moved to Need Human Input with a workpad diagnostic.",
            ));
        }

        if let Some(context) = context {
            audit_runtime_consistency(issue, &state, context, &mut violations);
        }
    }

    audit_parent_subissue_topology(issues, &mut violations);

    if let Some(context) = context {
        audit_session_consistency(issues, context, &mut violations);
    }

    ProjectAuditReport {
        total_issues: issues.len(),
        violations,
        integration_gaps: Vec::new(),
        skill_readiness_summary: None,
    }
}

pub fn render_doctor_repair_workpad(
    issue: &TrackerIssue,
    report: &ProjectAuditReport,
    action: &str,
) -> String {
    let related = related_violations(report, &issue.identifier);
    let mut lines = vec![
        "## Jade Symphony Doctor Triage".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `doctor`".to_string(),
        "- Actor role: `doctor`".to_string(),
        "- Actor: `jade-symphony doctor`".to_string(),
        "- Run ID: `doctor-repair`".to_string(),
        format!("- Input state: `{}`", issue.state),
        format!(
            "- Target state after repair: `{}`",
            doctor_target_state(action)
        ),
        format!("- Result: `{}`", doctor_result(action)),
        format!("- Requested action: `{action}`"),
    ];

    if let Some(main_agent) = claimed_main_agent(issue) {
        lines.push(format!("- Main Agent: `{main_agent}`"));
    }
    if let Some(branch) = issue.branch_name.as_deref() {
        lines.push(format!("- Tracker branch: `{branch}`"));
    }
    if has_pr_url(issue) {
        let targets = reliable_pr_targets(issue).join(", ");
        lines.push(format!("- PR evidence: `{targets}`"));
    } else {
        lines.push("- PR evidence: `not recorded`".into());
    }
    lines.push(format!(
        "- Evidence summary: {} issue-specific doctor finding(s) captured before repair.",
        related.len()
    ));

    lines.extend([String::new(), "### Doctor Findings".to_string()]);
    if related.is_empty() {
        lines.push("- No issue-specific doctor violations were found.".to_string());
    } else {
        for violation in related {
            lines.push(format!(
                "- `{}` ({:?}): {}",
                violation.code, violation.severity, violation.message
            ));
        }
    }

    lines.extend([
        String::new(),
        "### State Boundary".to_string(),
        "- Doctor repair records evidence before any tracker mutation.".to_string(),
        "- This repair does not delete worktrees, discard local work, or bypass review/merge lane authority.".to_string(),
    ]);

    lines.join("\n")
}

pub fn render_project_audit_report(report: &ProjectAuditReport) -> String {
    let mut lines = vec![
        format!("project_doctor=ok issues={}", report.total_issues),
        format!("violations={}", report.violations.len()),
        format!("blockers={}", report.blocker_count()),
    ];
    if let Some(summary) = &report.skill_readiness_summary {
        lines.push(format!("skill_readiness_summary={summary}"));
    }

    if report.is_clean() {
        lines.push("summary=Project invariants look clean.".into());
    } else {
        lines.push("summary=Project invariants need attention.".into());
        for violation in &report.violations {
            lines.push(format!(
                "- {} {} state={} severity={:?} code={} message={}",
                violation.issue_ref,
                violation.title,
                violation.state,
                violation.severity,
                violation.code,
                violation.message
            ));
            lines.push(format!("  suggestion={}", violation.suggestion));
        }
    }

    for gap in &report.integration_gaps {
        lines.push(format!("integration_gap={gap}"));
    }

    lines.join("\n")
}

pub fn human_review_repair_candidates(report: &ProjectAuditReport) -> Vec<&ProjectAuditViolation> {
    report
        .violations
        .iter()
        .filter(|violation| {
            violation.severity == AuditSeverity::Blocker
                && violation.code == HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
        })
        .collect()
}

pub fn draft_pr_repair_candidates(report: &ProjectAuditReport) -> Vec<&ProjectAuditViolation> {
    report
        .violations
        .iter()
        .filter(|violation| {
            violation.severity == AuditSeverity::Blocker && violation.code == AGENT_REVIEW_DRAFT_PR
        })
        .collect()
}

pub fn render_human_review_repair_workpad(violation: &ProjectAuditViolation) -> String {
    [
        "## Jade Symphony Doctor Triage".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", violation.issue_ref, violation.title),
        "- Lane: `doctor`".into(),
        "- Actor role: `doctor`".into(),
        "- Actor: `jade-symphony doctor`".into(),
        "- Run ID: `doctor-human-review-repair`".into(),
        format!("- Input state: `{}`", violation.state),
        "- Target state after repair: `Agent Review`".into(),
        "- Result: `repair_recorded`".into(),
        "- PR evidence: `not recorded`".into(),
        format!("- Violation: `{}`", violation.code),
        format!("- Previous state: `{}`", violation.state),
        format!("- Message: {}", violation.message),
        format!("- Repair: {}", violation.suggestion),
        "- Evidence summary: invalid Human Review boundary repair evidence recorded before tracker mutation.".to_string(),
        String::new(),
        "### State Boundary".to_string(),
        "- Main implementation agent is moving this issue back to `Agent Review`.".to_string(),
        "- This repair does not set `Human Review`; that state requires independent Review Agent pass evidence.".to_string(),
    ]
    .join("\n")
}

fn doctor_target_state(action: &str) -> &'static str {
    match action {
        "move_need_human_input" => "Need Human Input",
        "mark_pr_ready" => "Agent Review",
        _ => "unchanged",
    }
}

fn doctor_result(action: &str) -> &'static str {
    match action {
        "move_need_human_input" => "routed",
        "mark_pr_ready" => "repair_recorded",
        _ => "triage_recorded",
    }
}

fn current_gmt_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format_gmt_timestamp(seconds)
}

fn format_gmt_timestamp(seconds_since_unix_epoch: u64) -> String {
    let days = (seconds_since_unix_epoch / 86_400) as i64;
    let seconds_of_day = seconds_since_unix_epoch % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} GMT")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_unix_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month as u32, day as u32)
}

pub fn render_project_audit_report_json(
    report: &ProjectAuditReport,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

fn violation(
    issue: &TrackerIssue,
    severity: AuditSeverity,
    code: &str,
    message: &str,
    suggestion: &str,
) -> ProjectAuditViolation {
    ProjectAuditViolation {
        issue_ref: issue.identifier.clone(),
        title: issue.title.clone(),
        state: issue.state.clone(),
        severity,
        code: code.into(),
        message: message.into(),
        suggestion: suggestion.into(),
    }
}

fn has_pr_url(issue: &TrackerIssue) -> bool {
    issue
        .linked_pull_requests
        .iter()
        .any(|pr| pr.url.as_deref().is_some_and(|url| !url.trim().is_empty()))
}

fn reliable_pr_targets(issue: &TrackerIssue) -> Vec<String> {
    let mut targets = Vec::new();
    for pr in &issue.linked_pull_requests {
        let target = pr
            .url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(str::to_string)
            .or_else(|| pr.number.map(|number| format!("#{number}")));
        if let Some(target) = target {
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    targets
}

fn has_draft_pr(issue: &TrackerIssue) -> bool {
    issue
        .linked_pull_requests
        .iter()
        .any(|pr| pr.is_draft == Some(true))
}

fn has_handoff_evidence(issue: &TrackerIssue) -> bool {
    bool_project_field(issue, "handoff_evidence_recorded")
        || string_project_field(issue, "handoff_evidence")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn has_review_pass_evidence(issue: &TrackerIssue) -> bool {
    bool_project_field(issue, "review_pass_evidence_recorded")
        || string_project_field(issue, "review_pass_evidence")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        || issue
            .description
            .as_deref()
            .is_some_and(has_review_pass_evidence_text)
}

fn has_review_pass_evidence_text(description: &str) -> bool {
    let normalized = description.to_lowercase();
    [
        "review pass evidence: `recorded`",
        "review pass evidence: recorded",
        "evidence recorded. independent review agent may move this issue to human review; the main implementation agent must not.",
        "independent agent review passed with recorded evidence; issue is ready for human review.",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn has_dirty_or_conflicted_pr(issue: &TrackerIssue) -> bool {
    string_project_field(issue, "pr_merge_state")
        .map(|state| {
            let state = normalize_state(&state);
            state == "dirty" || state == "blocked" || state == "behind" || state == "conflicted"
        })
        .unwrap_or(false)
}

fn audit_runtime_consistency(
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

fn context_runtime_states(context: &ProjectDoctorContext) -> Vec<&RuntimeState> {
    if !context.runtime_states.is_empty() {
        context.runtime_states.iter().collect()
    } else {
        context.runtime_state.iter().collect()
    }
}

fn audit_terminal_state_mismatch(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    let Some(github_issue_state) =
        string_project_field(issue, "GitHub Issue State").map(|value| normalize_state(&value))
    else {
        return;
    };

    if github_issue_state == "closed" && normalized_issue_state != "done" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "closed_issue_not_done",
            "GitHub issue is closed, but Project Status is not Done.",
            "Reconcile the Project status with the closed GitHub issue before relying on tracker health.",
        ));
    }

    if normalized_issue_state == "done" && github_issue_state != "closed" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "done_project_issue_still_open",
            "Project Status is Done, but the GitHub issue is still open.",
            "Close the GitHub issue or move the Project item back to the appropriate active state.",
        ));
    }
}

fn audit_session_consistency(
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

fn find_issue_by_ref<'a>(issues: &'a [TrackerIssue], issue_ref: &str) -> Option<&'a TrackerIssue> {
    issues
        .iter()
        .find(|issue| issue_refs_match(&issue.identifier, issue_ref))
}

fn bool_project_field(issue: &TrackerIssue, key: &str) -> bool {
    issue
        .project_fields
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn string_project_field(issue: &TrackerIssue, key: &str) -> Option<String> {
    issue
        .project_fields
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn string_project_field_any(issue: &TrackerIssue, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| string_project_field(issue, key))
}

fn claimed_main_agent(issue: &TrackerIssue) -> Option<String> {
    string_project_field_any(issue, &["Main Agent", "main_agent"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn active_claimed_main_agent(issue: &TrackerIssue) -> Option<String> {
    claimed_main_agent(issue).filter(|value| match LaneClaim::parse(value) {
        Ok(claim) => claim.state == LaneClaimState::Active,
        Err(_) => true,
    })
}

fn audit_lane_claim_fields(
    issue: &TrackerIssue,
    normalized_issue_state: &str,
    context: Option<&ProjectDoctorContext>,
    violations: &mut Vec<ProjectAuditViolation>,
) {
    for (field, expected_lane) in [
        ("Main Agent", "main"),
        ("Review Agent", "review"),
        ("Merging Agent", "merge"),
    ] {
        let Some(value) = string_project_field(issue, field)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        match LaneClaim::parse(&value) {
            Ok(claim) => {
                if claim.lane.as_str() != expected_lane {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "lane_claim_mismatched_lane",
                        &format!(
                            "{field} claim has lane `{}` instead of `{expected_lane}`.",
                            claim.lane.as_str()
                        ),
                        "Rewrite the claim through the owning lane so the Project field matches its lane.",
                    ));
                }
                if claim.issue != issue.identifier {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "lane_claim_mismatched_issue",
                        &format!("{field} claim points at `{}`.", claim.issue),
                        "Preserve the old evidence in the workpad, then write a fresh claim for this issue if work is still active.",
                    ));
                }
                if matches!(normalized_issue_state, "done" | "closed")
                    && claim.state == LaneClaimState::Active
                {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "terminal_issue_active_lane_claim",
                        &format!("{field} claim is still `state=active` on a terminal issue."),
                        "Update the structured claim to `state=done` after preserving run evidence.",
                    ));
                }
                if claim.state == LaneClaimState::Active
                    && !matches!(normalized_issue_state, "done" | "closed")
                    && context.is_some_and(|context| !context_has_run(context, &claim.run))
                {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "active_lane_claim_missing_registry",
                        &format!("{field} claim run `{}` has no matching runtime/session registry evidence.", claim.run),
                        "Preserve any issue/worktree/PR context, then use doctor repair or a superseding lane claim before starting replacement work.",
                    ));
                }
                if claim.state.is_terminal_audit_pointer()
                    && !matches!(normalized_issue_state, "done" | "closed")
                    && context.is_some_and(|context| !context_has_run(context, &claim.run))
                {
                    violations.push(violation(
                        issue,
                        AuditSeverity::Warning,
                        "terminal_lane_claim_missing_registry",
                        &format!("{field} terminal claim run `{}` has no matching runtime/session registry evidence.", claim.run),
                        "Treat this as historical audit guidance; preserve the claim and supersede it only if this lane needs fresh work.",
                    ));
                }
            }
            Err(_) if matches!(normalized_issue_state, "done" | "closed") => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "terminal_issue_legacy_lane_claim",
                    &format!("{field} retains a legacy claim value."),
                    "Keep it as audit evidence for now; migrate it through a future doctor repair flow if needed.",
                ));
            }
            Err(_) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "active_issue_legacy_lane_claim",
                    &format!("{field} contains a legacy claim value."),
                    "Inspect the active workspace/session, then supersede it with a structured `v=1` claim before dispatching another worker.",
                ));
            }
        }
    }
}

fn context_has_run(context: &ProjectDoctorContext, run_id: &str) -> bool {
    context_runtime_states(context)
        .iter()
        .any(|state| state.run_id.as_deref() == Some(run_id))
        || context
            .sessions
            .iter()
            .any(|session| session.run_id.as_deref() == Some(run_id))
}

fn has_runtime_owner_metadata(issue: &TrackerIssue) -> bool {
    issue.project_fields.contains_key("runtime_owner")
        || issue.project_fields.contains_key("runtime_state")
        || claimed_main_agent(issue).is_some()
        || string_project_field_any(issue, &["Merging Agent", "merging_agent"])
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
}

fn issue_refs_match(left: &str, right: &str) -> bool {
    normalize_issue_ref(left) == normalize_issue_ref(right)
}

fn normalize_issue_ref(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
}

fn related_violations<'a>(
    report: &'a ProjectAuditReport,
    issue_ref: &str,
) -> Vec<&'a ProjectAuditViolation> {
    report
        .violations
        .iter()
        .filter(|violation| issue_refs_match(&violation.issue_ref, issue_ref))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LinkedPullRequest;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    fn issue(identifier: &str, state: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: identifier.into(),
            item_id: None,
            identifier: identifier.into(),
            title: format!("Issue {identifier}"),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: BTreeMap::new(),
            created_at: None,
            updated_at: None,
        }
    }

    fn linked_pr(url: &str, state: &str) -> LinkedPullRequest {
        LinkedPullRequest {
            id: Some("PR_1".into()),
            number: Some(1),
            url: Some(url.into()),
            state: Some(state.into()),
            ..Default::default()
        }
    }

    fn linked_pr_to(url: &str, state: &str, base: &str) -> LinkedPullRequest {
        let mut pr = linked_pr(url, state);
        pr.base_ref_name = Some(base.into());
        pr
    }

    fn with_native_parent(mut issue: TrackerIssue, parent: &str) -> TrackerIssue {
        issue.project_fields.insert(
            "GitHub Native Parent".into(),
            serde_json::json!({ "identifier": parent }),
        );
        issue
    }

    fn with_native_subissues(mut issue: TrackerIssue, subissues: &[&str]) -> TrackerIssue {
        issue.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::Value::Array(
                subissues
                    .iter()
                    .map(|issue_ref| serde_json::json!({ "identifier": issue_ref }))
                    .collect(),
            ),
        );
        issue
    }

    fn with_parent_branch(mut issue: TrackerIssue, branch: &str) -> TrackerIssue {
        issue.description = Some(format!(
            "## Parent Topology\n\nParent integration branch: `{branch}`"
        ));
        issue
    }

    fn runtime_state(identifier: &str, updated_at_ms: u64) -> RuntimeState {
        let mut runtime_state = RuntimeState::active(
            crate::runtime_state::RuntimeIssueState {
                id: format!("ISSUE_{identifier}"),
                identifier: identifier.into(),
            },
            "dry-run",
        );
        runtime_state.updated_at_ms = Some(updated_at_ms);
        runtime_state
    }

    fn runtime_context(identifier: &str, updated_at_ms: u64) -> ProjectDoctorContext {
        let runtime_state = runtime_state(identifier, updated_at_ms);
        ProjectDoctorContext {
            runtime_state: Some(runtime_state),
            runtime_states: Vec::new(),
            sessions: Vec::new(),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        }
    }

    fn session(identifier: Option<&str>, status: &str) -> SessionStatusSnapshot {
        SessionStatusSnapshot {
            session_id: "jade-main-202-attempt-1-runtime".into(),
            lane: "main".into(),
            backend: "tmux".into(),
            run_id: None,
            status: status.into(),
            evidence_source: "registry".into(),
            evidence: "registry record has not updated for 19000ms".into(),
            issue_identifier: identifier.map(str::to_string),
            issue_title: Some("Runtime session".into()),
            attach_command: Some("tmux attach-session -t jade-main-202-attempt-1-runtime".into()),
            log_path: Some("/tmp/jade/logs/tmux/jade-main-202-attempt-1-runtime.log".into()),
            updated_at_ms: 1_000,
        }
    }

    fn with_github_issue_state(mut issue: TrackerIssue, state: &str) -> TrackerIssue {
        issue.project_fields.insert(
            "GitHub Issue State".into(),
            serde_json::Value::String(state.into()),
        );
        issue
    }

    #[test]
    fn reports_agent_review_missing_pr_handoff() {
        let report = audit_project_issues(&[issue("#57", "Agent Review")]);

        assert_eq!(report.blocker_count(), 1);
        assert_eq!(report.violations[0].code, "agent_review_missing_pr_handoff");
    }

    #[test]
    fn accepts_agent_review_with_pr_url() {
        let mut issue = issue("#57", "Agent Review");
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/57",
            "OPEN",
        ));

        let report = audit_project_issues(&[issue]);

        assert!(report.is_clean());
    }

    #[test]
    fn accepts_completed_main_session_for_agent_review_issue() {
        let mut issue = issue("#57", "Agent Review");
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/57",
            "OPEN",
        ));
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: vec![session(Some("#57"), "completed")],
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report.is_clean());
    }

    #[test]
    fn reports_raw_unknown_session_status_drift() {
        let mut drifted = session(Some("#202"), "unknown");
        drifted.evidence = "unknown persisted session status recorded_legacy".into();
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: vec![drifted],
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };
        let report =
            audit_project_issues_with_context(&[issue("#202", "In Progress")], Some(&context));

        let violation = report
            .violations
            .iter()
            .find(|violation| violation.code == "tmux_session_needs_operator_attention")
            .unwrap();
        assert!(violation
            .message
            .contains("unknown persisted session status recorded_legacy"));
    }

    #[test]
    fn reports_agent_review_with_draft_pr() {
        let mut issue = issue("#57", "Agent Review");
        let mut pr = linked_pr("https://github.com/Alive24/jade-symphony/pull/57", "OPEN");
        pr.is_draft = Some(true);
        issue.linked_pull_requests.push(pr);

        let report = audit_project_issues(&[issue]);

        assert_eq!(report.blocker_count(), 1);
        assert_eq!(report.violations[0].code, AGENT_REVIEW_DRAFT_PR);
        assert_eq!(draft_pr_repair_candidates(&report).len(), 1);
    }

    #[test]
    fn reports_human_review_without_review_pass_evidence() {
        let report = audit_project_issues(&[issue("#41", "Human Review")]);

        assert_eq!(
            report.violations[0].code,
            HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
        );
    }

    #[test]
    fn accepts_human_review_with_workpad_review_pass_evidence() {
        let mut issue = issue("#41", "Human Review");
        issue.description = Some(
            [
                "<!-- jade-symphony-workpad -->",
                "## Agent Review",
                "- Decision: Independent Agent Review passed with recorded evidence; issue is ready for Human Review.",
                "- Review pass evidence: `recorded`",
                "Evidence recorded. Independent Review Agent may move this issue to Human Review; the main implementation agent must not.",
            ]
            .join("\n"),
        );

        let report = audit_project_issues(&[issue]);

        assert!(report.is_clean());
    }

    #[test]
    fn accepts_happy_parent_subissue_topology() {
        let parent_branch = "integration/issue-243-parent-subissue-orchestration";
        let mut parent = with_parent_branch(
            with_native_subissues(issue("#243", "Human Review"), &["#272", "#273"]),
            parent_branch,
        );
        parent.description = Some(format!(
            "Parent integration branch: `{parent_branch}`\nReview pass evidence: `recorded`"
        ));
        let mut subissue_one = with_native_parent(issue("#272", "Done"), "#243");
        subissue_one.linked_pull_requests.push(linked_pr_to(
            "https://github.com/Alive24/jade-symphony/pull/272",
            "MERGED",
            parent_branch,
        ));
        let mut subissue_two = with_native_parent(issue("#273", "Done"), "#243");
        subissue_two.linked_pull_requests.push(linked_pr_to(
            "https://github.com/Alive24/jade-symphony/pull/273",
            "MERGED",
            parent_branch,
        ));

        let report = audit_project_issues(&[parent, subissue_one, subissue_two]);

        assert!(report.is_clean());
    }

    #[test]
    fn reports_subissue_pr_targeting_main() {
        let parent_branch = "integration/issue-243-parent-subissue-orchestration";
        let parent = with_parent_branch(
            with_native_subissues(issue("#243", "Todo"), &["#273"]),
            parent_branch,
        );
        let mut subissue = with_native_parent(issue("#273", "Agent Review"), "#243");
        subissue.linked_pull_requests.push(linked_pr_to(
            "https://github.com/Alive24/jade-symphony/pull/273",
            "OPEN",
            "main",
        ));

        let report = audit_project_issues(&[parent, subissue]);

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "subissue_pr_targets_main"
                && violation.severity == AuditSeverity::Blocker));
    }

    #[test]
    fn reports_missing_parent_integration_branch_evidence() {
        let parent = with_native_subissues(issue("#243", "Todo"), &["#273"]);
        let subissue = with_native_parent(issue("#273", "Todo"), "#243");

        let report = audit_project_issues(&[parent, subissue]);

        assert!(report.violations.iter().any(|violation| {
            violation.code == "parent_topology_missing_integration_branch"
                && violation.severity == AuditSeverity::Blocker
        }));
    }

    #[test]
    fn reports_done_subissue_without_parent_merge_evidence() {
        let parent_branch = "integration/issue-243-parent-subissue-orchestration";
        let parent = with_parent_branch(
            with_native_subissues(issue("#243", "Todo"), &["#272"]),
            parent_branch,
        );
        let mut subissue = with_native_parent(issue("#272", "Done"), "#243");
        subissue.linked_pull_requests.push(linked_pr_to(
            "https://github.com/Alive24/jade-symphony/pull/272",
            "OPEN",
            parent_branch,
        ));

        let report = audit_project_issues(&[parent, subissue]);

        assert!(report.violations.iter().any(|violation| {
            violation.code == "subissue_done_missing_parent_merge"
                && violation.severity == AuditSeverity::Blocker
        }));
    }

    #[test]
    fn reports_parent_human_review_before_subissues_done() {
        let parent_branch = "integration/issue-243-parent-subissue-orchestration";
        let mut parent = with_parent_branch(
            with_native_subissues(issue("#243", "Human Review"), &["#272", "#273"]),
            parent_branch,
        );
        parent.description = Some(format!(
            "Parent integration branch: `{parent_branch}`\nReview pass evidence: `recorded`"
        ));
        let mut done_subissue = with_native_parent(issue("#272", "Done"), "#243");
        done_subissue.linked_pull_requests.push(linked_pr_to(
            "https://github.com/Alive24/jade-symphony/pull/272",
            "MERGED",
            parent_branch,
        ));
        let mut active_subissue = with_native_parent(issue("#273", "Agent Review"), "#243");
        active_subissue.linked_pull_requests.push(linked_pr_to(
            "https://github.com/Alive24/jade-symphony/pull/273",
            "MERGED",
            parent_branch,
        ));

        let report = audit_project_issues(&[parent, done_subissue, active_subissue]);

        assert!(report.violations.iter().any(|violation| {
            violation.code == "parent_human_review_subissue_not_done"
                && violation.severity == AuditSeverity::Blocker
        }));
    }

    #[test]
    fn reports_body_only_parent_hierarchy_as_warning() {
        let mut subissue = issue("#274", "Todo");
        subissue.description =
            Some("Related Parent Issue or Context: subissue under parent issue #243.".into());

        let report = audit_project_issues(&[subissue]);

        assert!(report.violations.iter().any(|violation| {
            violation.code == "body_only_parent_hierarchy"
                && violation.severity == AuditSeverity::Warning
        }));
    }

    #[test]
    fn review_agent_claim_alone_does_not_satisfy_human_review_evidence() {
        let mut issue = issue("#41", "Human Review");
        issue.project_fields.insert(
            "Review Agent".into(),
            serde_json::Value::String("Gemini A".into()),
        );

        let report = audit_project_issues(&[issue]);

        assert_eq!(
            report.violations[0].code,
            HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
        );
    }

    #[test]
    fn failed_review_workpad_does_not_satisfy_human_review_evidence() {
        let mut issue = issue("#41", "Human Review");
        issue.description = Some(
            [
                "<!-- jade-symphony-workpad -->",
                "## Agent Review",
                "- Decision: Agent Review needs additional context; Human Review is not allowed yet.",
                "- Review did not pass; unavailable or inconclusive review must not move to Human Review.",
            ]
            .join("\n"),
        );

        let report = audit_project_issues(&[issue]);

        assert_eq!(
            report.violations[0].code,
            HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE
        );
    }

    #[test]
    fn human_review_repair_candidates_are_specific_to_missing_review_evidence() {
        let report =
            audit_project_issues(&[issue("#41", "Human Review"), issue("#57", "Agent Review")]);

        let candidates = human_review_repair_candidates(&report);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].issue_ref, "#41");
    }

    #[test]
    fn human_review_repair_workpad_preserves_authority_boundary() {
        let report = audit_project_issues(&[issue("#41", "Human Review")]);
        let workpad = render_human_review_repair_workpad(&report.violations[0]);

        assert!(workpad.contains("## Jade Symphony Doctor Triage"));
        assert!(workpad.contains("moving this issue back to `Agent Review`"));
        assert!(workpad.contains("does not set `Human Review`"));
    }

    #[test]
    fn reports_dirty_merging_pr() {
        let mut issue = issue("#60", "Merging");
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/60",
            "OPEN",
        ));
        issue.project_fields.insert(
            "pr_merge_state".into(),
            serde_json::Value::String("DIRTY".into()),
        );

        let report = audit_project_issues(&[issue]);

        assert_eq!(report.violations[0].code, "merging_pr_not_clean");
    }

    #[test]
    fn reports_merging_missing_pr_target() {
        let report = audit_project_issues(&[issue("#60", "Merging")]);

        assert_eq!(report.blocker_count(), 1);
        assert_eq!(report.violations[0].code, "merging_missing_pr_target");
    }

    #[test]
    fn reports_merging_ambiguous_pr_target() {
        let mut issue = issue("#60", "Merging");
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/60",
            "OPEN",
        ));
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/61",
            "OPEN",
        ));

        let report = audit_project_issues(&[issue]);

        assert_eq!(report.blocker_count(), 1);
        assert_eq!(report.violations[0].code, "merging_ambiguous_pr_target");
    }

    #[test]
    fn accepts_merging_with_one_pr_target() {
        let mut issue = issue("#60", "Merging");
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/60",
            "OPEN",
        ));

        let report = audit_project_issues(&[issue]);

        assert!(report.is_clean());
    }

    #[test]
    fn renders_operator_summary() {
        let report = audit_project_issues(&[issue("#57", "Agent Review")]);
        let rendered = render_project_audit_report(&report);

        assert!(rendered.contains("project_doctor=ok"));
        assert!(rendered.contains("violations=1"));
        assert!(rendered.contains("agent_review_missing_pr_handoff"));
    }

    #[test]
    fn renders_json_report() {
        let mut report = audit_project_issues(&[issue("#57", "Agent Review")]);
        report.integration_gaps.push("missing scope".into());

        let rendered = render_project_audit_report_json(&report).unwrap();
        let parsed: ProjectAuditReport = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed.total_issues, 1);
        assert_eq!(parsed.blocker_count(), 1);
        assert_eq!(parsed.violations[0].code, "agent_review_missing_pr_handoff");
        assert_eq!(parsed.integration_gaps, vec!["missing scope"]);
        assert!(!rendered.contains("\nintegration_gap="));
    }

    #[test]
    fn reports_todo_with_main_agent_claim() {
        let mut issue = issue("#202", "Todo");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String("codex-alpha".into()),
        );

        let report = audit_project_issues(&[issue]);

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "todo_main_agent_claimed"));
    }

    #[test]
    fn skips_done_issue_legacy_lane_claims() {
        let mut issue = with_github_issue_state(issue("#67", "Done"), "CLOSED");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String("legacy-codex-worker".into()),
        );

        let report = audit_project_issues(&[issue]);

        assert!(report.is_clean());
    }

    #[test]
    fn skips_done_issue_active_structured_claims_without_registry() {
        let mut issue = with_github_issue_state(issue("#244", "Done"), "CLOSED");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String(
                "v=1 lane=main actor=codex source=manual issue=#244 run=20260516T0415Z-issue244-main-a7f3 state=active thread=unknown registry=run/20260516T0415Z-issue244-main-a7f3".into(),
            ),
        );
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: Vec::new(),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report.is_clean());
    }

    #[test]
    fn skips_done_issue_runtime_and_session_checks() {
        let issue = with_github_issue_state(issue("#202", "Done"), "CLOSED");
        let mut context = runtime_context("#202", 1_000);
        context.runtime_state.as_mut().unwrap().backend_session_id = Some("missing-session".into());
        context.sessions = vec![session(Some("#202"), "stale")];

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report.is_clean());
    }

    #[test]
    fn reports_done_project_item_with_open_github_issue() {
        let issue = with_github_issue_state(issue("#255", "Done"), "OPEN");

        let report = audit_project_issues(&[issue]);

        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].code, "done_project_issue_still_open");
    }

    #[test]
    fn reports_closed_github_issue_without_done_project_status() {
        let issue = with_github_issue_state(issue("#255", "Agent Review"), "CLOSED");

        let report = audit_project_issues(&[issue]);

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "closed_issue_not_done"));
    }

    #[test]
    fn reports_active_structured_claim_missing_registry_evidence() {
        let mut issue = issue("#244", "In Progress");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String(
                "v=1 lane=main actor=codex source=manual issue=#244 run=20260516T0415Z-issue244-main-a7f3 state=active thread=unknown registry=run/20260516T0415Z-issue244-main-a7f3".into(),
            ),
        );
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: Vec::new(),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "active_lane_claim_missing_registry"));
    }

    #[test]
    fn accepts_active_structured_claim_with_manual_registry_evidence() {
        let run = "20260516T0415Z-issue244-main-a7f3";
        let mut issue = issue("#244", "In Progress");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String(
                format!("v=1 lane=main actor=codex source=manual issue=#244 run={run} state=active thread=unknown registry=run/{run}"),
            ),
        );
        let mut session = session(Some("#244"), "recorded");
        session.run_id = Some(run.into());
        session.session_id = format!("manual-main-{run}");
        session.attach_command = None;
        session.log_path = None;
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: vec![session],
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(!report
            .violations
            .iter()
            .any(|violation| violation.code == "active_lane_claim_missing_registry"));
    }

    #[test]
    fn reports_terminal_structured_claim_missing_registry_as_guidance() {
        let mut issue = issue("#244", "In Progress");
        issue.project_fields.insert(
            "Main Agent".into(),
            serde_json::Value::String(
                "v=1 lane=main actor=codex source=manual issue=#244 run=20260516T0415Z-issue244-main-a7f3 state=done thread=unknown registry=run/20260516T0415Z-issue244-main-a7f3".into(),
            ),
        );
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: Vec::new(),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report.violations.iter().any(|violation| {
            violation.code == "terminal_lane_claim_missing_registry"
                && violation.severity == AuditSeverity::Warning
        }));
        assert!(!report
            .violations
            .iter()
            .any(|violation| violation.code == "active_lane_claim_missing_registry"));
    }

    #[test]
    fn reports_stale_runtime_state_for_in_progress_issue() {
        let issue = issue("#202", "In Progress");
        let context = runtime_context("#202", 1_000);

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "runtime_state_stale"));
    }

    #[test]
    fn reports_runtime_state_tracker_mismatch() {
        let issue = issue("#202", "Agent Review");
        let context = runtime_context("#202", 19_000);

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "runtime_state_tracker_mismatch"));
    }

    #[test]
    fn reports_in_progress_with_pr_evidence() {
        let mut issue = issue("#202", "In Progress");
        issue.linked_pull_requests.push(linked_pr(
            "https://github.com/Alive24/jade-symphony/pull/202",
            "OPEN",
        ));

        let report = audit_project_issues(&[issue]);

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "in_progress_has_pr_evidence"));
    }

    #[test]
    fn reports_unsafe_runtime_active_issue_disagreement() {
        let issue = issue("#203", "In Progress");
        let context = runtime_context("#202", 19_000);

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "runtime_active_issue_disagrees"));
    }

    #[test]
    fn accepts_multiple_in_progress_issues_with_matching_runtime_states() {
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: vec![runtime_state("#202", 19_000), runtime_state("#203", 19_000)],
            sessions: Vec::new(),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(
            &[issue("#202", "In Progress"), issue("#203", "In Progress")],
            Some(&context),
        );

        assert!(!report
            .violations
            .iter()
            .any(|violation| violation.code == "runtime_active_issue_disagrees"));
    }

    #[test]
    fn reports_in_progress_issue_missing_matching_runtime_state() {
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: vec![runtime_state("#202", 19_000), runtime_state("#203", 19_000)],
            sessions: Vec::new(),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report =
            audit_project_issues_with_context(&[issue("#204", "In Progress")], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "runtime_active_issue_disagrees"));
    }

    #[test]
    fn reports_stale_tmux_session_for_matching_issue() {
        let issue = issue("#202", "In Progress");
        let mut context = runtime_context("#202", 19_000);
        context.sessions = vec![session(Some("#202"), "stale")];

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "tmux_session_stale"));
    }

    #[test]
    fn reports_app_server_session_attention_without_tmux_recovery_wording() {
        let issue = issue("#202", "In Progress");
        let mut context = runtime_context("#202", 19_000);
        let mut app_server_session = session(Some("#202"), "failed");
        app_server_session.backend = "codex".into();
        app_server_session.attach_command = None;
        app_server_session.log_path = None;
        context.sessions = vec![app_server_session];

        let report = audit_project_issues_with_context(&[issue], Some(&context));
        let violation = report
            .violations
            .iter()
            .find(|violation| violation.code == "tmux_session_needs_operator_attention")
            .expect("expected session attention warning");

        assert!(violation
            .message
            .contains("Registered codex session `jade-main-202-attempt-1-runtime`"));
        assert!(violation.suggestion.contains("runtime artifacts"));
        assert!(!violation.suggestion.contains("attach command"));
    }

    #[test]
    fn reports_runtime_session_missing_registry() {
        let issue = issue("#202", "In Progress");
        let mut context = runtime_context("#202", 19_000);
        context.runtime_state.as_mut().unwrap().backend_session_id = Some("missing-session".into());

        let report = audit_project_issues_with_context(&[issue], Some(&context));

        assert!(report
            .violations
            .iter()
            .any(|violation| violation.code == "runtime_session_missing_registry"));
    }

    #[test]
    fn reports_unattributed_tmux_session() {
        let context = ProjectDoctorContext {
            runtime_state: None,
            runtime_states: Vec::new(),
            sessions: vec![session(None, "running")],
            now_ms: 20_000,
            stale_after_ms: 10_000,
        };

        let report = audit_project_issues_with_context(&[], Some(&context));

        assert_eq!(report.violations[0].code, "tmux_session_missing_issue");
    }

    #[test]
    fn reports_missing_local_skill_root_as_warning() {
        let temp = tempfile::tempdir().unwrap();
        write_skill_suite(
            temp.path(),
            &[("jade-symphony-doctor", "suite/jade-symphony-doctor")],
        );
        let target = SkillInstallTarget {
            label: "Codex".into(),
            root: temp.path().join("missing-codex-skills"),
        };
        let mut report = ProjectAuditReport {
            total_issues: 0,
            violations: Vec::new(),
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };

        append_local_skill_install_doctor_violations(&mut report, temp.path(), &[target]);

        assert_eq!(report.violations.len(), 1);
        assert_eq!(report.violations[0].severity, AuditSeverity::Warning);
        assert_eq!(report.violations[0].code, "local_skill_root_missing");
        assert!(report.violations[0]
            .suggestion
            .contains("install-jade-symphony-skills.js --dry-run"));
    }

    #[test]
    fn reports_unhealthy_local_skill_shapes_and_stale_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let skills = [
            ("jade-symphony-alias-file", "suite/jade-symphony-alias-file"),
            ("jade-symphony-missing-md", "suite/jade-symphony-missing-md"),
            ("jade-symphony-stale", "suite/jade-symphony-stale"),
            ("jade-symphony-file-link", "suite/jade-symphony-file-link"),
            (
                "jade-symphony-broken-link",
                "suite/jade-symphony-broken-link",
            ),
        ];
        write_skill_suite(temp.path(), &skills);
        let target_root = temp.path().join("codex-skills");
        fs::create_dir_all(&target_root).unwrap();

        fs::write(target_root.join("jade-symphony-alias-file"), "alias").unwrap();
        fs::create_dir_all(target_root.join("jade-symphony-missing-md")).unwrap();
        fs::create_dir_all(target_root.join("jade-symphony-stale")).unwrap();
        fs::write(
            target_root
                .join("jade-symphony-stale")
                .join("SKILL.md"),
            "---\nname: stale-jade-skill\nmetadata:\n  suite-version: 2026.01.01\n---\nUse Jade CLI here.\n",
        )
        .unwrap();
        let file_target = temp.path().join("target-SKILL.md");
        fs::write(
            &file_target,
            skill_text("jade-symphony-file-link", "2026.05.17"),
        )
        .unwrap();
        symlink_file(&file_target, &target_root.join("jade-symphony-file-link"));
        symlink_file(
            &temp.path().join("does-not-exist"),
            &target_root.join("jade-symphony-broken-link"),
        );

        let target = SkillInstallTarget {
            label: "Codex".into(),
            root: target_root,
        };
        let mut report = ProjectAuditReport {
            total_issues: 0,
            violations: Vec::new(),
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };

        append_local_skill_install_doctor_violations(&mut report, temp.path(), &[target]);

        let codes = report
            .violations
            .iter()
            .map(|violation| violation.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"local_skill_expected_directory_file"));
        assert!(codes.contains(&"local_skill_missing_skill_md"));
        assert!(codes.contains(&"local_skill_stale_name"));
        assert!(codes.contains(&"local_skill_stale_suite_version"));
        assert!(codes.contains(&"local_skill_stale_cli_naming"));
        assert!(codes.contains(&"local_skill_symlink_targets_file"));
        assert!(codes.contains(&"local_skill_broken_symlink"));
    }

    #[test]
    fn accepts_healthy_local_skill_directory() {
        let temp = tempfile::tempdir().unwrap();
        write_skill_suite(
            temp.path(),
            &[("jade-symphony-doctor", "suite/jade-symphony-doctor")],
        );
        let target_root = temp.path().join("gemini-skills");
        let destination = target_root.join("jade-symphony-doctor");
        fs::create_dir_all(&destination).unwrap();
        fs::write(
            destination.join("SKILL.md"),
            skill_text("jade-symphony-doctor", "2026.05.17"),
        )
        .unwrap();
        let target = SkillInstallTarget {
            label: "Gemini".into(),
            root: target_root,
        };
        let mut report = ProjectAuditReport {
            total_issues: 0,
            violations: Vec::new(),
            integration_gaps: Vec::new(),
            skill_readiness_summary: None,
        };

        append_local_skill_install_doctor_violations(&mut report, temp.path(), &[target]);

        assert!(report.violations.is_empty());
    }

    fn write_skill_suite(repo_root: &Path, skills: &[(&str, &str)]) {
        let suite_root = repo_root.join("skills").join("jade-symphony");
        fs::create_dir_all(&suite_root).unwrap();
        let mut manifest =
            "suite_name = \"Jade Symphony skill suite\"\nversion = \"2026.05.17\"\n".to_string();
        for (name, path) in skills {
            manifest.push_str(&format!(
                "\n[[skills]]\nname = \"{name}\"\npath = \"{path}\"\n"
            ));
            let skill_dir = suite_root.join(path);
            fs::create_dir_all(&skill_dir).unwrap();
            fs::write(skill_dir.join("SKILL.md"), skill_text(name, "2026.05.17")).unwrap();
        }
        fs::write(suite_root.join("manifest.toml"), manifest).unwrap();
    }

    fn skill_text(name: &str, version: &str) -> String {
        format!(
            "---\nname: {name}\nmetadata:\n  suite-version: {version}\n---\nUse the Jade Symphony CLI.\n"
        )
    }

    #[cfg(unix)]
    fn symlink_file(source: &Path, destination: &Path) {
        std::os::unix::fs::symlink(source, destination).unwrap();
    }

    #[cfg(windows)]
    fn symlink_file(source: &Path, destination: &Path) {
        std::os::windows::fs::symlink_file(source, destination).unwrap();
    }
}
