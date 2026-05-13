use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::git_handoff::{CommandOutput, GitHandoffError, HandoffCommandRunner};
use crate::model::{normalize_state, LinkedPullRequest, TrackerIssue};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestMergeStatus {
    pub number: Option<u64>,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub merge_state_status: Option<String>,
    pub review_decision: Option<String>,
    pub base_ref_name: Option<String>,
    #[serde(default)]
    pub checks: Vec<PullRequestCheckStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestCheckStatus {
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeLaneDecision {
    pub kind: MergeLaneDecisionKind,
    pub issue_ref: String,
    pub pr_url: Option<String>,
    pub target_state: Option<&'static str>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeLaneDecisionKind {
    ReadyToMerge,
    AlreadyMerged,
    WrongIssueState,
    MissingPullRequest,
    AmbiguousPullRequest,
    PullRequestClosed,
    DraftPullRequest,
    BaseMismatch,
    ReviewNotApproved,
    ChecksPending,
    ChecksFailing,
    MergeDirty,
    MergeabilityUnknown,
}

impl MergeLaneDecisionKind {
    pub fn is_merge_ready(self) -> bool {
        matches!(self, Self::ReadyToMerge)
    }
}

#[derive(Debug, Error)]
pub enum MergeLaneError {
    #[error("merge lane command failed: {0}")]
    Git(#[from] GitHandoffError),
    #[error("merge lane payload failed: {0}")]
    Payload(String),
}

pub fn merge_lane_decision(
    issue: &TrackerIssue,
    expected_merging_state: &str,
    expected_base_branch: &str,
    linked_pull_requests: &[LinkedPullRequest],
    status: Option<&PullRequestMergeStatus>,
) -> MergeLaneDecision {
    if normalize_state(&issue.state) != normalize_state(expected_merging_state) {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::WrongIssueState,
            issue_ref: issue.identifier.clone(),
            pr_url: None,
            target_state: None,
            reason: format!(
                "issue is in `{}`, but merge lane only handles `{expected_merging_state}`",
                issue.state
            ),
        };
    }

    if linked_pull_requests.is_empty() {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::MissingPullRequest,
            issue_ref: issue.identifier.clone(),
            pr_url: None,
            target_state: Some("need_human_input"),
            reason: "no linked pull request evidence was found".into(),
        };
    }

    if linked_pull_requests.len() > 1 {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::AmbiguousPullRequest,
            issue_ref: issue.identifier.clone(),
            pr_url: None,
            target_state: Some("need_human_input"),
            reason: "multiple linked pull requests were found; operator must choose one".into(),
        };
    }

    let linked = &linked_pull_requests[0];
    let pr_url = linked.url.clone();
    let Some(status) = status else {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::MergeabilityUnknown,
            issue_ref: issue.identifier.clone(),
            pr_url,
            target_state: Some("need_human_input"),
            reason: "live pull request preflight did not produce mergeability evidence".into(),
        };
    };

    if normalize_state(&status.state) == "merged" {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::AlreadyMerged,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("done"),
            reason: "pull request is already merged".into(),
        };
    }

    if normalize_state(&status.state) != "open" {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::PullRequestClosed,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: format!("pull request state is `{}`", status.state),
        };
    }

    if status.is_draft {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::DraftPullRequest,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: "pull request is still draft".into(),
        };
    }

    if status
        .base_ref_name
        .as_deref()
        .is_some_and(|base| base != expected_base_branch)
    {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::BaseMismatch,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: format!(
                "pull request base is `{}`, expected `{expected_base_branch}`",
                status.base_ref_name.as_deref().unwrap_or_default()
            ),
        };
    }

    if !matches!(
        status.review_decision.as_deref(),
        Some("APPROVED") | Some("approved")
    ) {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::ReviewNotApproved,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: format!(
                "pull request review decision is `{}`",
                status.review_decision.as_deref().unwrap_or("missing")
            ),
        };
    }

    if let Some(failing) = first_failing_check(&status.checks) {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::ChecksFailing,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("rework"),
            reason: format!("check `{failing}` is failing"),
        };
    }

    if let Some(pending) = first_pending_check(&status.checks) {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::ChecksPending,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: format!("check `{pending}` is still pending"),
        };
    }

    match status.merge_state_status.as_deref() {
        Some("CLEAN") | Some("HAS_HOOKS") => MergeLaneDecision {
            kind: MergeLaneDecisionKind::ReadyToMerge,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("done"),
            reason: "pull request passed merge preflight".into(),
        },
        Some("DIRTY") | Some("BEHIND") => MergeLaneDecision {
            kind: MergeLaneDecisionKind::MergeDirty,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("rework"),
            reason: format!(
                "pull request merge state is `{}`",
                status.merge_state_status.as_deref().unwrap_or_default()
            ),
        },
        Some(other) => MergeLaneDecision {
            kind: MergeLaneDecisionKind::MergeabilityUnknown,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: format!("pull request merge state is `{other}`"),
        },
        None => MergeLaneDecision {
            kind: MergeLaneDecisionKind::MergeabilityUnknown,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some("need_human_input"),
            reason: "pull request merge state is missing".into(),
        },
    }
}

