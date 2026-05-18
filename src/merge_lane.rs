use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::git_handoff::{CommandOutput, GitHandoffError, HandoffCommandRunner};
use crate::handoff::branch_target_evidence;
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
            target_state: None,
            reason: format!("check `{pending}` is still pending; retry merge preflight later"),
        };
    }

    match status.merge_state_status.as_deref() {
        Some("DIRTY") | Some("BEHIND") => {
            return MergeLaneDecision {
                kind: MergeLaneDecisionKind::MergeDirty,
                issue_ref: issue.identifier.clone(),
                pr_url: Some(status.url.clone()),
                target_state: Some("rework"),
                reason: format!(
                    "pull request merge state is `{}`",
                    status.merge_state_status.as_deref().unwrap_or_default()
                ),
            };
        }
        Some("CLEAN") | Some("HAS_HOOKS") => {}
        Some("UNKNOWN") => {
            return MergeLaneDecision {
                kind: MergeLaneDecisionKind::MergeabilityUnknown,
                issue_ref: issue.identifier.clone(),
                pr_url: Some(status.url.clone()),
                target_state: None,
                reason:
                    "pull request merge state is `UNKNOWN` after recheck; retry merge preflight later"
                        .into(),
            };
        }
        Some(other) => {
            return MergeLaneDecision {
                kind: MergeLaneDecisionKind::MergeabilityUnknown,
                issue_ref: issue.identifier.clone(),
                pr_url: Some(status.url.clone()),
                target_state: Some("need_human_input"),
                reason: format!(
                    "pull request merge state is `{other}` and needs operator classification"
                ),
            };
        }
        None => {
            return MergeLaneDecision {
                kind: MergeLaneDecisionKind::MergeabilityUnknown,
                issue_ref: issue.identifier.clone(),
                pr_url: Some(status.url.clone()),
                target_state: None,
                reason:
                    "pull request merge state is missing after recheck; retry merge preflight later"
                        .into(),
            };
        }
    }

    if let Some((target_state, reason)) = blocking_review_decision(&status.review_decision) {
        return MergeLaneDecision {
            kind: MergeLaneDecisionKind::ReviewNotApproved,
            issue_ref: issue.identifier.clone(),
            pr_url: Some(status.url.clone()),
            target_state: Some(target_state),
            reason,
        };
    }

    MergeLaneDecision {
        kind: MergeLaneDecisionKind::ReadyToMerge,
        issue_ref: issue.identifier.clone(),
        pr_url: Some(status.url.clone()),
        target_state: Some("done"),
        reason: "pull request passed merge preflight with Project Merging approval".into(),
    }
}

fn blocking_review_decision(review_decision: &Option<String>) -> Option<(&'static str, String)> {
    let decision = review_decision.as_deref().unwrap_or_default().trim();
    if decision.is_empty() || decision.eq_ignore_ascii_case("APPROVED") {
        return None;
    }

    let target_state = if decision.eq_ignore_ascii_case("CHANGES_REQUESTED") {
        "rework"
    } else {
        "need_human_input"
    };

    Some((
        target_state,
        format!("pull request review decision is `{decision}`"),
    ))
}

