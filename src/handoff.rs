use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::TrackerIssue;
use crate::workspace::safe_identifier;

const DEFAULT_BRANCH_PREFIX: &str = "feature";
const TITLE_SLUG_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueHandoffPlan {
    pub issue_ref: String,
    pub issue_title: String,
    pub workspace_key: String,
    pub workspace_path: PathBuf,
    pub branch_name: String,
    pub pull_request: PullRequestHandoffPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestHandoffPlan {
    pub title: String,
    pub head_branch: String,
    pub base_branch: String,
    pub issue_ref: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReviewHandoffEvidence {
    pub issue_ref: String,
    pub workspace_key: String,
    pub workspace_path: PathBuf,
    pub branch_name: String,
    pub pull_request_url: Option<String>,
    pub validation_summary: String,
    pub last_transition: String,
    pub no_pr_blocker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentReviewHandoffStatus {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReviewHandoffReport {
    pub status: AgentReviewHandoffStatus,
    pub missing: Vec<String>,
    pub target_state: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HandoffError {
    #[error("branch {branch_name} appears to belong to issue #{found_issue}, expected #{expected_issue}")]
    BranchIssueMismatch {
        branch_name: String,
        expected_issue: String,
        found_issue: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchIssueCheck {
    Matches { issue_number: String },
    Mismatch { expected: String, found: String },
    Unknown,
}

impl AgentReviewHandoffReport {
    pub fn is_ready(&self) -> bool {
        self.status == AgentReviewHandoffStatus::Ready
    }
}

impl AgentReviewHandoffEvidence {
    pub fn from_plan(
        plan: &IssueHandoffPlan,
        validation_summary: impl Into<String>,
        last_transition: impl Into<String>,
    ) -> Self {
        Self {
            issue_ref: plan.issue_ref.clone(),
            workspace_key: plan.workspace_key.clone(),
            workspace_path: plan.workspace_path.clone(),
            branch_name: plan.branch_name.clone(),
            pull_request_url: None,
            validation_summary: validation_summary.into(),
            last_transition: last_transition.into(),
            no_pr_blocker: None,
        }
    }
}

pub fn evaluate_agent_review_handoff(
    evidence: &AgentReviewHandoffEvidence,
) -> AgentReviewHandoffReport {
    let mut missing = Vec::new();

    if evidence.issue_ref.trim().is_empty() {
        missing.push("issue id".into());
    }
    if evidence.workspace_key.trim().is_empty() {
        missing.push("workspace key".into());
    }
    if evidence.branch_name.trim().is_empty() {
        missing.push("branch name".into());
    }
    if evidence.validation_summary.trim().is_empty() {
        missing.push("validation summary".into());
    }
    if evidence.last_transition.trim().is_empty() {
        missing.push("last transition".into());
    }

    let has_pr = evidence
        .pull_request_url
        .as_deref()
        .map(|url| !url.trim().is_empty())
        .unwrap_or(false);
    let has_no_pr_blocker = evidence
        .no_pr_blocker
        .as_deref()
        .map(|blocker| !blocker.trim().is_empty())
        .unwrap_or(false);

    if !has_pr && !has_no_pr_blocker {
        missing.push("pull request url or explicit no-PR blocker".into());
    }

    if missing.is_empty() && has_pr {
        AgentReviewHandoffReport {
            status: AgentReviewHandoffStatus::Ready,
            missing,
            target_state: Some("agent_review".into()),
            message: "Agent Review handoff invariant passed.".into(),
        }
    } else {
        AgentReviewHandoffReport {
            status: AgentReviewHandoffStatus::Blocked,
            missing,
            target_state: Some("need_human_input".into()),
            message: "Agent Review handoff invariant is not satisfied; keeping issue out of Agent Review.".into(),
        }
    }
}

pub fn render_agent_review_handoff_workpad(
    issue: &TrackerIssue,
    evidence: &AgentReviewHandoffEvidence,
    report: &AgentReviewHandoffReport,
) -> String {
    let mut lines = vec![
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Agent Review Handoff Invariant".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Status: `{:?}`", report.status),
        format!("- Message: {}", report.message),
        format!(
            "- Target state: `{}`",
            report.target_state.as_deref().unwrap_or("none")
        ),
        String::new(),
        "### Evidence".to_string(),
        format!("- Workspace key: `{}`", evidence.workspace_key),
        format!("- Workspace path: `{}`", evidence.workspace_path.display()),
        format!("- Branch: `{}`", evidence.branch_name),
        format!(
            "- Pull request: `{}`",
            evidence.pull_request_url.as_deref().unwrap_or("missing")
        ),
        format!("- Validation: {}", evidence.validation_summary),
        format!("- Last transition: {}", evidence.last_transition),
    ];

    if let Some(blocker) = &evidence.no_pr_blocker {
        lines.push(format!("- No-PR blocker: {}", blocker));
    }

    if !report.missing.is_empty() {
        lines.push(String::new());
        lines.push("### Missing Handoff Evidence".into());
        for item in &report.missing {
            lines.push(format!("- {item}"));
        }
    }

    lines.push(String::new());
    lines.push("### Boundary".into());
    lines.push("- Main implementation agent may move complete work to `Agent Review` only after this invariant passes.".into());
    lines.push("- Main implementation agent must never set `Human Review`.".into());

    lines.join("\n")
}

pub fn plan_issue_handoff(
    workspace_root: &Path,
    issue: &TrackerIssue,
    base_branch: &str,
) -> Result<IssueHandoffPlan, HandoffError> {
    if let Some(existing_branch) = issue.branch_name.as_deref() {
        guard_branch_for_issue(existing_branch, &issue.identifier)?;
    }

    let branch_name = branch_name_for_issue(&issue.identifier, &issue.title);
    let workspace_key = workspace_key_for_issue(&issue.identifier, &issue.title);
    let workspace_path = workspace_root.join(&workspace_key);
    let pull_request =
        PullRequestHandoffPlan::new(&issue.identifier, &issue.title, &branch_name, base_branch);

    Ok(IssueHandoffPlan {
        issue_ref: issue.identifier.clone(),
        issue_title: issue.title.clone(),
        workspace_key,
        workspace_path,
        branch_name,
        pull_request,
    })
}

pub fn workspace_key_for_issue(issue_identifier: &str, title: &str) -> String {
    safe_identifier(&format!(
        "{}-{}",
        issue_slug(issue_identifier),
        title_slug(title)
    ))
}

pub fn branch_name_for_issue(issue_identifier: &str, title: &str) -> String {
    format!(
        "{}/{}-{}",
        DEFAULT_BRANCH_PREFIX,
        issue_slug(issue_identifier),
        title_slug(title)
    )
}

pub fn guard_branch_for_issue(
    branch_name: &str,
    issue_identifier: &str,
) -> Result<BranchIssueCheck, HandoffError> {
    match check_branch_issue(branch_name, issue_identifier) {
        BranchIssueCheck::Mismatch { expected, found } => Err(HandoffError::BranchIssueMismatch {
            branch_name: branch_name.into(),
            expected_issue: expected,
            found_issue: found,
        }),
        check => Ok(check),
    }
}

pub fn check_branch_issue(branch_name: &str, issue_identifier: &str) -> BranchIssueCheck {
    let expected = issue_number(issue_identifier);
    let found = branch_issue_number(branch_name);

    match (expected, found) {
        (Some(expected), Some(found)) if expected == found => BranchIssueCheck::Matches {
            issue_number: found,
        },
        (Some(expected), Some(found)) => BranchIssueCheck::Mismatch { expected, found },
        _ => BranchIssueCheck::Unknown,
    }
}

pub fn branch_issue_number(branch_name: &str) -> Option<String> {
    let tokens: Vec<_> = branch_name
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();

    for (index, token) in tokens.iter().enumerate() {
        if token.eq_ignore_ascii_case("issue") {
            if let Some(next) = tokens.get(index + 1).and_then(|value| digits_only(value)) {
                return Some(next);
            }
        }

        let normalized = token.to_ascii_lowercase();
        if let Some(suffix) = normalized.strip_prefix("issue").and_then(digits_only) {
            return Some(suffix);
        }
    }

    None
}

pub fn issue_number(issue_identifier: &str) -> Option<String> {
    issue_identifier
        .split(|ch: char| !ch.is_ascii_digit())
        .find_map(digits_only)
}

impl PullRequestHandoffPlan {
    pub fn new(issue_ref: &str, issue_title: &str, head_branch: &str, base_branch: &str) -> Self {
        let title = format!("{}: {}", issue_ref.trim(), issue_title.trim());
        let body = render_pull_request_body(issue_ref, issue_title);

        Self {
            title,
            head_branch: head_branch.into(),
            base_branch: base_branch.into(),
            issue_ref: issue_ref.into(),
            body,
        }
    }
}

fn render_pull_request_body(issue_ref: &str, issue_title: &str) -> String {
    format!(
        "\
## Summary
- Implements {issue_ref}: {issue_title}.

## Verification
- cargo test
- cargo fmt --check
- cargo clippy --all-targets --all-features -- -D warnings

## Handoff
Main implementation work stops at Agent Review. Human Review remains reserved for an independent Review Agent after passing evidence is recorded.

Closes {issue_ref}
"
    )
}

fn issue_slug(issue_identifier: &str) -> String {
    issue_number(issue_identifier)
        .map(|number| format!("issue-{number}"))
        .unwrap_or_else(|| title_slug(issue_identifier))
}

fn title_slug(title: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_dash = false;

    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            previous_was_dash = false;
        } else if !previous_was_dash && !slug.is_empty() {
            slug.push('-');
            previous_was_dash = true;
        }

        if slug.len() >= TITLE_SLUG_LIMIT {
            break;
        }
    }

    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "untitled".into()
    } else {
        slug.into()
    }
}

fn digits_only(value: &str) -> Option<String> {
    value
        .chars()
        .all(|ch| ch.is_ascii_digit())
        .then(|| value.to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: "I_21".into(),
            item_id: Some("PVTI_21".into()),
            identifier: "#21".into(),
            title: "Add workspace branch and PR handoff planning foundation".into(),
            description: None,
            url: None,
            state: "In Progress".into(),
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
    fn creates_deterministic_workspace_and_branch_plan() {
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue(), "main").unwrap();

        assert_eq!(
            plan.workspace_key,
            "issue-21-add-workspace-branch-and-pr-handoff-planning-foundation"
        );
        assert_eq!(
            plan.workspace_path,
            PathBuf::from(
                "/tmp/jade-workspaces/issue-21-add-workspace-branch-and-pr-handoff-planning-foundation"
            )
        );
        assert_eq!(
            plan.branch_name,
            "feature/issue-21-add-workspace-branch-and-pr-handoff-planning-foundation"
        );
        assert_eq!(plan.pull_request.head_branch, plan.branch_name);
        assert_eq!(plan.pull_request.base_branch, "main");
    }

    #[test]
    fn detects_matching_and_mismatched_issue_branches() {
        assert_eq!(
            check_branch_issue("feature/issue-21-workspace", "#21"),
            BranchIssueCheck::Matches {
                issue_number: "21".into()
            }
        );
        assert_eq!(
            check_branch_issue("feature/issue-20-auth", "#21"),
            BranchIssueCheck::Mismatch {
                expected: "21".into(),
                found: "20".into()
            }
        );
        assert_eq!(
            check_branch_issue("feature/workspace-planning", "#21"),
            BranchIssueCheck::Unknown
        );
        assert_eq!(
            check_branch_issue("feature/ISSUE21-workspace", "#21"),
            BranchIssueCheck::Matches {
                issue_number: "21".into()
            }
        );
    }

    #[test]
    fn rejects_existing_branch_for_different_issue() {
        let mut issue = issue();
        issue.branch_name = Some("feature/issue-20-auth".into());

        let err = plan_issue_handoff(Path::new("/tmp/workspaces"), &issue, "main").unwrap_err();

        assert_eq!(
            err,
            HandoffError::BranchIssueMismatch {
                branch_name: "feature/issue-20-auth".into(),
                expected_issue: "21".into(),
                found_issue: "20".into(),
            }
        );
    }

    #[test]
    fn renders_pull_request_handoff_body() {
        let pr =
            PullRequestHandoffPlan::new("#21", "Add handoff planning", "feature/issue-21", "main");

        assert_eq!(pr.title, "#21: Add handoff planning");
        assert!(pr.body.contains("Implements #21: Add handoff planning."));
        assert!(pr
            .body
            .contains("cargo clippy --all-targets --all-features -- -D warnings"));
        assert!(pr.body.contains("stops at Agent Review"));
        assert!(pr.body.contains("Closes #21"));
    }

    #[test]
    fn agent_review_handoff_requires_pr_url() {
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue(), "main").unwrap();
        let evidence =
            AgentReviewHandoffEvidence::from_plan(&plan, "cargo test passed", "completed");

        let report = evaluate_agent_review_handoff(&evidence);

        assert!(!report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("need_human_input"));
        assert!(report
            .missing
            .contains(&"pull request url or explicit no-PR blocker".to_string()));
    }

    #[test]
    fn agent_review_handoff_passes_with_pr_url() {
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue(), "main").unwrap();
        let mut evidence =
            AgentReviewHandoffEvidence::from_plan(&plan, "cargo test passed", "completed");
        evidence.pull_request_url = Some("https://github.com/Alive24/jade-symphony/pull/21".into());

        let report = evaluate_agent_review_handoff(&evidence);

        assert!(report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("agent_review"));
    }

    #[test]
    fn agent_review_handoff_workpad_names_missing_pr_evidence() {
        let issue = issue();
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue, "main").unwrap();
        let evidence =
            AgentReviewHandoffEvidence::from_plan(&plan, "cargo test passed", "completed");
        let report = evaluate_agent_review_handoff(&evidence);

        let workpad = render_agent_review_handoff_workpad(&issue, &evidence, &report);

        assert!(workpad.contains("## Jade Symphony Workpad"));
        assert!(workpad.contains("Agent Review Handoff Invariant"));
        assert!(workpad.contains("Pull request: `missing`"));
        assert!(workpad.contains("pull request url or explicit no-PR blocker"));
        assert!(workpad.contains("must never set `Human Review`"));
    }
}
