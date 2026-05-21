use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{LinkedPullRequest, TrackerIssue};
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
    pub continuation: Option<ReworkContinuationEvidence>,
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
pub struct ReworkContinuationEvidence {
    pub pull_request_url: String,
    pub pull_request_state: String,
    pub branch_name: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReviewHandoffEvidence {
    pub issue_ref: String,
    pub workspace_key: String,
    pub workspace_path: PathBuf,
    pub branch_name: String,
    pub pull_request_url: Option<String>,
    pub project_pr_link_verified: Option<bool>,
    pub pull_request_is_draft: Option<bool>,
    pub main_workpad_has_plan: Option<bool>,
    pub main_workpad_has_work_log: Option<bool>,
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
    #[error("issue {issue_ref} has multiple open pull request candidates: {candidates:?}")]
    AmbiguousReworkContinuation {
        issue_ref: String,
        candidates: Vec<String>,
    },
    #[error(
        "issue {issue_ref} has stale pull request evidence {pull_request_url} in state {state}"
    )]
    StaleReworkContinuation {
        issue_ref: String,
        pull_request_url: String,
        state: String,
    },
    #[error("issue {issue_ref} has pull request {pull_request_url} but no safe branch/workspace evidence")]
    MissingReworkContinuationBranch {
        issue_ref: String,
        pull_request_url: String,
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
            project_pr_link_verified: None,
            pull_request_is_draft: None,
            main_workpad_has_plan: None,
            main_workpad_has_work_log: None,
            validation_summary: validation_summary.into(),
            last_transition: last_transition.into(),
            no_pr_blocker: None,
        }
    }

    pub fn record_main_workpad_markdown(&mut self, markdown: Option<&str>) {
        let Some(markdown) = markdown else {
            self.main_workpad_has_plan = Some(false);
            self.main_workpad_has_work_log = Some(false);
            return;
        };

        self.main_workpad_has_plan = Some(markdown_has_heading(markdown, "### Plan"));
        self.main_workpad_has_work_log = Some(markdown_has_heading(markdown, "### Work Log"));
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
    match evidence.main_workpad_has_plan {
        Some(true) => {}
        Some(false) => missing.push("Main Workpad `### Plan`".into()),
        None => missing.push("Main Workpad `### Plan` evidence".into()),
    }
    match evidence.main_workpad_has_work_log {
        Some(true) => {}
        Some(false) => missing.push("Main Workpad `### Work Log`".into()),
        None => missing.push("Main Workpad `### Work Log` evidence".into()),
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
    if has_pr {
        match evidence.project_pr_link_verified {
            Some(true) => {}
            Some(false) => missing.push("Project-visible pull request linkage".into()),
            None => missing.push("Project-visible pull request linkage status".into()),
        }
        match evidence.pull_request_is_draft {
            Some(false) => {}
            Some(true) => missing.push("non-draft pull request".into()),
            None => missing.push("pull request draft status".into()),
        }
    }

    if missing.is_empty()
        && has_pr
        && evidence.project_pr_link_verified == Some(true)
        && evidence.pull_request_is_draft == Some(false)
        && evidence.main_workpad_has_plan == Some(true)
        && evidence.main_workpad_has_work_log == Some(true)
    {
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
        format!(
            "- Project PR linkage verified: `{}`",
            evidence
                .project_pr_link_verified
                .map(|verified| verified.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
        format!(
            "- Pull request draft: `{}`",
            evidence
                .pull_request_is_draft
                .map(|is_draft| is_draft.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
        format!(
            "- Main Workpad has `### Plan`: `{}`",
            evidence
                .main_workpad_has_plan
                .map(|present| present.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
        format!(
            "- Main Workpad has `### Work Log`: `{}`",
            evidence
                .main_workpad_has_work_log
                .map(|present| present.to_string())
                .unwrap_or_else(|| "unknown".into())
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

fn markdown_has_heading(markdown: &str, heading: &str) -> bool {
    markdown.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == heading
            || trimmed
                .strip_prefix(heading)
                .is_some_and(|rest| rest.starts_with(' '))
    })
}

pub fn plan_issue_handoff(
    workspace_root: &Path,
    issue: &TrackerIssue,
    base_branch: &str,
) -> Result<IssueHandoffPlan, HandoffError> {
    plan_issue_handoff_for_profile(workspace_root, issue, base_branch, None)
}

pub fn plan_issue_handoff_for_profile(
    workspace_root: &Path,
    issue: &TrackerIssue,
    base_branch: &str,
    profile_id: Option<&str>,
) -> Result<IssueHandoffPlan, HandoffError> {
    let continuation = rework_continuation_evidence(issue)?;
    let is_rework = issue.normalized_state() == "rework";
    if let Some(existing_branch) = issue.branch_name.as_deref() {
        guard_branch_for_issue(existing_branch, &issue.identifier)?;
    }

    let branch_name = match issue.branch_name.as_deref() {
        Some(existing_branch) if is_rework => existing_branch.to_string(),
        None if let Some(continuation) = &continuation => continuation
            .branch_name
            .as_deref()
            .map(str::trim)
            .filter(|branch_name| !branch_name.is_empty())
            .map(|branch_name| {
                guard_branch_for_issue(branch_name, &issue.identifier)?;
                Ok::<_, HandoffError>(branch_name.to_string())
            })
            .transpose()?
            .ok_or_else(|| HandoffError::MissingReworkContinuationBranch {
                issue_ref: issue.identifier.clone(),
                pull_request_url: continuation.pull_request_url.clone(),
            })?,
        _ => branch_name_for_issue(&issue.identifier, &issue.title),
    };

    let workspace_key =
        profile_workspace_key_for_issue(profile_id, &issue.identifier, &issue.title);
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
        continuation,
    })
}

fn rework_continuation_evidence(
    issue: &TrackerIssue,
) -> Result<Option<ReworkContinuationEvidence>, HandoffError> {
    if issue.normalized_state() != "rework" || issue.linked_pull_requests.is_empty() {
        return Ok(None);
    }

    let mut open = issue
        .linked_pull_requests
        .iter()
        .filter(|pull_request| pull_request_is_open(pull_request))
        .collect::<Vec<_>>();

    match open.len() {
        0 => {
            let pull_request = &issue.linked_pull_requests[0];
            Err(HandoffError::StaleReworkContinuation {
                issue_ref: issue.identifier.clone(),
                pull_request_url: pull_request_url(pull_request),
                state: pull_request
                    .state
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
            })
        }
        1 => {
            let pull_request = open.remove(0);
            let branch_name = pull_request
                .head_ref_name
                .as_deref()
                .map(str::trim)
                .filter(|branch_name| !branch_name.is_empty())
                .map(ToOwned::to_owned);
            let source = if branch_name.is_some() {
                "linked_pull_request_head_ref"
            } else {
                "linked_pull_request"
            };
            Ok(Some(ReworkContinuationEvidence {
                pull_request_url: pull_request_url(pull_request),
                pull_request_state: pull_request
                    .state
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
                branch_name,
                source: source.into(),
            }))
        }
        _ => Err(HandoffError::AmbiguousReworkContinuation {
            issue_ref: issue.identifier.clone(),
            candidates: open.into_iter().map(pull_request_url).collect(),
        }),
    }
}

fn pull_request_is_open(pull_request: &LinkedPullRequest) -> bool {
    pull_request
        .state
        .as_deref()
        .map(|state| state.eq_ignore_ascii_case("open"))
        .unwrap_or(false)
}

fn pull_request_url(pull_request: &LinkedPullRequest) -> String {
    pull_request
        .url
        .clone()
        .or_else(|| {
            pull_request
                .number
                .map(|number| format!("pull request #{number}"))
        })
        .unwrap_or_else(|| "unknown pull request".into())
}

pub fn workspace_key_for_issue(issue_identifier: &str, title: &str) -> String {
    safe_identifier(&format!(
        "{}-{}",
        issue_slug(issue_identifier),
        title_slug(title)
    ))
}

pub fn profile_workspace_key_for_issue(
    profile_id: Option<&str>,
    issue_identifier: &str,
    title: &str,
) -> String {
    match profile_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(profile_id) => safe_identifier(&format!(
            "{}--{}",
            safe_identifier(profile_id),
            workspace_key_for_issue(issue_identifier, title)
        )),
        None => workspace_key_for_issue(issue_identifier, title),
    }
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

    fn linked_pr(number: u64, state: &str) -> LinkedPullRequest {
        LinkedPullRequest {
            id: Some(format!("PR_{number}")),
            number: Some(number),
            url: Some(format!(
                "https://github.com/Alive24/jade-symphony/pull/{number}"
            )),
            state: Some(state.into()),
            ..Default::default()
        }
    }

    fn linked_pr_with_head(number: u64, state: &str, head_ref_name: &str) -> LinkedPullRequest {
        LinkedPullRequest {
            head_ref_name: Some(head_ref_name.into()),
            ..linked_pr(number, state)
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
    fn profile_workspace_keys_avoid_worker_collisions() {
        assert_eq!(
            profile_workspace_key_for_issue(Some("codex-alpha"), "#39", "Add execution profiles"),
            "codex-alpha--issue-39-add-execution-profiles"
        );

        let plan = plan_issue_handoff_for_profile(
            Path::new("/tmp/jade-workspaces"),
            &issue(),
            "main",
            Some("codex-alpha"),
        )
        .unwrap();
        assert!(plan.workspace_key.starts_with("codex-alpha--issue-21"));
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
    fn reuses_existing_branch_for_rework_continuation() {
        let mut issue = issue();
        issue.state = "Rework".into();
        issue.branch_name = Some("feature/issue-21-existing-work".into());
        issue.linked_pull_requests = vec![linked_pr(45, "OPEN")];

        let plan = plan_issue_handoff(Path::new("/tmp/workspaces"), &issue, "main").unwrap();

        assert_eq!(plan.branch_name, "feature/issue-21-existing-work");
        assert_eq!(
            plan.pull_request.head_branch,
            "feature/issue-21-existing-work"
        );
        assert_eq!(
            plan.continuation
                .as_ref()
                .map(|continuation| continuation.pull_request_url.as_str()),
            Some("https://github.com/Alive24/jade-symphony/pull/45")
        );
    }

    #[test]
    fn reuses_linked_pull_request_head_branch_for_rework_continuation() {
        let mut issue = issue();
        issue.state = "Rework".into();
        issue.linked_pull_requests = vec![linked_pr_with_head(
            45,
            "OPEN",
            "feature/issue-21-existing-work",
        )];

        let plan = plan_issue_handoff(Path::new("/tmp/workspaces"), &issue, "main").unwrap();

        assert_eq!(plan.branch_name, "feature/issue-21-existing-work");
        assert_eq!(
            plan.pull_request.head_branch,
            "feature/issue-21-existing-work"
        );
        assert_eq!(
            plan.continuation
                .as_ref()
                .and_then(|continuation| continuation.branch_name.as_deref()),
            Some("feature/issue-21-existing-work")
        );
        assert_eq!(
            plan.continuation
                .as_ref()
                .map(|continuation| continuation.source.as_str()),
            Some("linked_pull_request_head_ref")
        );
    }

    #[test]
    fn rejects_rework_linked_pull_request_head_for_different_issue() {
        let mut issue = issue();
        issue.state = "Rework".into();
        issue.linked_pull_requests = vec![linked_pr_with_head(45, "OPEN", "feature/issue-20-auth")];

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
    fn blocks_rework_with_multiple_open_pull_requests() {
        let mut issue = issue();
        issue.state = "Rework".into();
        issue.branch_name = Some("feature/issue-21-existing-work".into());
        issue.linked_pull_requests = vec![linked_pr(45, "OPEN"), linked_pr(46, "OPEN")];

        let err = plan_issue_handoff(Path::new("/tmp/workspaces"), &issue, "main").unwrap_err();

        assert!(matches!(
            err,
            HandoffError::AmbiguousReworkContinuation { .. }
        ));
    }

    #[test]
    fn blocks_rework_with_stale_pull_request() {
        let mut issue = issue();
        issue.state = "Rework".into();
        issue.branch_name = Some("feature/issue-21-existing-work".into());
        issue.linked_pull_requests = vec![linked_pr(45, "MERGED")];

        let err = plan_issue_handoff(Path::new("/tmp/workspaces"), &issue, "main").unwrap_err();

        assert_eq!(
            err,
            HandoffError::StaleReworkContinuation {
                issue_ref: "#21".into(),
                pull_request_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                state: "MERGED".into(),
            }
        );
    }

    #[test]
    fn blocks_rework_with_pr_but_no_branch_evidence() {
        let mut issue = issue();
        issue.state = "Rework".into();
        issue.linked_pull_requests = vec![linked_pr(45, "OPEN")];

        let err = plan_issue_handoff(Path::new("/tmp/workspaces"), &issue, "main").unwrap_err();

        assert_eq!(
            err,
            HandoffError::MissingReworkContinuationBranch {
                issue_ref: "#21".into(),
                pull_request_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
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
        evidence.project_pr_link_verified = Some(true);
        evidence.pull_request_is_draft = Some(false);
        evidence.record_main_workpad_markdown(Some(
            "## Jade Symphony Workpad\n\n### Plan\n\n### Work Log",
        ));

        let report = evaluate_agent_review_handoff(&evidence);

        assert!(report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("agent_review"));
    }

    #[test]
    fn agent_review_handoff_blocks_unverified_project_pr_linkage() {
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue(), "main").unwrap();
        let mut evidence =
            AgentReviewHandoffEvidence::from_plan(&plan, "cargo test passed", "completed");
        evidence.pull_request_url = Some("https://github.com/Alive24/jade-symphony/pull/21".into());
        evidence.pull_request_is_draft = Some(false);
        evidence.record_main_workpad_markdown(Some(
            "## Jade Symphony Workpad\n\n### Plan\n\n### Work Log",
        ));

        let report = evaluate_agent_review_handoff(&evidence);

        assert!(!report.is_ready());
        assert!(report
            .missing
            .contains(&"Project-visible pull request linkage status".into()));
    }

    #[test]
    fn agent_review_handoff_blocks_draft_pr() {
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue(), "main").unwrap();
        let mut evidence =
            AgentReviewHandoffEvidence::from_plan(&plan, "cargo test passed", "completed");
        evidence.pull_request_url = Some("https://github.com/Alive24/jade-symphony/pull/21".into());
        evidence.project_pr_link_verified = Some(true);
        evidence.pull_request_is_draft = Some(true);
        evidence.record_main_workpad_markdown(Some(
            "## Jade Symphony Workpad\n\n### Plan\n\n### Work Log",
        ));

        let report = evaluate_agent_review_handoff(&evidence);

        assert!(!report.is_ready());
        assert!(report.missing.contains(&"non-draft pull request".into()));
    }

    #[test]
    fn agent_review_handoff_blocks_unknown_draft_status() {
        let plan = plan_issue_handoff(Path::new("/tmp/jade-workspaces"), &issue(), "main").unwrap();
        let mut evidence =
            AgentReviewHandoffEvidence::from_plan(&plan, "cargo test passed", "completed");
        evidence.pull_request_url = Some("https://github.com/Alive24/jade-symphony/pull/21".into());
        evidence.project_pr_link_verified = Some(true);
        evidence.record_main_workpad_markdown(Some(
            "## Jade Symphony Workpad\n\n### Plan\n\n### Work Log",
        ));

        let report = evaluate_agent_review_handoff(&evidence);

        assert!(!report.is_ready());
        assert!(report.missing.contains(&"pull request draft status".into()));
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
