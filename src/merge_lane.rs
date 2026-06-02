use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::git_handoff::{CommandOutput, GitHandoffError, HandoffCommandRunner};
use crate::handoff::branch_target_evidence;
use crate::lane_claim::LaneClaim;
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
    pub head_ref_name: Option<String>,
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
    StaleBranch,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeConflictRepairOutcome {
    pub repaired: bool,
    pub worktree_path: Option<PathBuf>,
    pub output: CommandOutput,
    pub reason: String,
    pub failure_kind: Option<MergeConflictRepairFailureKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeConflictRepairFailureKind {
    MissingHeadBranch,
    MissingWorktree,
    DirtyWorktree,
    ContentConflict,
    PostMergeDirty,
    PushFailed,
}

impl MergeConflictRepairOutcome {
    pub fn is_agent_repair_eligible(&self) -> bool {
        !self.repaired
            && matches!(
                self.failure_kind,
                Some(MergeConflictRepairFailureKind::ContentConflict)
            )
            && self.worktree_path.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRepairEvidence {
    pub method: String,
    pub conflict_summary: String,
    pub resolution_summary: String,
    pub semantic_safety: String,
    pub verification: String,
    pub push_evidence: String,
    pub next_state_rationale: String,
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
            target_state: Some("need_human_input"),
            reason: format!(
                "check `{failing}` is failing; merge lane needs operator classification before repair"
            ),
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
        Some("BEHIND") => {
            return MergeLaneDecision {
                kind: MergeLaneDecisionKind::StaleBranch,
                issue_ref: issue.identifier.clone(),
                pr_url: Some(status.url.clone()),
                target_state: None,
                reason:
                    "pull request is behind the base branch; merge lane should safely update the PR branch and retry later"
                        .into(),
            };
        }
        Some("DIRTY") => {
            return MergeLaneDecision {
                kind: MergeLaneDecisionKind::MergeDirty,
                issue_ref: issue.identifier.clone(),
                pr_url: Some(status.url.clone()),
                target_state: None,
                reason: format!(
                    "pull request merge state is `{}`; merge lane should attempt safe local conflict repair before operator escalation",
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

pub fn native_linked_pull_requests_for_merge(
    config: &RuntimeConfig,
    linked_pull_requests: &[LinkedPullRequest],
) -> Vec<LinkedPullRequest> {
    native_linked_pull_requests_for_merge_parts(
        &config.tracker.kind,
        config.tracker.owner.as_deref(),
        config.tracker.repo.as_deref(),
        linked_pull_requests,
    )
}

fn native_linked_pull_requests_for_merge_parts(
    tracker_kind: &str,
    owner: Option<&str>,
    repo: Option<&str>,
    linked_pull_requests: &[LinkedPullRequest],
) -> Vec<LinkedPullRequest> {
    let require_native_id = tracker_kind == "github_project_v2";
    linked_pull_requests
        .iter()
        .filter(|pull_request| {
            linked_pull_request_matches_native_repo(pull_request, require_native_id, owner, repo)
        })
        .cloned()
        .collect()
}

fn linked_pull_request_matches_native_repo(
    pull_request: &LinkedPullRequest,
    require_native_id: bool,
    owner: Option<&str>,
    repo: Option<&str>,
) -> bool {
    if require_native_id && !pull_request.is_github_native_linkage() {
        return false;
    }

    let Some((url_owner, url_repo)) = pull_request
        .url
        .as_deref()
        .and_then(github_pull_request_repo)
    else {
        return true;
    };

    match (owner, repo) {
        (Some(owner), Some(repo)) => {
            url_owner.eq_ignore_ascii_case(owner) && url_repo.eq_ignore_ascii_case(repo)
        }
        _ => true,
    }
}

fn github_pull_request_repo(url: &str) -> Option<(&str, &str)> {
    let suffix = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))?;
    let mut parts = suffix.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    (parts.next() == Some("pull")).then_some((owner, repo))
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
        head_ref_name: pull_request.head_ref_name.clone(),
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
            "number,url,state,isDraft,mergeStateStatus,reviewDecision,baseRefName,headRefName,statusCheckRollup"
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
    // Issue worktrees intentionally keep the PR branch checked out for audit and
    // recovery. Branch/worktree cleanup belongs to the explicit clean surface,
    // not to the merge success path.
    let output = runner.run(
        "gh",
        &["pr".into(), "merge".into(), pr_ref.into(), "--merge".into()],
        cwd,
    )?;
    require_success("gh", &output)?;
    Ok(output)
}

pub fn update_pull_request_branch(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<CommandOutput, MergeLaneError> {
    Ok(runner.run(
        "gh",
        &["pr".into(), "update-branch".into(), pr_ref.into()],
        cwd,
    )?)
}

pub fn repair_dirty_pull_request(
    pr_ref: &str,
    head_ref_name: Option<&str>,
    expected_base_branch: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
    fixture_mode: bool,
) -> Result<MergeConflictRepairOutcome, MergeLaneError> {
    if fixture_mode {
        return Ok(MergeConflictRepairOutcome {
            repaired: true,
            worktree_path: None,
            output: CommandOutput {
                status: 0,
                stdout: format!(
                    "fixture conflict repair rehearsed for {pr_ref}; no live branch was changed"
                ),
                stderr: String::new(),
            },
            reason: "fixture-mode safe conflict repair rehearsal completed".into(),
            failure_kind: None,
        });
    }

    let Some(head_ref_name) = head_ref_name.filter(|value| !value.trim().is_empty()) else {
        return Ok(MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: None,
            output: CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "pull request head branch is missing from preflight metadata".into(),
            },
            reason: "cannot attempt safe conflict repair without a PR head branch".into(),
            failure_kind: Some(MergeConflictRepairFailureKind::MissingHeadBranch),
        });
    };

    let Some(worktree_path) = find_worktree_for_branch(head_ref_name, runner, cwd)? else {
        return Ok(MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: None,
            output: CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: format!("no local worktree found for branch `{head_ref_name}`"),
            },
            reason: format!(
                "no local worktree is available for PR branch `{head_ref_name}`; operator must adopt or create the existing PR worktree before merge-lane repair"
            ),
            failure_kind: Some(MergeConflictRepairFailureKind::MissingWorktree),
        });
    };

    let status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        &worktree_path,
    )?;
    require_success("git", &status)?;
    if !status.stdout.trim().is_empty() {
        return Ok(MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: Some(worktree_path),
            output: status,
            reason: "PR worktree is dirty before merge-lane repair".into(),
            failure_kind: Some(MergeConflictRepairFailureKind::DirtyWorktree),
        });
    }

    let fetch_ref = format!("origin/{expected_base_branch}");
    let fetch = runner.run(
        "git",
        &["fetch".into(), "origin".into(), expected_base_branch.into()],
        &worktree_path,
    )?;
    require_success("git", &fetch)?;

    let merge = runner.run(
        "git",
        &["merge".into(), "--no-edit".into(), fetch_ref.clone()],
        &worktree_path,
    )?;
    if merge.status != 0 {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], &worktree_path);
        return Ok(MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: Some(worktree_path),
            output: merge,
            reason: format!(
                "safe merge-lane repair could not merge `{fetch_ref}` into `{head_ref_name}` without manual conflict resolution"
            ),
            failure_kind: Some(MergeConflictRepairFailureKind::ContentConflict),
        });
    }

    let post_status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        &worktree_path,
    )?;
    require_success("git", &post_status)?;
    if !post_status.stdout.trim().is_empty() {
        return Ok(MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: Some(worktree_path),
            output: post_status,
            reason: "merge-lane repair left uncommitted changes in the PR worktree".into(),
            failure_kind: Some(MergeConflictRepairFailureKind::PostMergeDirty),
        });
    }

    let push = runner.run(
        "git",
        &["push".into(), "origin".into(), head_ref_name.into()],
        &worktree_path,
    )?;
    if push.status != 0 {
        return Ok(MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: Some(worktree_path),
            output: push,
            reason:
                "merge-lane repair succeeded locally, but pushing the repaired PR branch failed"
                    .into(),
            failure_kind: Some(MergeConflictRepairFailureKind::PushFailed),
        });
    }

    Ok(MergeConflictRepairOutcome {
        repaired: true,
        worktree_path: Some(worktree_path),
        output: push,
        reason: format!(
            "safe merge-lane repair merged `{fetch_ref}` into `{head_ref_name}` and pushed the existing PR branch"
        ),
        failure_kind: None,
    })
}

