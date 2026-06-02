use std::path::{Path, PathBuf};

use shea_symphony::agent::UsageLimitPause;
use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::{
    LiveWorktreeResult, PullRequestPublication, PullRequestReadyStatus,
};
use shea_symphony::handoff::{
    plan_issue_handoff_for_profile, AgentReviewHandoffEvidence, HandoffError, IssueHandoffPlan,
};
use shea_symphony::issue_workspace::{
    discover_issue_workspaces, infer_issue_ref_from_branch_or_path, IssueWorkspaceCandidate,
    IssueWorkspaceReport, WorkspaceMatchStrength,
};
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::model::{LinkedPullRequest, TrackerIssue};
use shea_symphony::ownership::{render_runtime_ownership_marker, RuntimeOwnershipMarker};
use shea_symphony::profiles::selected_execution_profile;
use shea_symphony::runtime_state::RuntimeState;
use shea_symphony::tracker::TrackerAdapter;
use shea_symphony::workspace::run_workspace_command;

use super::IssueExecutionResult;
use crate::orchestration::{current_git_branch, DEFAULT_RUN_LOOP_BASE_BRANCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunLoopLiveHandoff {
    pub(crate) worktree: LiveWorktreeResult,
    pub(crate) publication: PullRequestPublication,
    pub(crate) verification: String,
    pub(crate) project_pr_link_verified: Option<bool>,
    pub(crate) pull_request_ready: Option<PullRequestReadyStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandoffVerification {
    pub(crate) success: bool,
    pub(crate) summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MainLaunchWorkspacePreflight {
    pub(crate) evidence: Vec<String>,
}

pub(crate) fn run_loop_handoff_plan(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueHandoffPlan, HandoffError> {
    let profile = selected_execution_profile(&config.profiles)
        .ok()
        .flatten()
        .map(|profile| profile.workspace_namespace);
    let mut plan = plan_issue_handoff_for_profile(
        &config.workspace.root,
        issue,
        DEFAULT_RUN_LOOP_BASE_BRANCH,
        profile.as_deref(),
    )?;

    if issue.normalized_state() == "rework" {
        if let Ok(repo_root) = std::env::current_dir() {
            if let Ok(report) = discover_issue_workspaces(config, issue, &repo_root) {
                if let Some(candidate) = report
                    .canonical_index
                    .and_then(|index| report.candidates.get(index))
                {
                    if candidate.branch.as_deref() == Some(plan.branch_name.as_str()) {
                        plan.workspace_path = candidate.path.clone();
                    }
                }
            }
        }
    }

    Ok(plan)
}

pub(crate) fn run_loop_preflight_launch_workspace(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &mut IssueHandoffPlan,
) -> Result<MainLaunchWorkspacePreflight, HandoffError> {
    if config.tracker.kind != "github_project_v2" {
        return Ok(MainLaunchWorkspacePreflight {
            evidence: vec![format!(
                "workspace_preflight action=prepare path={} branch={} mode=fixture_or_non_github",
                handoff.workspace_path.display(),
                handoff.branch_name
            )],
        });
    }
    let repo_root =
        std::env::current_dir().map_err(|error| HandoffError::WorkspacePreflightBlocked {
            issue_ref: issue.identifier.clone(),
            reason: format!("cannot inspect current repository worktrees: {error}"),
        })?;
    let report = discover_issue_workspaces(config, issue, &repo_root).map_err(|error| {
        HandoffError::WorkspacePreflightBlocked {
            issue_ref: issue.identifier.clone(),
            reason: error.to_string(),
        }
    })?;
    run_loop_apply_launch_workspace_report(config, issue, handoff, &report)
}

pub(crate) fn run_loop_apply_launch_workspace_report(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &mut IssueHandoffPlan,
    report: &IssueWorkspaceReport,
) -> Result<MainLaunchWorkspacePreflight, HandoffError> {
    let mut evidence = Vec::new();
    for warning in &report.warnings {
        evidence.push(format!("workspace_warning={}", compact_evidence(warning)));
    }

    let (missing_candidates, live_candidates): (Vec<_>, Vec<_>) = report
        .candidates
        .iter()
        .partition(|candidate| !candidate.path.exists());

    for candidate in &missing_candidates {
        evidence.push(format!(
            "ignored_missing_workspace path={} namespace={} evidence={} next=`git worktree prune` after operator review or `workspace ensure`",
            candidate.path.display(),
            workspace_namespace_label(&candidate.path),
            candidate_evidence_summary(candidate)
        ));
    }

    let strong_live = live_candidates
        .iter()
        .copied()
        .filter(|candidate| candidate.strength == WorkspaceMatchStrength::Strong)
        .collect::<Vec<_>>();
    let selected = if strong_live.len() > 1 {
        return Err(workspace_preflight_error(
            issue,
            format!(
                "multiple strong live workspace candidates: {}; run `workspace show` and resolve with `workspace adopt`",
                candidate_path_list(&strong_live)
            ),
        ));
    } else if let Some(candidate) = strong_live.first() {
        Some(*candidate)
    } else if live_candidates.len() > 1 {
        return Err(workspace_preflight_error(
            issue,
            format!(
                "multiple live workspace candidates without a canonical match: {}; run `workspace show` and resolve with `workspace adopt`",
                candidate_path_list(&live_candidates)
            ),
        ));
    } else {
        live_candidates.first().copied()
    };

    if let Some(candidate) = selected {
        validate_launch_candidate_clean(issue, candidate)?;
        let branch = candidate.branch.as_deref().ok_or_else(|| {
            workspace_preflight_error(
                issue,
                format!(
                    "detached workspace candidate {}; resolve with `workspace adopt` after choosing a branch worktree",
                    candidate.path.display()
                ),
            )
        })?;
        let inferred_issue = infer_issue_ref_from_branch_or_path(Some(branch), &candidate.path);
        if inferred_issue.as_deref() != Some(issue.identifier.as_str())
            && branch != handoff.branch_name
        {
            return Err(workspace_preflight_error(
                issue,
                format!(
                    "workspace candidate {} on branch {} does not match issue {}; run `workspace show` before launch",
                    candidate.path.display(),
                    branch,
                    issue.identifier
                ),
            ));
        }
        apply_recovery_worktree_to_handoff(config, issue, handoff, &candidate.path, branch)
            .map_err(|error| workspace_preflight_error(issue, error.to_string()))?;
        evidence.push(format!(
            "workspace_preflight action=reuse path={} branch={} evidence={}",
            candidate.path.display(),
            branch,
            candidate_evidence_summary(candidate)
        ));
    } else {
        evidence.push(format!(
            "workspace_preflight action=prepare path={} branch={} next=`workspace ensure {}`",
            handoff.workspace_path.display(),
            handoff.branch_name,
            issue.identifier
        ));
    }

    Ok(MainLaunchWorkspacePreflight { evidence })
}

pub(crate) fn run_loop_apply_recovery_handoff(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &mut IssueHandoffPlan,
    state: &RuntimeState,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if !matches!(
        state.last_event.as_deref(),
        Some("SessionRunning" | "SessionTerminal")
    ) {
        return Ok(None);
    }

    if let Some(path) = state.workspace_path.as_ref() {
        if let Some(branch) = current_git_branch(path)? {
            apply_recovery_worktree_to_handoff(config, issue, handoff, path, &branch)?;
            return Ok(Some(format!(
                "source=runtime_state workspace={} branch={}",
                path.display(),
                branch
            )));
        }
    }

    let repo_root = std::env::current_dir()?;
    let report = discover_issue_workspaces(config, issue, &repo_root)?;
    if let Some(candidate) = report
        .canonical_index
        .and_then(|index| report.candidates.get(index))
    {
        let Some(branch) = candidate.branch.as_deref() else {
            return Ok(None);
        };
        apply_recovery_worktree_to_handoff(config, issue, handoff, &candidate.path, branch)?;
        return Ok(Some(format!(
            "source=workspace_discovery workspace={} branch={}",
            candidate.path.display(),
            branch
        )));
    }

    Ok(None)
}

fn apply_recovery_worktree_to_handoff(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    handoff: &mut IssueHandoffPlan,
    path: &Path,
    branch: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let inferred_issue = infer_issue_ref_from_branch_or_path(Some(branch), path);
    if inferred_issue.as_deref() != Some(issue.identifier.as_str()) && branch != handoff.branch_name
    {
        return Err(format!(
            "recover refuses worktree {} on branch {}; it does not match issue {}",
            path.display(),
            branch,
            issue.identifier
        )
        .into());
    }

    handoff.workspace_key = recovery_workspace_key(config, path)?;
    handoff.workspace_path = path.to_path_buf();
    handoff.branch_name = branch.to_string();
    handoff.pull_request.head_branch = branch.to_string();
    Ok(())
}

fn validate_launch_candidate_clean(
    issue: &TrackerIssue,
    candidate: &IssueWorkspaceCandidate,
) -> Result<(), HandoffError> {
    if candidate.branch.is_none() {
        return Err(workspace_preflight_error(
            issue,
            format!(
                "workspace candidate {} is detached and cannot be launched safely",
                candidate.path.display()
            ),
        ));
    }
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&candidate.path)
        .output()
        .map_err(|error| {
            workspace_preflight_error(
                issue,
                format!(
                    "could not inspect workspace candidate {}: {error}",
                    candidate.path.display()
                ),
            )
        })?;
    if !output.status.success() {
        return Err(workspace_preflight_error(
            issue,
            format!(
                "could not inspect workspace candidate {}: {}",
                candidate.path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let dirty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !dirty.is_empty() {
        return Err(workspace_preflight_error(
            issue,
            format!(
                "workspace candidate {} is dirty: {}; stop before app-server launch and inspect it",
                candidate.path.display(),
                dirty.replace('\n', "; ")
            ),
        ));
    }
    Ok(())
}

fn workspace_preflight_error(issue: &TrackerIssue, reason: impl Into<String>) -> HandoffError {
    HandoffError::WorkspacePreflightBlocked {
        issue_ref: issue.identifier.clone(),
        reason: reason.into(),
    }
}

fn candidate_path_list(candidates: &[&IssueWorkspaceCandidate]) -> String {
    candidates
        .iter()
        .map(|candidate| candidate.path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn candidate_evidence_summary(candidate: &IssueWorkspaceCandidate) -> String {
    candidate
        .evidence
        .iter()
        .map(|evidence| format!("{}:{}", evidence.source, evidence.detail.replace(' ', "_")))
        .collect::<Vec<_>>()
        .join("|")
}

fn workspace_namespace_label(path: &Path) -> &'static str {
    if path
        .components()
        .any(|component| component.as_os_str() == ".jade-symphony")
    {
        "jade-symphony"
    } else if path
        .components()
        .any(|component| component.as_os_str() == ".shea-symphony")
    {
        "shea-symphony"
    } else {
        "unknown"
    }
}

fn recovery_workspace_key(
    config: &RuntimeConfig,
    path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let root = canonicalize_or_self(&config.workspace.root);
    let path = canonicalize_or_self(path);
    if !path.starts_with(&root) {
        return Err(format!(
            "recover refuses worktree outside configured workspace root: {} not under {}",
            path.display(),
            root.display()
        )
        .into());
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(format!(
            "recover cannot derive workspace key from path {}",
            path.display()
        )
        .into());
    };
    Ok(name.to_string())
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn run_loop_runtime_ownership(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    handoff: &IssueHandoffPlan,
) -> Result<RuntimeOwnershipMarker, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    Ok(RuntimeOwnershipMarker {
        issue_ref: issue.identifier.clone(),
        actor_role: config.identity.actor_role.clone(),
        actor_label: config.identity.actor_label.clone(),
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        workspace_key: handoff.workspace_key.clone(),
        branch_name: handoff.branch_name.clone(),
    })
}

pub(crate) fn run_loop_ownership_workpad(
    issue: &TrackerIssue,
    ownership: &RuntimeOwnershipMarker,
    event: &str,
    claim: &LaneClaim,
) -> String {
    [
        "## Shea Symphony Workpad".to_string(),
        String::new(),
        "### Runtime Ownership".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Event: `{event}`"),
        format!("- Run: `{}`", claim.run),
        format!("- Claim: `{}`", claim.render()),
        "- This marker is advisory tracker-visible ownership for active `In Progress` work.".into(),
        "- Another main loop profile should not resume this issue when the marker differs.".into(),
        String::new(),
        render_runtime_ownership_marker(ownership),
    ]
    .join("\n")
}

pub(crate) fn run_loop_live_handoff_enabled(config: &RuntimeConfig) -> bool {
    config.tracker.kind == "github_project_v2" && config.tracker.fixture_path.is_none()
}

pub(crate) fn run_handoff_verification(
    workspace_path: &Path,
    config: &RuntimeConfig,
) -> HandoffVerification {
    if config.verification.commands.is_empty() {
        return HandoffVerification {
            success: true,
            summary: "skipped:not_configured".into(),
        };
    }

    for (index, command) in config.verification.commands.iter().enumerate() {
        let label = format!("verification:{}", index + 1);
        if let Err(error) = run_workspace_command(
            &label,
            command,
            workspace_path,
            config.verification.timeout_ms,
        ) {
            return HandoffVerification {
                success: false,
                summary: format!(
                    "failed command={} index={} error={}",
                    shell_summary(command),
                    index + 1,
                    compact_evidence(&error.to_string())
                ),
            };
        }
    }

    HandoffVerification {
        success: true,
        summary: format!("passed:{} command(s)", config.verification.commands.len()),
    }
}

fn shell_summary(command: &str) -> String {
    let compact = compact_evidence(command);
    format!("`{compact}`")
}

pub(crate) fn compact_evidence(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 240;
    let truncated = compact.chars().take(LIMIT).collect::<String>();
    if truncated.len() < compact.len() {
        format!("{truncated}...")
    } else {
        compact
    }
}

pub(crate) fn run_loop_handoff_workpad(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    handoff: &IssueHandoffPlan,
    ownership: Option<&RuntimeOwnershipMarker>,
) -> String {
    let mut lines = vec![
        "## Shea Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `shea-symphony main loop`".to_string(),
        String::new(),
        "### Plan".to_string(),
        "- [x] Read the issue contract, Project state, Main Workpad, and timeline evidence."
            .to_string(),
        "- [x] Prepare or resume the isolated issue workspace and branch.".to_string(),
        "- [x] Run the configured Main Agent backend for the implementation slice.".to_string(),
        "- [x] Verify handoff evidence and prepare the PR for Agent Review.".to_string(),
        String::new(),
        "### Work Log".to_string(),
        format!(
            "- Run `{}` executed with backend `{}`.",
            result.run_id.as_deref().unwrap_or("n/a"),
            result.backend
        ),
        format!(
            "- Workspace `{}` was used for implementation evidence.",
            result.workspace_path.display()
        ),
        format!("- Backend message: {}", result.message),
        String::new(),
        "### Run Evidence".to_string(),
        format!("- Run: `{}`", result.run_id.as_deref().unwrap_or("n/a")),
        format!("- Workspace: `{}`", result.workspace_path.display()),
        format!("- Backend: `{}`", result.backend),
        format!(
            "- Profile: `{}`",
            result.profile_id.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Instance: `{}`",
            result.instance_name.as_deref().unwrap_or("n/a")
        ),
        format!("- Actor role: `{}`", result.actor_role),
        format!("- Actor label: `{}`", result.actor_label),
        format!(
            "- Git author: `{}`",
            result.git_author.as_deref().unwrap_or("n/a")
        ),
        format!("- Git identity: `{}`", result.git_identity.summary()),
        format!("- Success: `{}`", result.success),
        format!(
            "- Session: `{}`",
            result.session_id.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Session status: `{}`",
            if result.pending_session {
                "running"
            } else {
                "terminal"
            }
        ),
        format!(
            "- Attach command: `{}`",
            result.backend_attach_command.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Session log: `{}`",
            result
                .backend_log_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "n/a".into())
        ),
        format!("- Message: {}", result.message),
        String::new(),
        "### Planned Handoff".to_string(),
        format!("- Workspace key: `{}`", handoff.workspace_key),
        format!("- Workspace path: `{}`", handoff.workspace_path.display()),
        format!("- Branch: `{}`", handoff.branch_name),
        format!("- PR title: `{}`", handoff.pull_request.title),
        format!("- PR base branch: `{}`", handoff.pull_request.base_branch),
        format!("- Branch target role: `{:?}`", handoff.branch_target.role),
        branch_target_workpad_line(handoff),
        rework_continuation_workpad_line(handoff),
        handoff_verification_workpad_line(result),
        live_handoff_workpad_line(result),
        String::new(),
        "### Main-Agent Boundary".to_string(),
        "- Locally complete main-agent work stops at `Agent Review`.".to_string(),
        "- `Human Review` is reserved for independent Review Agent pass evidence.".to_string(),
    ];

    if let Some(ownership) = ownership {
        lines.push(String::new());
        lines.push(render_runtime_ownership_marker(ownership));
    }

    lines.join("\n")
}

fn branch_target_workpad_line(handoff: &IssueHandoffPlan) -> String {
    let mut parts = Vec::new();
    if let Some(parent_issue) = &handoff.branch_target.parent_issue {
        parts.push(format!("native_parent={parent_issue}"));
    }
    if let Some(parent_integration_branch) = &handoff.branch_target.parent_integration_branch {
        parts.push(format!(
            "parent_integration_branch={parent_integration_branch}"
        ));
    }
    if let Some(parent_final_base_branch) = &handoff.branch_target.parent_final_base_branch {
        parts.push(format!("parent_final_base={parent_final_base_branch}"));
    }

    if parts.is_empty() {
        "- Branch target evidence: `single-issue default`".to_string()
    } else {
        format!("- Branch target evidence: `{}`", parts.join(" "))
    }
}

fn rework_continuation_workpad_line(handoff: &IssueHandoffPlan) -> String {
    match &handoff.continuation {
        Some(continuation) => format!(
            "- Rework continuation: `{}` from `{}` ({}) branch=`{}`",
            continuation.pull_request_url,
            continuation.source,
            continuation.pull_request_state,
            continuation.branch_name.as_deref().unwrap_or("unknown")
        ),
        None => "- Rework continuation: `not-used`".to_string(),
    }
}

fn handoff_verification_workpad_line(result: &IssueExecutionResult) -> String {
    format!(
        "- Handoff verification: `{}`",
        result
            .handoff_verification
            .as_deref()
            .unwrap_or("skipped:not_run")
    )
}

fn live_handoff_workpad_line(result: &IssueExecutionResult) -> String {
    match &result.live_handoff {
        Some(handoff) => {
            let ready = handoff
                .pull_request_ready
                .as_ref()
                .map(|status| {
                    format!(
                        "ready-check: `was_draft={} marked_ready={}`",
                        status.was_draft, status.marked_ready
                    )
                })
                .unwrap_or_else(|| "ready-check: `not-run`".into());
            format!(
                "- Live PR: `{}` (created: `{}`, branch pushed: `{}`, verification: `{}`, {})",
                handoff.publication.pr_url,
                handoff.publication.pr_created,
                handoff.publication.branch_pushed,
                handoff.verification,
                ready
            )
        }
        None => "- Live PR: `not-created`".to_string(),
    }
}

fn record_live_handoff_pr_link(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    result: &IssueExecutionResult,
) -> Result<(), String> {
    let Some(handoff) = &result.live_handoff else {
        return Ok(());
    };

    let linked = adapter
        .list_linked_pull_requests(issue_ref)
        .map_err(|error| format!("handoff PR link verification failed: {error}"))?;
    if native_linked_pull_requests_contain(&linked, &handoff.publication.pr_url) {
        return Ok(());
    }

    adapter
        .link_pull_request(issue_ref, &handoff.publication.pr_url)
        .map_err(|error| format!("handoff PR link repair failed: {error}"))?;

    let linked = adapter
        .list_linked_pull_requests(issue_ref)
        .map_err(|error| format!("handoff PR link verification failed: {error}"))?;

    if native_linked_pull_requests_contain(&linked, &handoff.publication.pr_url) {
        Ok(())
    } else {
        let fallback_visible = linked_pull_requests_contain(&linked, &handoff.publication.pr_url);
        Err(format!(
            "GitHub-native linked PR was not visible after repair attempt: {}; fallback_diagnostic_visible={}",
            handoff.publication.pr_url,
            fallback_visible
        ))
    }
}

pub(crate) fn apply_live_handoff_pr_link(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    result: &mut IssueExecutionResult,
) -> bool {
    if result.live_handoff.is_none() {
        return false;
    }

    match record_live_handoff_pr_link(adapter, issue_ref, result) {
        Ok(()) => {
            if let Some(handoff) = result.live_handoff.as_mut() {
                handoff.project_pr_link_verified = Some(true);
            }
            true
        }
        Err(error) => {
            if let Some(handoff) = result.live_handoff.as_mut() {
                handoff.project_pr_link_verified = Some(false);
            }
            result.success = false;
            result.message = error;
            false
        }
    }
}

pub(crate) fn linked_pull_requests_contain(
    linked_pull_requests: &[LinkedPullRequest],
    pr_url: &str,
) -> bool {
    linked_pull_requests
        .iter()
        .any(|linked| linked_pull_request_matches(linked, pr_url))
}

pub(crate) fn native_linked_pull_requests_contain(
    linked_pull_requests: &[LinkedPullRequest],
    pr_url: &str,
) -> bool {
    linked_pull_requests
        .iter()
        .filter(|linked| linked.is_github_native_linkage())
        .any(|linked| linked_pull_request_matches(linked, pr_url))
}

fn linked_pull_request_matches(linked: &LinkedPullRequest, pr_url: &str) -> bool {
    let expected_url = pr_url.trim();
    let expected_number = pull_request_number_from_ref(expected_url);
    linked
        .url
        .as_deref()
        .is_some_and(|url| url.trim() == expected_url)
        || expected_number.is_some() && linked.number == expected_number
}

fn pull_request_number_from_ref(reference: &str) -> Option<u64> {
    pull_request_number_from_url(reference).or_else(|| {
        reference
            .trim()
            .trim_start_matches('#')
            .trim_start_matches("PR_")
            .parse()
            .ok()
    })
}

pub(crate) fn pull_request_number_from_url(url: &str) -> Option<u64> {
    url.trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|segment| segment.parse().ok())
}

pub(crate) fn run_loop_agent_review_handoff_evidence(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    handoff: &IssueHandoffPlan,
    main_workpad_markdown: Option<&str>,
) -> AgentReviewHandoffEvidence {
    let mut evidence = AgentReviewHandoffEvidence::from_plan(
        handoff,
        format!(
            "backend={} success={} session={} message={}",
            result.backend,
            result.success,
            result.session_id.as_deref().unwrap_or("n/a"),
            result.message
        ),
        "main agent completed local run",
    );
    evidence.record_main_workpad_markdown(main_workpad_markdown);
    evidence.pull_request_url = result
        .live_handoff
        .as_ref()
        .map(|handoff| handoff.publication.pr_url.clone())
        .or_else(|| {
            issue
                .linked_pull_requests
                .iter()
                .find_map(|pr| pr.url.clone())
        });
    evidence.pull_request_is_draft = result
        .live_handoff
        .as_ref()
        .and_then(|handoff| {
            handoff
                .pull_request_ready
                .as_ref()
                .map(|ready| ready.was_draft && !ready.marked_ready)
        })
        .or_else(|| {
            let url = evidence.pull_request_url.as_deref()?;
            issue
                .linked_pull_requests
                .iter()
                .find(|pr| pr.url.as_deref() == Some(url))
                .and_then(|pr| pr.is_draft)
        });
    evidence.project_pr_link_verified = result
        .live_handoff
        .as_ref()
        .and_then(|handoff| handoff.project_pr_link_verified)
        .or_else(|| {
            let url = evidence.pull_request_url.as_deref()?;
            Some(native_linked_pull_requests_contain(
                &issue.linked_pull_requests,
                url,
            ))
        });
    if evidence.pull_request_url.is_none() {
        evidence.no_pr_blocker = Some(
            "No pull request URL was present in tracker data at handoff time; keeping issue out of Agent Review until PR evidence is durable.".into(),
        );
    }
    evidence
}

pub(crate) fn run_loop_handoff_failure_workpad(
    issue: &TrackerIssue,
    error: &HandoffError,
) -> String {
    [
        "## Shea Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `shea-symphony main loop`".to_string(),
        String::new(),
        "### Handoff Planning Blocker".to_string(),
        format!("- Error: `{error}`"),
        "- Backend execution was skipped before claim/run to avoid mixing issue scope.".to_string(),
        String::new(),
        "### Required Human Decision".to_string(),
        "- Confirm the correct branch/workspace ownership before retrying.".to_string(),
    ]
    .join("\n")
}

pub(crate) fn run_loop_assignee_ownership_workpad(issue: &TrackerIssue, reason: &str) -> String {
    [
        "## Shea Symphony Workpad".to_string(),
        String::new(),
        "### Assignee Ownership Blocker".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Reason: {reason}"),
        format!("- Issue assignees: `{}`", issue.assignees.join(", ")),
        String::new(),
        "### Boundary".to_string(),
        "- Shea Symphony did not claim this issue or move it to `In Progress`.".to_string(),
        "- Assign the issue to the active GitHub identity or selected execution profile before retrying.".to_string(),
    ]
    .join("\n")
}

pub(crate) fn run_loop_usage_limit_pause_workpad(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    pause: &UsageLimitPause,
    retry_delay_ms: u64,
) -> String {
    [
        "## Shea Symphony Workpad".to_string(),
        String::new(),
        "### Usage-Limit Pause".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `shea-symphony main loop`".to_string(),
        format!("- Backend: `{}`", result.backend),
        format!("- Classifier: `{}`", pause.classifier),
        format!("- Evidence: {}", pause.evidence),
        format!("- Retry backoff: `{retry_delay_ms}ms`"),
        String::new(),
        "### State Safety".to_string(),
        "- Tracker state was not advanced to `Agent Review`.".to_string(),
        "- Runtime state keeps the active issue and next retry time.".to_string(),
        "- The main loop will skip this issue until retry backoff expires or an operator intervenes."
            .to_string(),
    ]
    .join("\n")
}
