use serde::{Deserialize, Serialize};

use crate::model::{normalize_state, LinkedPullRequest, TrackerIssue};
use crate::runtime_state::{detect_runtime_stall, RuntimeState};

pub const HUMAN_REVIEW_MISSING_REVIEW_EVIDENCE: &str = "human_review_missing_review_evidence";

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
                    "Record exactly one PR link in the Project field, issue closing reference, or Jade workpad before attempting to land.",
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
            "done" if has_open_pr(issue) => {
                violations.push(violation(
                    issue,
                    AuditSeverity::Warning,
                    "done_issue_has_open_pr",
                    "Done issue still has an open linked PR.",
                    "Confirm whether the PR should be merged, closed, or unlinked.",
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

        if state == "todo" && claimed_main_agent(issue).is_some() {
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

    ProjectAuditReport {
        total_issues: issues.len(),
        violations,
        integration_gaps: Vec::new(),
    }
}

pub fn render_doctor_repair_workpad(
    issue: &TrackerIssue,
    report: &ProjectAuditReport,
    action: &str,
) -> String {
    let related = related_violations(report, &issue.identifier);
    let mut lines = vec![
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Project Doctor Repair".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Current state: `{}`", issue.state),
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
    }

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

pub fn render_human_review_repair_workpad(violation: &ProjectAuditViolation) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Project Doctor Repair".to_string(),
        format!("- Issue: {} {}", violation.issue_ref, violation.title),
        format!("- Violation: `{}`", violation.code),
        format!("- Previous state: `{}`", violation.state),
        format!("- Message: {}", violation.message),
        format!("- Repair: {}", violation.suggestion),
        String::new(),
        "### State Boundary".to_string(),
        "- Main implementation agent is moving this issue back to `Agent Review`.".to_string(),
        "- This repair does not set `Human Review`; that state requires independent Review Agent pass evidence.".to_string(),
    ]
    .join("\n")
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

fn has_open_pr(issue: &TrackerIssue) -> bool {
    issue
        .linked_pull_requests
        .iter()
        .any(|pr| match pr_state(pr) {
            Some(state) => state == "open",
            None => true,
        })
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
    let Some(runtime_state) = context.runtime_state.as_ref() else {
        return;
    };
    let Some(active_issue) = runtime_state.active_issue.as_ref() else {
        return;
    };

    let runtime_matches_issue = issue_refs_match(&active_issue.identifier, &issue.identifier);

    if runtime_matches_issue && normalized_issue_state != "in progress" {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "runtime_state_tracker_mismatch",
            "Runtime state still points at this issue, but the tracker state is not In Progress.",
            "Inspect the runtime state and tracker evidence before clearing or resuming the active issue.",
        ));
    }

    if runtime_matches_issue && normalized_issue_state == "in progress" {
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
        && runtime_state.workspace_path.is_some()
    {
        violations.push(violation(
            issue,
            AuditSeverity::Warning,
            "runtime_worktree_tracker_mismatch",
            "Runtime state has a workspace for this queued issue.",
            "Inspect the worktree and either resume the active work or move the issue to Need Human Input with evidence.",
        ));
    }

    if runtime_matches_issue {
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

fn pr_state(pr: &LinkedPullRequest) -> Option<String> {
    pr.state.as_deref().map(normalize_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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

    fn runtime_context(identifier: &str, updated_at_ms: u64) -> ProjectDoctorContext {
        let mut runtime_state = RuntimeState::active(
            crate::runtime_state::RuntimeIssueState {
                id: format!("ISSUE_{identifier}"),
                identifier: identifier.into(),
            },
            "dry-run",
        );
        runtime_state.updated_at_ms = Some(updated_at_ms);
        ProjectDoctorContext {
            runtime_state: Some(runtime_state),
            now_ms: 20_000,
            stale_after_ms: 10_000,
        }
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

        assert!(workpad.contains("Project Doctor Repair"));
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

        assert_eq!(report.violations[0].code, "todo_main_agent_claimed");
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
}