pub fn fixture_merge_output(pr_ref: &str) -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: format!("fixture merge rehearsed for {pr_ref}; no live GitHub merge was performed"),
        stderr: String::new(),
    }
}

pub fn merge_lane_workpad(
    issue: &TrackerIssue,
    decision: &MergeLaneDecision,
    merge_output: Option<&CommandOutput>,
) -> String {
    merge_lane_workpad_with_repair_evidence(issue, decision, merge_output, None)
}

pub fn merge_lane_workpad_with_repair_evidence(
    issue: &TrackerIssue,
    decision: &MergeLaneDecision,
    merge_output: Option<&CommandOutput>,
    repair_evidence: Option<&MergeRepairEvidence>,
) -> String {
    let mut lines = vec![
        "## Shea Symphony Merge Run".to_string(),
        String::new(),
        format!("- Generated at: `{}`", current_gmt_timestamp()),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Lane: `merge`".to_string(),
        "- Actor role: `merge_agent`".to_string(),
        format!("- Actor: `{}`", merge_actor(issue)),
        format!("- Run ID: `{}`", merge_run_id(issue)),
        "- Source: `shea-symphony merge once`".to_string(),
        "- Input state: `Merging`".to_string(),
        format!("- Decision: `{:?}`", decision.kind),
        format!("- Result: `{}`", merge_result(decision)),
        format!("- Reason: {}", decision.reason),
        format!(
            "- Pull request: `{}`",
            decision.pr_url.as_deref().unwrap_or("missing")
        ),
        format!(
            "- Target state after merge routing: `{}`",
            decision.target_state.unwrap_or("none")
        ),
        format!(
            "- Evidence summary: merge decision `{}`; command evidence `{}`.",
            merge_result(decision),
            if merge_output.is_some() {
                "recorded"
            } else {
                "not recorded"
            }
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

    if let Some(evidence) = repair_evidence {
        lines.extend([
            String::new(),
            "### Merge Repair Evidence".to_string(),
            format!("- Method: `{}`", evidence.method),
            format!("- Conflict summary: {}", evidence.conflict_summary),
            format!("- Resolution summary: {}", evidence.resolution_summary),
            format!("- Semantic safety: {}", evidence.semantic_safety),
            format!("- Verification: {}", evidence.verification),
            format!("- Push evidence: {}", evidence.push_evidence),
            format!("- Next-state rationale: {}", evidence.next_state_rationale),
        ]);
    }

    if decision.target_state == Some("need_human_input") {
        lines.extend(required_human_input_section(decision));
    }

    lines.join("\n")
}

fn merge_actor(issue: &TrackerIssue) -> String {
    merge_claim(issue)
        .map(|claim| claim.actor.as_str().to_string())
        .unwrap_or_else(|| "not recorded".into())
}

fn merge_run_id(issue: &TrackerIssue) -> String {
    merge_claim(issue)
        .map(|claim| claim.run)
        .unwrap_or_else(|| "not recorded".into())
}

fn merge_claim(issue: &TrackerIssue) -> Option<LaneClaim> {
    issue
        .project_fields
        .get("Merging Agent")
        .or_else(|| issue.project_fields.get("Merge Agent"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| LaneClaim::parse(value).ok())
}

fn merge_result(decision: &MergeLaneDecision) -> &'static str {
    match decision.kind {
        MergeLaneDecisionKind::ReadyToMerge | MergeLaneDecisionKind::AlreadyMerged => {
            "merged_or_done"
        }
        MergeLaneDecisionKind::StaleBranch => "stale_branch_update",
        MergeLaneDecisionKind::MergeDirty => "repair_or_blocked",
        MergeLaneDecisionKind::WrongIssueState
        | MergeLaneDecisionKind::MissingPullRequest
        | MergeLaneDecisionKind::AmbiguousPullRequest
        | MergeLaneDecisionKind::PullRequestClosed
        | MergeLaneDecisionKind::DraftPullRequest
        | MergeLaneDecisionKind::BaseMismatch
        | MergeLaneDecisionKind::ReviewNotApproved
        | MergeLaneDecisionKind::ChecksFailing
        | MergeLaneDecisionKind::MergeabilityUnknown => "blocked",
        MergeLaneDecisionKind::ChecksPending => "skipped",
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
        head_ref_name: optional_string(value.get("headRefName")),
        checks: checks_from_json(value.get("statusCheckRollup")),
    })
}

fn find_worktree_for_branch(
    branch_name: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<Option<PathBuf>, MergeLaneError> {
    let output = runner.run(
        "git",
        &["worktree".into(), "list".into(), "--porcelain".into()],
        cwd,
    )?;
    require_success("git", &output)?;
    Ok(parse_worktree_for_branch(&output.stdout, branch_name))
}

fn parse_worktree_for_branch(output: &str, branch_name: &str) -> Option<PathBuf> {
    let expected_branch = format!("refs/heads/{branch_name}");
    let mut current_worktree: Option<PathBuf> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_worktree = Some(PathBuf::from(path));
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if branch == expected_branch {
                return current_worktree.clone();
            }
        } else if line.trim().is_empty() {
            current_worktree = None;
        }
    }

    None
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
            "Should the draft pull request be marked ready, or should this issue leave Merging until the author finishes handoff?"
        }
        MergeLaneDecisionKind::BaseMismatch => {
            "Should the pull request base branch be changed, or is this issue targeting a different release branch?"
        }
        MergeLaneDecisionKind::ReviewNotApproved => {
            "Should the review decision block merge, or should the issue move back to Rework for follow-up?"
        }
        MergeLaneDecisionKind::ChecksFailing => {
            "Are the failing checks caused by a merge-lane-only problem that may be repaired here, or does implementation need human-directed follow-up?"
        }
        MergeLaneDecisionKind::MergeDirty => {
            "Is this conflict safe merge-lane repair, or does it require human input before changing the PR branch?"
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
        "- After answer: rerun `shea-symphony merge once` so the merge lane can re-evaluate with concrete evidence.".to_string(),
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
            url: Some("https://github.com/Alive24/shea-symphony/pull/60".into()),
            state: Some("OPEN".into()),
            ..Default::default()
        }
    }

    fn historical_pr_autolink() -> LinkedPullRequest {
        LinkedPullRequest {
            id: None,
            number: Some(60),
            url: Some("https://github.com/Alive24/jade-symphony/pull/60".into()),
            state: None,
            ..Default::default()
        }
    }

    fn clean_fixture_pr() -> LinkedPullRequest {
        LinkedPullRequest {
            id: Some("PR_60".into()),
            number: Some(60),
            url: Some("https://github.com/Alive24/shea-symphony/pull/60".into()),
            state: Some("OPEN".into()),
            is_draft: Some(false),
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            base_ref_name: Some("main".into()),
            head_ref_name: None,
            source: crate::model::LinkedPullRequestSource::GithubNative,
        }
    }

    fn clean_status() -> PullRequestMergeStatus {
        PullRequestMergeStatus {
            number: Some(60),
            url: "https://github.com/Alive24/shea-symphony/pull/60".into(),
            state: "OPEN".into(),
            is_draft: false,
            merge_state_status: Some("CLEAN".into()),
            review_decision: Some("APPROVED".into()),
            base_ref_name: Some("main".into()),
            head_ref_name: Some("feature/issue-60".into()),
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
                "url": "https://github.com/Alive24/shea-symphony/pull/60",
                "state": "OPEN",
                "isDraft": false,
                "mergeStateStatus": "{merge_state_status}",
                "reviewDecision": "APPROVED",
                "baseRefName": "main",
                "headRefName": "feature/issue-60",
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

    struct RecordingRunner {
        program: RefCell<Option<String>>,
        args: RefCell<Vec<String>>,
        output: CommandOutput,
    }

    impl HandoffCommandRunner for RecordingRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            _cwd: &Path,
        ) -> Result<CommandOutput, GitHandoffError> {
            self.program.replace(Some(program.into()));
            self.args.replace(args.to_vec());
            Ok(self.output.clone())
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
    fn github_project_merge_prefers_current_repo_native_prs_over_history_autolinks() {
        let raw_pull_requests = vec![pr(), historical_pr_autolink()];
        let filtered = native_linked_pull_requests_for_merge_parts(
            "github_project_v2",
            Some("Alive24"),
            Some("shea-symphony"),
            &raw_pull_requests,
        );

        assert_eq!(filtered, vec![pr()]);

        let issue = issue("Merging", raw_pull_requests);
        let decision =
            merge_lane_decision(&issue, "Merging", "main", &filtered, Some(&clean_status()));

        assert_eq!(decision.kind, MergeLaneDecisionKind::ReadyToMerge);
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
    fn dirty_pr_attempts_safe_repair_before_human_input() {
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
        assert_eq!(decision.target_state, None);
        assert!(decision
            .reason
            .contains("attempt safe local conflict repair"));
    }

    #[test]
    fn dirty_pr_without_github_review_decision_still_attempts_repair() {
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
        assert_eq!(decision.target_state, None);
    }

    #[test]
    fn dirty_native_subissue_pr_attempts_repair_before_human_input() {
        let parent_branch = "integration/issue-243-parent-subissue-orchestration";
        let issue = subissue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.merge_state_status = Some("DIRTY".into());
        status.base_ref_name = Some(parent_branch.into());
        status.review_decision = Some(String::new());
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            parent_branch,
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::MergeDirty);
        assert_eq!(decision.target_state, None);
        assert!(decision.reason.contains("safe local conflict repair"));
    }

    #[test]
    fn behind_pr_stays_in_merging_for_safe_update_and_retry() {
        let issue = issue("Merging", vec![pr()]);
        let mut status = clean_status();
        status.merge_state_status = Some("BEHIND".into());
        let decision = merge_lane_decision(
            &issue,
            "Merging",
            "main",
            &issue.linked_pull_requests,
            Some(&status),
        );

        assert_eq!(decision.kind, MergeLaneDecisionKind::StaleBranch);
        assert_eq!(decision.target_state, None);
        assert!(decision.reason.contains("safely update"));
    }

    #[test]
    fn stale_branch_update_uses_github_non_rewrite_command() {
        let runner = RecordingRunner {
            program: RefCell::new(None),
            args: RefCell::new(Vec::new()),
            output: CommandOutput {
                status: 0,
                stdout: "updated".into(),
                stderr: String::new(),
            },
        };

        let output = update_pull_request_branch("60", &runner, Path::new(".")).unwrap();

        assert_eq!(output.status, 0);
        assert_eq!(runner.program.borrow().as_deref(), Some("gh"));
        assert_eq!(
            *runner.args.borrow(),
            vec![
                "pr".to_string(),
                "update-branch".to_string(),
                "60".to_string()
            ]
        );
    }

    #[test]
    fn merge_pull_request_preserves_local_issue_worktree_branch() {
        let runner = RecordingRunner {
            program: RefCell::new(None),
            args: RefCell::new(Vec::new()),
            output: CommandOutput {
                status: 0,
                stdout: "merged".into(),
                stderr: String::new(),
            },
        };

        let output = merge_pull_request("60", &runner, Path::new(".")).unwrap();

        assert_eq!(output.status, 0);
        assert_eq!(runner.program.borrow().as_deref(), Some("gh"));
        assert_eq!(
            *runner.args.borrow(),
            vec![
                "pr".to_string(),
                "merge".to_string(),
                "60".to_string(),
                "--merge".to_string()
            ]
        );
        assert!(!runner
            .args
            .borrow()
            .iter()
            .any(|arg| arg == "--delete-branch"));
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
    fn unknown_then_dirty_after_recheck_routes_to_need_human_input() {
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
        assert_eq!(decision.target_state, None);
    }

    #[test]
    fn unknown_then_behind_after_recheck_stays_in_merging_for_update() {
        let runner = SequenceRunner {
            outputs: RefCell::new(vec![pr_json("UNKNOWN"), pr_json("BEHIND")]),
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

        assert_eq!(decision.kind, MergeLaneDecisionKind::StaleBranch);
        assert_eq!(decision.target_state, None);
    }

    #[test]
    fn parses_worktree_for_matching_branch() {
        let output = "\
worktree /repo
HEAD 111
branch refs/heads/main

worktree /repo/pr
HEAD 222
branch refs/heads/feature/issue-60
";

        assert_eq!(
            parse_worktree_for_branch(output, "feature/issue-60"),
            Some(PathBuf::from("/repo/pr"))
        );
        assert_eq!(parse_worktree_for_branch(output, "missing"), None);
    }

    #[test]
    fn fixture_conflict_repair_reports_success_without_live_mutation() {
        let runner = RecordingRunner {
            program: RefCell::new(None),
            args: RefCell::new(Vec::new()),
            output: CommandOutput {
                status: 99,
                stdout: String::new(),
                stderr: "should not be called".into(),
            },
        };

        let outcome =
            repair_dirty_pull_request("60", None, "main", &runner, Path::new("."), true).unwrap();

        assert!(outcome.repaired);
        assert!(outcome.output.stdout.contains("fixture conflict repair"));
        assert!(runner.program.borrow().is_none());
    }

    #[test]
    fn conflict_repair_requires_head_branch() {
        let runner = RecordingRunner {
            program: RefCell::new(None),
            args: RefCell::new(Vec::new()),
            output: CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        };

        let outcome =
            repair_dirty_pull_request("60", None, "main", &runner, Path::new("."), false).unwrap();

        assert!(!outcome.repaired);
        assert!(outcome.reason.contains("without a PR head branch"));
        assert!(runner.program.borrow().is_none());
    }

    #[test]
    fn content_conflict_repair_failure_is_merge_agent_eligible() {
        let outcome = MergeConflictRepairOutcome {
            repaired: false,
            worktree_path: Some(PathBuf::from("/repo/pr")),
            output: CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "CONFLICT (content): Merge conflict in src/main.rs".into(),
            },
            reason: "safe merge-lane repair could not merge `origin/main` without manual conflict resolution".into(),
            failure_kind: Some(MergeConflictRepairFailureKind::ContentConflict),
        };

        assert!(outcome.is_agent_repair_eligible());
    }

    #[test]
    fn merge_workpad_records_merge_agent_repair_evidence() {
        let issue = issue("Merging", vec![pr()]);
        let decision = MergeLaneDecision {
            kind: MergeLaneDecisionKind::MergeDirty,
            issue_ref: issue.identifier.clone(),
            pr_url: Some("https://github.com/Alive24/shea-symphony/pull/60".into()),
            target_state: None,
            reason: "merge-agent repaired the conflicted branch".into(),
        };
        let evidence = MergeRepairEvidence {
            method: "merge_agent".into(),
            conflict_summary: "src/main.rs conflicted".into(),
            resolution_summary: "kept approved behavior".into(),
            semantic_safety: "reviewed intent preserved".into(),
            verification: "git diff --check; git status --porcelain".into(),
            push_evidence: "git push origin feature/issue-60 exit status `0`".into(),
            next_state_rationale: "stay in Merging for reread".into(),
        };

        let workpad =
            merge_lane_workpad_with_repair_evidence(&issue, &decision, None, Some(&evidence));

        assert!(workpad.contains("### Merge Repair Evidence"));
        assert!(workpad.contains("- Method: `merge_agent`"));
        assert!(workpad.contains("reviewed intent preserved"));
        assert!(workpad.contains("stay in Merging for reread"));
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
    fn failing_check_routes_to_need_human_input() {
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
        assert_eq!(decision.target_state, Some("need_human_input"));
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
