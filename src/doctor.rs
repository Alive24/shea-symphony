use serde::{Deserialize, Serialize};

use crate::model::{normalize_state, LinkedPullRequest, TrackerIssue};

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

pub fn audit_project_issues(issues: &[TrackerIssue]) -> ProjectAuditReport {
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
            "in progress"
                if !issue.project_fields.contains_key("runtime_owner")
                    && !issue.project_fields.contains_key("runtime_state") =>
            {
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
    }

    ProjectAuditReport {
        total_issues: issues.len(),
        violations,
        integration_gaps: Vec::new(),
    }
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
}

fn has_dirty_or_conflicted_pr(issue: &TrackerIssue) -> bool {
    string_project_field(issue, "pr_merge_state")
        .map(|state| {
            let state = normalize_state(&state);
            state == "dirty" || state == "blocked" || state == "behind" || state == "conflicted"
        })
        .unwrap_or(false)
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
}
