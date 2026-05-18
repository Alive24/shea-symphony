use serde::{Deserialize, Serialize};

use crate::model::TrackerIssue;
use crate::review::{ReviewFindingClass, ReviewGateDecision, ReviewJob, ReviewOutcome};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReworkDiagnosticKind {
    ReviewFinding,
    InconclusiveReview,
    MergeConflict,
    DirtyPullRequest,
    ValidationFailure,
    RuntimeFailure,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReworkFinding {
    pub class: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReworkDiagnostic {
    pub issue_ref: String,
    pub source: String,
    pub kind: ReworkDiagnosticKind,
    pub summary: String,
    pub next_action: String,
    pub command: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub pr_ref: Option<String>,
    #[serde(default)]
    pub review_artifact_path: Option<String>,
    #[serde(default)]
    pub review_ledger_path: Option<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub findings: Vec<ReworkFinding>,
}

impl ReworkDiagnostic {
    pub fn validation_failure(
        issue_ref: impl Into<String>,
        command: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        Self {
            issue_ref: issue_ref.into(),
            source: "validation".into(),
            kind: ReworkDiagnosticKind::ValidationFailure,
            summary: "Verification failed before handoff.".into(),
            next_action: "Inspect the failing command output, repair within issue scope, and rerun verification.".into(),
            command: Some(command.into()),
            stdout: None,
            stderr: Some(stderr.into()),
            pr_ref: None,
            review_artifact_path: None,
            review_ledger_path: None,
            changed_files: Vec::new(),
            findings: Vec::new(),
        }
    }

    pub fn merge_conflict(
        issue_ref: impl Into<String>,
        pr_ref: impl Into<String>,
        changed_files: Vec<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            issue_ref: issue_ref.into(),
            source: "merging".into(),
            kind: ReworkDiagnosticKind::MergeConflict,
            summary: summary.into(),
            next_action: "Resolve the conflict in the issue branch and preserve review freshness evidence when applicable.".into(),
            command: None,
            stdout: None,
            stderr: None,
            pr_ref: Some(pr_ref.into()),
            review_artifact_path: None,
            review_ledger_path: None,
            changed_files,
            findings: Vec::new(),
        }
    }
}

pub fn rework_diagnostic_from_review(
    issue: &TrackerIssue,
    job: &ReviewJob,
    decision: &ReviewGateDecision,
) -> ReworkDiagnostic {
    let mut findings = Vec::new();
    let mut stdout = None;
    let mut stderr = None;
    let mut summary = decision.message.clone();

    if let Some(report) = &job.report {
        if let Some(report_summary) = &report.summary {
            summary = report_summary.clone();
        }
        stdout = report.stdout.clone();
        stderr = report.stderr.clone();
        findings = report
            .findings
            .iter()
            .filter(|finding| finding.class == ReviewFindingClass::Confirmed)
            .map(|finding| ReworkFinding {
                class: "Confirmed".into(),
                title: finding.title.clone(),
                body: finding.body.clone(),
            })
            .collect();
    }

    let (kind, next_action) = if decision.outcome == ReviewOutcome::InconclusiveNeedsRework {
        (
            ReworkDiagnosticKind::InconclusiveReview,
            "Restore the missing PR/workspace/review evidence, rerun required verification, and hand back to Agent Review.",
        )
    } else {
        (
            ReworkDiagnosticKind::ReviewFinding,
            "Address confirmed review findings, rerun required verification, and hand back to Agent Review.",
        )
    };

    ReworkDiagnostic {
        issue_ref: issue.identifier.clone(),
        source: format!("agent_review:{}", job.backend),
        kind,
        summary,
        next_action: next_action.into(),
        command: None,
        stdout,
        stderr,
        pr_ref: issue
            .linked_pull_requests
            .iter()
            .find_map(|pr| pr.url.clone()),
        review_artifact_path: job
            .artifact_path
            .as_ref()
            .map(|path| path.display().to_string()),
        review_ledger_path: job
            .ledger_path
            .as_ref()
            .map(|path| path.display().to_string()),
        changed_files: Vec::new(),
        findings,
    }
}

pub fn render_rework_diagnostic_workpad(
    issue: &TrackerIssue,
    diagnostic: &ReworkDiagnostic,
) -> String {
    let mut lines = vec![
        "## Rework Diagnostic".to_string(),
        String::new(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Source: `{}`", diagnostic.source),
        format!("- Kind: `{:?}`", diagnostic.kind),
        format!("- Summary: {}", diagnostic.summary),
        format!("- Next action: {}", diagnostic.next_action),
        "- Evidence was recorded before moving the issue to `Rework`.".to_string(),
    ];

    if let Some(pr_ref) = &diagnostic.pr_ref {
        lines.push(format!("- Pull request: `{pr_ref}`"));
        lines.push("- PR-specific context is captured here; mirror this note to the PR conversation when the active adapter supports PR comments.".into());
    }
    if let Some(path) = &diagnostic.review_artifact_path {
        lines.push(format!("- Review artifact: `{path}`"));
    }
    if let Some(path) = &diagnostic.review_ledger_path {
        lines.push(format!("- Review job ledger: `{path}`"));
    }

    if !diagnostic.changed_files.is_empty() {
        lines.push(String::new());
        lines.push("### Changed Files".into());
        for path in &diagnostic.changed_files {
            lines.push(format!("- `{path}`"));
        }
    }

    if !diagnostic.findings.is_empty() {
        lines.push(String::new());
        lines.push("### Findings".into());
        for finding in &diagnostic.findings {
            lines.push(format!(
                "- {}: {} - {}",
                finding.class, finding.title, finding.body
            ));
        }
    }

    if let Some(command) = &diagnostic.command {
        lines.push(String::new());
        lines.push("### Command".into());
        lines.push(format!("- `{command}`"));
    }

    push_log_block(&mut lines, "Stdout", diagnostic.stdout.as_deref());
    push_log_block(&mut lines, "Stderr", diagnostic.stderr.as_deref());

    lines.push(String::new());
    lines.push("### Role Boundary".into());
    lines.push(
        "- Main implementation agent repairs confirmed Rework and then stops at `Agent Review`."
            .into(),
    );
    lines.push(
        "- `Human Review` remains reserved for independent Review Agent pass evidence.".into(),
    );

    lines.join("\n")
}

fn push_log_block(lines: &mut Vec<String>, label: &str, content: Option<&str>) {
    let Some(content) = content else {
        return;
    };
    if content.trim().is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push(format!("### {label}"));
    lines.push("```text".into());
    lines.push(truncate_log(content));
    lines.push("```".into());
}

fn truncate_log(content: &str) -> String {
    const LIMIT: usize = 2_000;
    if content.len() <= LIMIT {
        content.to_string()
    } else {
        format!("{} [... truncated]", &content[..LIMIT])
    }
}

pub fn rework_transition_expected(decision: &ReviewGateDecision) -> bool {
    matches!(
        decision.outcome,
        ReviewOutcome::NeedsRework | ReviewOutcome::InconclusiveNeedsRework
    ) && decision.target_state == Some("rework")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackerIssue;
    use crate::review::{
        AgentReviewReport, ReviewFinding, ReviewFindingClass, ReviewGateDecision, ReviewJob,
        ReviewJobState, ReviewOutcome,
    };

    fn issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: "ISSUE_50".into(),
            item_id: None,
            identifier: "#50".into(),
            title: "Persist Rework diagnostics".into(),
            description: None,
            url: None,
            state: "Agent Review".into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn review_diagnostic_keeps_confirmed_findings_and_logs() {
        let issue = issue();
        let job = ReviewJob {
            id: "review-1".into(),
            issue_ref: "#50".into(),
            backend: "fake-reviewer".into(),
            state: ReviewJobState::Completed,
            artifact_path: Some("/tmp/review-artifact.json".into()),
            ledger_path: Some("/tmp/reviews/jobs/50-review-1.json".into()),
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: vec![ReviewFinding {
                    class: ReviewFindingClass::Confirmed,
                    title: "Missing test".into(),
                    body: "Add ordering coverage.".into(),
                }],
                summary: Some("Confirmed finding requires rework.".into()),
                stdout: Some("review stdout".into()),
                stderr: Some("review stderr".into()),
                exit_status: None,
                session_id: None,
            }),
            error: None,
        };
        let decision = ReviewGateDecision {
            outcome: ReviewOutcome::NeedsRework,
            target_state: Some("rework"),
            message: "Confirmed findings require Rework.".into(),
        };

        let diagnostic = rework_diagnostic_from_review(&issue, &job, &decision);
        let workpad = render_rework_diagnostic_workpad(&issue, &diagnostic);

        assert_eq!(diagnostic.kind, ReworkDiagnosticKind::ReviewFinding);
        assert!(workpad.contains("Evidence was recorded before moving the issue to `Rework`"));
        assert!(workpad.contains("Review artifact: `/tmp/review-artifact.json`"));
        assert!(workpad.contains("Review job ledger: `/tmp/reviews/jobs/50-review-1.json`"));
        assert!(workpad.contains("Confirmed: Missing test - Add ordering coverage."));
        assert!(workpad.contains("review stdout"));
        assert!(workpad.contains("review stderr"));
    }

    #[test]
    fn review_diagnostic_names_inconclusive_review_rework() {
        let issue = issue();
        let job = ReviewJob {
            id: "review-1".into(),
            issue_ref: "#50".into(),
            backend: "gemini-cli".into(),
            state: ReviewJobState::Completed,
            artifact_path: Some("/tmp/review-artifact.md".into()),
            ledger_path: Some("/tmp/reviews/jobs/50-review-1.json".into()),
            report: Some(AgentReviewReport {
                reviewer_backend: "gemini-cli".into(),
                findings: Vec::new(),
                summary: Some("Could not complete review: missing PR evidence.".into()),
                stdout: Some("Could not complete review: missing PR evidence.".into()),
                stderr: None,
                exit_status: None,
                session_id: None,
            }),
            error: None,
        };
        let decision = ReviewGateDecision {
            outcome: ReviewOutcome::InconclusiveNeedsRework,
            target_state: Some("rework"),
            message: "Agent Review was inconclusive and requires Rework.".into(),
        };

        let diagnostic = rework_diagnostic_from_review(&issue, &job, &decision);
        let workpad = render_rework_diagnostic_workpad(&issue, &diagnostic);

        assert_eq!(diagnostic.kind, ReworkDiagnosticKind::InconclusiveReview);
        assert!(workpad.contains("Kind: `InconclusiveReview`"));
        assert!(workpad.contains("Restore the missing PR/workspace/review evidence"));
        assert!(workpad.contains("Could not complete review"));
    }

    #[test]
    fn merge_conflict_diagnostic_records_pr_specific_context() {
        let issue = issue();
        let diagnostic = ReworkDiagnostic::merge_conflict(
            "#50",
            "https://github.com/Alive24/jade-symphony/pull/99",
            vec!["src/main.rs".into()],
            "PR no longer merges cleanly.",
        );

        let workpad = render_rework_diagnostic_workpad(&issue, &diagnostic);

        assert!(
            workpad.contains("Pull request: `https://github.com/Alive24/jade-symphony/pull/99`")
        );
        assert!(workpad.contains("src/main.rs"));
        assert!(workpad.contains("mirror this note to the PR conversation"));
    }

    #[test]
    fn validation_failure_diagnostic_keeps_actionable_stderr() {
        let issue = issue();
        let diagnostic =
            ReworkDiagnostic::validation_failure("#50", "cargo test", "test failure details");

        let workpad = render_rework_diagnostic_workpad(&issue, &diagnostic);

        assert!(workpad.contains("Verification failed before handoff."));
        assert!(workpad.contains("`cargo test`"));
        assert!(workpad.contains("test failure details"));
    }

    #[test]
    fn detects_rework_transition_decisions() {
        let decision = ReviewGateDecision {
            outcome: ReviewOutcome::NeedsRework,
            target_state: Some("rework"),
            message: "confirmed".into(),
        };
        assert!(rework_transition_expected(&decision));

        let decision = ReviewGateDecision {
            outcome: ReviewOutcome::InconclusiveNeedsRework,
            target_state: Some("rework"),
            message: "inconclusive".into(),
        };
        assert!(rework_transition_expected(&decision));
    }
}