pub fn pull_request_status_from_linked(
    pull_request: &LinkedPullRequest,
) -> Option<PullRequestMergeStatus> {
    Some(PullRequestMergeStatus {
        number: pull_request.number,
        url: pull_request.url.clone()?,
        state: pull_request.state.clone().unwrap_or_else(|| "OPEN".into()),
        is_draft: pull_request.is_draft.unwrap_or(false),
        merge_state_status: pull_request.merge_state_status.clone(),
        review_decision: pull_request.review_decision.clone(),
        base_ref_name: pull_request.base_ref_name.clone(),
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

pub fn fetch_pull_request_status_with_recheck(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
    attempts: usize,
) -> Result<PullRequestMergeStatus, MergeLaneError> {
    let attempts = attempts.max(1);
    let mut status = fetch_pull_request_status(pr_ref, runner, cwd)?;
    for _ in 1..attempts {
        if !mergeability_needs_recheck(&status) {
            break;
        }
        status = fetch_pull_request_status(pr_ref, runner, cwd)?;
    }
    Ok(status)
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
    let branch_target = branch_target_evidence(issue, expected_merge_base_branch_for_workpad());
    lines.push(String::new());
    lines.push("### Branch Target Evidence".to_string());
    lines.push(format!("- Role: `{:?}`", branch_target.role));
    lines.push(format!(
        "- Expected PR base branch: `{}`",
        branch_target.pull_request_base_branch
    ));
    if let Some(parent_issue) = branch_target.parent_issue {
        lines.push(format!("- Native parent issue: `{parent_issue}`"));
    }
    if let Some(parent_integration_branch) = branch_target.parent_integration_branch {
        lines.push(format!(
            "- Parent integration branch: `{parent_integration_branch}`"
        ));
    }

    if let Some(output) = merge_output {
        lines.extend([
            String::new(),
            "### Merge Command Evidence".to_string(),
            format!("- Exit status: `{}`", output.status),
            format!("- Stdout: `{}`", single_line(&output.stdout)),
            format!("- Stderr: `{}`", single_line(&output.stderr)),
        ]);
    }

    if decision.target_state == Some("need_human_input") {
        lines.extend(required_human_input_section(decision));
    }

    lines.join("\n")
}

pub fn expected_merge_base_branch(_config: &RuntimeConfig) -> &'static str {
    "main"
}

fn expected_merge_base_branch_for_workpad() -> &'static str {
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

fn mergeability_needs_recheck(status: &PullRequestMergeStatus) -> bool {
    status
        .merge_state_status
        .as_deref()
        .map(|state| state.eq_ignore_ascii_case("UNKNOWN"))
        .unwrap_or(true)
}

fn required_human_input_section(decision: &MergeLaneDecision) -> Vec<String> {
    let question = match decision.kind {
        MergeLaneDecisionKind::MissingPullRequest => {
            "Which pull request should this Merging issue land?"
        }
        MergeLaneDecisionKind::AmbiguousPullRequest => {
            "Which of the linked pull requests is the canonical merge target?"
        }
        MergeLaneDecisionKind::PullRequestClosed => {
            "Should the closed pull request be reopened, replaced, or should the issue leave Merging?"
        }
        MergeLaneDecisionKind::DraftPullRequest => {
            "Should the draft pull request stay in Merging, move back to Rework, or wait for the author to mark it ready?"
        }
        MergeLaneDecisionKind::BaseMismatch => {
            "Should the pull request base branch be changed, or is this issue targeting a different release branch?"
        }
        MergeLaneDecisionKind::ReviewNotApproved => {
            "Should the review decision block merge, or should the issue move back to Rework for follow-up?"
        }
        MergeLaneDecisionKind::MergeabilityUnknown => {
            "How should this non-standard mergeability state be classified for the merge lane?"
        }
        _ => "What human decision is required before the merge lane can continue?",
    };

    vec![
        String::new(),
        "### Required Human Input".to_string(),
        format!("- Question: {question}"),
        "- Options: update PR metadata, choose the canonical PR, move the issue out of Merging, or document the required repair.".to_string(),
        "- After answer: rerun `jade-symphony merge-once` so the merge lane can re-evaluate with concrete evidence.".to_string(),
    ]
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
    use crate::git_handoff::GitHandoffError;
    use std::cell::RefCell;

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
            ..Default::default()
        }
    }

    fn clean_fixture_pr() -> LinkedPullRequest {
        LinkedPullRequest {
            id: Some("PR_60".into()),
            number: Some(60),
            url: Some("https://github.com/Alive24/jade-symphony/pull/60".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            base_ref_name: Some("main".into()),
            head_ref_name: None,
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

    fn subissue(state: &str, prs: Vec<LinkedPullRequest>) -> TrackerIssue {
        let mut issue = issue(state, prs);
        issue.identifier = "#274".into();
        issue.title = "Teach lane flows about parent integration branches".into();
        issue
            .project_fields
            .insert("Native Parent Issue".into(), serde_json::json!("#243"));
        issue.project_fields.insert(
            "Parent Integration Branch".into(),
            serde_json::json!("integration/issue-243-parent-subissue-orchestration"),
        );
        issue
    }

    fn pr_json(merge_state_status: &str) -> String {
        format!(
            r#"{{
                "number": 60,
                "url": "https://github.com/Alive24/jade-symphony/pull/60",
                "state": "OPEN",
                "isDraft": false,
                "mergeStateStatus": "{merge_state_status}",
                "reviewDecision": "APPROVED",
                "baseRefName": "main",
                "statusCheckRollup": [
                    {{"name": "cargo test", "status": "COMPLETED", "conclusion": "SUCCESS"}}
                ]
            }}"#
        )
    }

    struct SequenceRunner {
        outputs: RefCell<Vec<String>>,
    }

    impl HandoffCommandRunner for SequenceRunner {
        fn run(
            &self,
            _program: &str,
            _args: &[String],
            _cwd: &Path,
        ) -> Result<CommandOutput, GitHandoffError> {
            Ok(CommandOutput {
                status: 0,
                stdout: self.outputs.borrow_mut().remove(0),
                stderr: String::new(),
            })
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
    fn fixture_linked_pr_metadata_can_represent_clean_merge_preflight() {
        let status = pull_request_status_from_linked(&clean_fixture_pr()).unwrap();

        assert_eq!(status.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(status.merge_state_status.as_deref(), Some("CLEAN"));
        assert_eq!(status.base_ref_name.as_deref(), Some("main"));
        assert!(!status.is_draft);
    }

    #[test]
    fn clean_project_merging_pr_without_github_review_decision_is_ready_to_merge() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.review_decision = Some(String::new());
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
    fn dirty_pr_without_github_review_decision_routes_to_rework() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.merge_state_status = Some("DIRTY".into());
        status.review_decision = Some(String::new());
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
    fn unknown_then_clean_after_recheck_becomes_ready() {
        let runner = SequenceRunner {
            outputs: RefCell::new(vec![pr_json("UNKNOWN"), pr_json("CLEAN")]),
        };

        let status =
            fetch_pull_request_status_with_recheck("60", &runner, Path::new("."), 2).unwrap();

        assert_eq!(status.merge_state_status.as_deref(), Some("CLEAN"));
    }

    #[test]
    fn unknown_then_dirty_after_recheck_routes_to_rework() {
        let runner = SequenceRunner {
            outputs: RefCell::new(vec![pr_json("UNKNOWN"), pr_json("DIRTY")]),
        };
        let status =
            fetch_pull_request_status_with_recheck("60", &runner, Path::new("."), 2).unwrap();
        let issue = issue("Merging", vec![pr()]);
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
    fn unknown_remaining_after_recheck_stays_in_merging() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.merge_state_status = Some("UNKNOWN".into());
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::MergeabilityUnknown);
        assert_eq!(decision.target_state, None);
        assert!(decision.reason.contains("retry merge preflight later"));
    }

    #[test]
    fn changes_requested_review_routes_to_rework() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.review_decision = Some("CHANGES_REQUESTED".into());
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::ReviewNotApproved);
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
    fn pending_check_stays_in_merging_for_retry() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.checks[0].status = Some("IN_PROGRESS".into());
        status.checks[0].conclusion = None;
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::ChecksPending);
        assert_eq!(decision.target_state, None);
        assert!(decision.reason.contains("retry merge preflight later"));
    }

    #[test]
    fn need_human_input_workpad_includes_actionable_question() {
        let issue = issue("Merging", Vec::new());
        let decision = merge_lane_decision(&issue, "Merging", "main", &[], None);
        let workpad = merge_lane_workpad(&issue, &decision, None);

        assert!(workpad.contains("### Required Human Input"));
        assert!(workpad.contains("Which pull request should this Merging issue land?"));
        assert!(workpad.contains("After answer"));
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

    #[test]
    fn subissue_merge_uses_parent_integration_branch_as_expected_base() {
        let issue = subissue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.base_ref_name = Some("integration/issue-243-parent-subissue-orchestration".into());
        let expected_base = crate::handoff::expected_merge_base_branch_for_issue(&issue, "main");
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            &expected_base,
            &issue.linked_pull_requests,
            Some(&status),
        );
        let workpad = merge_lane_workpad(&issue, &decision, None);

        assert_eq!(decision.kind, MergeLaneDecisionKind::ReadyToMerge);
        assert_eq!(decision.target_state, Some("done"));
        assert!(workpad.contains("Role: `Subissue`"));
        assert!(workpad.contains(
            "Expected PR base branch: `integration/issue-243-parent-subissue-orchestration`"
        ));
    }

    #[test]
    fn subissue_pr_targeting_main_is_a_base_mismatch() {
        let issue = subissue("Merging", vec![pr()]);
        let status = clean_status();
        let expected_base = crate::handoff::expected_merge_base_branch_for_issue(&issue, "main");
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            &expected_base,
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::BaseMismatch);
        assert_eq!(decision.target_state, Some("need_human_input"));
        assert!(decision
            .reason
            .contains("expected `integration/issue-243-parent-subissue-orchestration`"));
    }
}