pub fn pull_request_status_from_linked(
    pull_request: &LinkedPullRequest,
) -> Option<PullRequestMergeStatus> {
    Some(PullRequestMergeStatus {
        number: pull_request.number,
        url: pull_request.url.clone()?,
        state: pull_request.state.clone().unwrap_or_else(|| "OPEN".into()),
        is_draft: false,
        merge_state_status: None,
        review_decision: None,
        base_ref_name: None,
        checks: Vec::new(),
    })
}

pub fn fetch_pull_request_status(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<PullRequestMergeStatus, MergeLaneError> {
    let output = runner.run(
        "gh",
        &[
            "pr".into(),
            "view".into(),
            pr_ref.into(),
            "--json".into(),
            "number,url,state,isDraft,mergeStateStatus,reviewDecision,baseRefName,statusCheckRollup"
                .into(),
        ],
        cwd,
    )?;
    require_success("gh", &output)?;
    let value: serde_json::Value = serde_json::from_str(&output.stdout)
        .map_err(|error| MergeLaneError::Payload(format!("invalid gh PR JSON: {error}")))?;
    pull_request_status_from_json(&value)
}

pub fn merge_pull_request(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<CommandOutput, MergeLaneError> {
    let output = runner.run(
        "gh",
        &[
            "pr".into(),
            "merge".into(),
            pr_ref.into(),
            "--merge".into(),
            "--delete-branch".into(),
        ],
        cwd,
    )?;
    require_success("gh", &output)?;
    Ok(output)
}

pub fn merge_lane_workpad(
    issue: &TrackerIssue,
    decision: &MergeLaneDecision,
    merge_output: Option<&CommandOutput>,
) -> String {
    let mut lines = vec![
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Merge Lane Handoff".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Actor role: `merge_agent`".to_string(),
        "- Source: `jade-symphony merge-once`".to_string(),
        format!("- Decision: `{:?}`", decision.kind),
        format!("- Reason: {}", decision.reason),
        format!(
            "- Pull request: `{}`",
            decision.pr_url.as_deref().unwrap_or("missing")
        ),
        format!(
            "- Target state: `{}`",
            decision.target_state.unwrap_or("none")
        ),
        String::new(),
        "### Authority Boundary".to_string(),
        "- This lane only consumes issues already in `Merging`.".to_string(),
        "- It must not move work into `Human Review`.".to_string(),
        "- It records diagnostics before any tracker state transition.".to_string(),
    ];

    if let Some(output) = merge_output {
        lines.extend([
            String::new(),
            "### Merge Command Evidence".to_string(),
            format!("- Exit status: `{}`", output.status),
            format!("- Stdout: `{}`", single_line(&output.stdout)),
            format!("- Stderr: `{}`", single_line(&output.stderr)),
        ]);
    }

    lines.join("\n")
}

pub fn expected_merge_base_branch(_config: &RuntimeConfig) -> &'static str {
    "main"
}

fn pull_request_status_from_json(
    value: &serde_json::Value,
) -> Result<PullRequestMergeStatus, MergeLaneError> {
    let url = value
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| MergeLaneError::Payload("PR payload missing url".into()))?;
    Ok(PullRequestMergeStatus {
        number: value.get("number").and_then(|value| value.as_u64()),
        url: url.into(),
        state: value
            .get("state")
            .and_then(|value| value.as_str())
            .unwrap_or("UNKNOWN")
            .into(),
        is_draft: value
            .get("isDraft")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        merge_state_status: optional_string(value.get("mergeStateStatus")),
        review_decision: optional_string(value.get("reviewDecision")),
        base_ref_name: optional_string(value.get("baseRefName")),
        checks: checks_from_json(value.get("statusCheckRollup")),
    })
}

