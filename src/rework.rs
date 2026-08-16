use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::TrackerIssue;
use crate::review::{ReviewFindingClass, ReviewGateDecision, ReviewJob, ReviewOutcome};
use crate::workflow::WorkflowDefinition;
use crate::workpad_templates::{render_workpad_template, WorkpadTemplateId};

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
    workflow: Option<&WorkflowDefinition>,
    issue: &TrackerIssue,
    diagnostic: &ReworkDiagnostic,
) -> Result<String, crate::prompt::PromptError> {
    const RECORD_SEPARATOR: &str = "\u{1e}";
    const FIELD_SEPARATOR: &str = "\u{1f}";
    let review_origin = diagnostic.source.starts_with("agent_review:");
    let lane = if review_origin { "review" } else { "main" };
    let findings = diagnostic
        .findings
        .iter()
        .map(|finding| {
            format!(
                "{}{}{}{}{}",
                finding.class, FIELD_SEPARATOR, finding.title, FIELD_SEPARATOR, finding.body
            )
        })
        .collect::<Vec<_>>()
        .join(RECORD_SEPARATOR);
    render_workpad_template(
        workflow,
        WorkpadTemplateId::ReworkDiagnostic,
        &[
            ("review_origin", review_origin.to_string()),
            ("generated_at", current_gmt_timestamp()),
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            ("lane", lane.into()),
            (
                "actor_role",
                if review_origin {
                    "review_agent"
                } else {
                    "implementation_agent"
                }
                .into(),
            ),
            ("source", diagnostic.source.clone()),
            ("input_state", issue.state.clone()),
            ("kind", format!("{:?}", diagnostic.kind)),
            ("summary", diagnostic.summary.clone()),
            ("next_action", diagnostic.next_action.clone()),
            (
                "changed_file_count",
                diagnostic.changed_files.len().to_string(),
            ),
            ("finding_count", diagnostic.findings.len().to_string()),
            ("pr_ref", diagnostic.pr_ref.clone().unwrap_or_default()),
            (
                "review_artifact_path",
                diagnostic.review_artifact_path.clone().unwrap_or_default(),
            ),
            (
                "review_ledger_path",
                diagnostic.review_ledger_path.clone().unwrap_or_default(),
            ),
            (
                "changed_files",
                diagnostic.changed_files.join(RECORD_SEPARATOR),
            ),
            ("findings", findings),
            ("command", diagnostic.command.clone().unwrap_or_default()),
            (
                "stdout",
                diagnostic
                    .stdout
                    .as_deref()
                    .map(truncate_log)
                    .unwrap_or_default(),
            ),
            (
                "stderr",
                diagnostic
                    .stderr
                    .as_deref()
                    .map(truncate_log)
                    .unwrap_or_default(),
            ),
            ("record_separator", RECORD_SEPARATOR.into()),
            ("field_separator", FIELD_SEPARATOR.into()),
        ],
    )
}

fn truncate_log(content: &str) -> String {
    const LIMIT: usize = 2_000;
    if content.len() <= LIMIT {
        content.to_string()
    } else {
        format!("{} [... truncated]", &content[..LIMIT])
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
            backend_session_id: None,
            report: Some(AgentReviewReport {
                reviewer_backend: "fake-reviewer".into(),
                findings: vec![ReviewFinding {
                    class: ReviewFindingClass::Confirmed,
                    title: "Missing test".into(),
                    body: "Add ordering coverage.".into(),
                    severity: None,
                    file: None,
                    line: None,
                    evidence: None,
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
        let workpad = render_rework_diagnostic_workpad(None, &issue, &diagnostic).unwrap();

        assert_eq!(diagnostic.kind, ReworkDiagnosticKind::ReviewFinding);
        assert!(workpad.contains("Evidence was recorded before moving the issue to `Rework`"));
        assert!(workpad.contains("Review artifact: `/tmp/review-artifact.json`"));
        assert!(workpad.contains("Review job ledger: `/tmp/reviews/jobs/50-review-1.json`"));
        assert!(workpad.contains("Confirmed: Missing test - Add ordering coverage."));
        assert!(workpad.contains("does not replace the canonical Main Agent Workpad"));
        assert!(workpad.contains("repairs confirmed Rework in the existing Main Agent Workpad"));
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
            backend_session_id: None,
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
        let workpad = render_rework_diagnostic_workpad(None, &issue, &diagnostic).unwrap();

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
            "https://github.com/Alive24/shea-symphony/pull/99",
            vec!["src/main.rs".into()],
            "PR no longer merges cleanly.",
        );

        let workpad = render_rework_diagnostic_workpad(None, &issue, &diagnostic).unwrap();

        assert!(
            workpad.contains("Pull request: `https://github.com/Alive24/shea-symphony/pull/99`")
        );
        assert!(workpad.contains("src/main.rs"));
        assert!(workpad.contains("mirror this note to the PR conversation"));
    }

    #[test]
    fn validation_failure_diagnostic_keeps_actionable_stderr() {
        let issue = issue();
        let diagnostic =
            ReworkDiagnostic::validation_failure("#50", "cargo test", "test failure details");

        let workpad = render_rework_diagnostic_workpad(None, &issue, &diagnostic).unwrap();

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