fn checks_from_json(value: Option<&serde_json::Value>) -> Vec<PullRequestCheckStatus> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| PullRequestCheckStatus {
                    name: item
                        .get("name")
                        .or_else(|| item.get("context"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("unnamed")
                        .into(),
                    status: optional_string(item.get("status")),
                    conclusion: optional_string(item.get("conclusion")),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn optional_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
}

fn first_failing_check(checks: &[PullRequestCheckStatus]) -> Option<String> {
    checks.iter().find_map(|check| {
        let conclusion = check.conclusion.as_deref()?.to_ascii_uppercase();
        matches!(
            conclusion.as_str(),
            "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "CANCELLED"
        )
        .then(|| check.name.clone())
    })
}

fn first_pending_check(checks: &[PullRequestCheckStatus]) -> Option<String> {
    checks.iter().find_map(|check| {
        let status = check
            .status
            .as_deref()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let conclusion = check
            .conclusion
            .as_deref()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let completed = status.is_empty() || status == "COMPLETED";
        let passed = matches!(conclusion.as_str(), "" | "SUCCESS" | "NEUTRAL" | "SKIPPED");
        (!completed || !passed).then(|| check.name.clone())
    })
}

fn require_success(program: &str, output: &CommandOutput) -> Result<(), MergeLaneError> {
    if output.status == 0 {
        Ok(())
    } else {
        Err(MergeLaneError::Git(GitHandoffError::CommandFailed {
            program: program.into(),
            status: output.status,
            stdout: output.stdout.clone(),
            stderr: output.stderr.clone(),
        }))
    }
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(state: &str, prs: Vec<LinkedPullRequest>) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "ISSUE_60".into(),
            item_id: None,
            identifier: "#60".into(),
            title: "Implement guarded Merging land flow".into(),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: None,
            linked_pull_requests: prs,
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    fn pr() -> LinkedPullRequest {
        LinkedPullRequest {
            id: Some("PR_60".into()),
            number: Some(60),
            url: Some("https://github.com/Alive24/jade-symphony/pull/60".into()),
            state: Some("OPEN".into()),
        }
    }

    fn clean_status() -> PullRequestMergeStatus {
        PullRequestMergeStatus {
            number: Some(60),
            url: "https://github.com/Alive24/jade-symphony/pull/60".into(),
            state: "OPEN".into(),
            is_draft: false,
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            base_ref_name: Some("main".into()),
            checks: vec![PullRequestCheckStatus {
                name: "cargo test".into(),
                status: Some("COMPLETED".into()),
                conclusion: Some("SUCCESS".into()),
            }],
        }
    }

    #[test]
    fn clean_approved_pr_is_ready_to_merge() {
        let issue = issue("Merging", vec![pr()]);
        let status = clean_status();
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::ReadyToMerge);
        assert_eq!(decision.target_state, Some("done"));
    }

    #[test]
    fn dirty_pr_routes_to_rework() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.merge_state_status = Some("DIRTY".into());
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::MergeDirty);
        assert_eq!(decision.target_state, Some("rework"));
    }

    #[test]
    fn missing_pr_routes_to_human_input() {
        let issue = issue("Merging", Vec::new());
        let decision = merge_lane_decision(&issue, "Merging", "main", &[], None);

        assert_eq!(decision.kind, MergeLaneDecisionKind::MissingPullRequest);
        assert_eq!(decision.target_state, Some("need_human_input"));
    }

    #[test]
    fn failing_check_routes_to_rework() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.checks[0].conclusion = Some("FAILURE".into());
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::ChecksFailing);
        assert_eq!(decision.target_state, Some("rework"));
    }

    #[test]
    fn already_merged_pr_routes_to_done() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.state = "MERGED".into();
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::AlreadyMerged);
        assert_eq!(decision.target_state, Some("done"));
    }
}
