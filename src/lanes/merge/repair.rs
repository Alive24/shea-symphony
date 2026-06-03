use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::merge_lane::{MergeConflictRepairOutcome, MergeRepairEvidence};
use shea_symphony::model::TrackerIssue;
use shea_symphony::workflow::WorkflowDefinition;

use crate::commands::session::{
    agent_session_backend, agent_session_backend_spec, record_agent_session_events,
    rendered_lane_prompt_artifact_path, AgentSessionLaneArg,
};
use crate::orchestration::single_line;

mod agent_contract;
mod outcome;

use agent_contract::{
    agent_events_text, merge_agent_conflict_repair_prompt, merge_agent_resolution_summary,
    merge_agent_semantic_safety,
};
pub(crate) use agent_contract::{merge_agent_reports_repaired, merge_agent_requests_human_input};
pub(crate) use outcome::finish_merge_agent_repaired_branch;
use outcome::{
    merge_agent_repair_backend_failed, merge_agent_repair_blocked,
    merge_agent_repair_retryable_verification_failed, merge_agent_repair_semantic_uncertainty,
    merge_agent_repair_verification_failed,
};

pub(crate) struct MergeAgentConflictRepairOutcome {
    pub(crate) repaired: bool,
    pub(crate) retryable: bool,
    pub(super) output: CommandOutput,
    pub(crate) evidence: MergeRepairEvidence,
    pub(super) reason: String,
    pub(super) backend: String,
    pub(super) session_id: Option<String>,
}

pub(super) fn mechanical_merge_repair_evidence(
    repair: &MergeConflictRepairOutcome,
    expected_base: &str,
) -> MergeRepairEvidence {
    MergeRepairEvidence {
        method: "mechanical_git_merge".into(),
        conflict_summary: format!(
            "`git merge --no-edit origin/{expected_base}` completed without content conflicts"
        ),
        resolution_summary: repair.reason.clone(),
        semantic_safety: "No agent-authored changes were needed; Git produced a clean merge commit from the approved PR branch and current base.".into(),
        verification: "`git status --porcelain` was clean after the merge commit; push was attempted only after that clean check.".into(),
        push_evidence: format!(
            "push exit status `{}` stdout=`{}` stderr=`{}`",
            repair.output.status,
            single_line(&repair.output.stdout),
            single_line(&repair.output.stderr)
        ),
        next_state_rationale: "Successful repair stays in `Merging` so a later merge tick rereads GitHub mergeability before landing.".into(),
    }
}

pub(super) fn ineligible_merge_agent_repair_evidence(
    repair: &MergeConflictRepairOutcome,
) -> MergeRepairEvidence {
    MergeRepairEvidence {
        method: "not_started".into(),
        conflict_summary: repair.reason.clone(),
        resolution_summary:
            "Merge-agent repair was not started because trusted repair preconditions were not met."
                .into(),
        semantic_safety:
            "Without a trusted clean PR worktree and content-conflict evidence, the merge lane cannot prove branch safety."
                .into(),
        verification: "No agent verification ran.".into(),
        push_evidence: "No push attempted.".into(),
        next_state_rationale:
            "Unsafe or untrusted repair preconditions route to `Need Human Input` with one operator question."
                .into(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_merge_agent_conflict_repair(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    claim: &LaneClaim,
    pr_ref: &str,
    head_ref_name: &str,
    expected_base: &str,
    mechanical_repair: &MergeConflictRepairOutcome,
    runner: &dyn HandoffCommandRunner,
) -> Result<MergeAgentConflictRepairOutcome, Box<dyn std::error::Error>> {
    let Some(worktree_path) = mechanical_repair.worktree_path.as_ref() else {
        return Ok(merge_agent_repair_blocked(
            "missing trusted PR worktree after mechanical content-conflict repair failed",
            mechanical_repair,
        ));
    };

    let clean_after_abort = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        worktree_path,
    )?;
    if clean_after_abort.status != 0 || !clean_after_abort.stdout.trim().is_empty() {
        return Ok(merge_agent_repair_blocked(
            "PR worktree was not clean after aborting the failed mechanical merge",
            mechanical_repair,
        ));
    }

    let fetch_ref = format!("origin/{expected_base}");
    let fetch = runner.run(
        "git",
        &["fetch".into(), "origin".into(), expected_base.into()],
        worktree_path,
    )?;
    if fetch.status != 0 {
        return Ok(merge_agent_repair_blocked(
            "merge-agent repair could not refresh the expected base branch",
            mechanical_repair,
        ));
    }

    let conflict_merge = runner.run(
        "git",
        &["merge".into(), "--no-edit".into(), fetch_ref.clone()],
        worktree_path,
    )?;
    if conflict_merge.status == 0 {
        return finish_merge_agent_repaired_branch(
            config,
            issue,
            "mechanical_retry",
            "The second base merge completed before agent edits were needed.",
            "`git merge --no-edit` completed cleanly on retry.",
            "No merge-agent semantic changes were needed.",
            vec!["git merge --no-edit".into()],
            pr_ref,
            head_ref_name,
            runner,
            worktree_path,
            CommandOutput {
                status: 0,
                stdout: conflict_merge.stdout,
                stderr: conflict_merge.stderr,
            },
            "direct-cli".into(),
            None,
        );
    }

    let conflict_files = runner.run(
        "git",
        &[
            "diff".into(),
            "--name-only".into(),
            "--diff-filter=U".into(),
        ],
        worktree_path,
    )?;
    let conflict_summary = if conflict_files.stdout.trim().is_empty() {
        format!(
            "Git reported conflicts while merging `{fetch_ref}`, but no unmerged files were listed."
        )
    } else {
        format!(
            "Conflicted files after merging `{fetch_ref}`: `{}`",
            single_line(&conflict_files.stdout)
        )
    };

    let prompt = merge_agent_conflict_repair_prompt(
        workflow,
        issue,
        claim,
        pr_ref,
        head_ref_name,
        expected_base,
        &conflict_summary,
        &mechanical_repair.output,
    )?;
    let backend_spec = agent_session_backend_spec(config, AgentSessionLaneArg::Merge)?;
    let backend = agent_session_backend(&backend_spec.backend)?;
    let prompt_path = rendered_lane_prompt_artifact_path(
        config,
        issue,
        AgentSessionLaneArg::Merge,
        1,
        &backend_spec.backend,
    );
    let mut prepared = backend.prepare(worktree_path.clone(), prompt, config)?;
    prepared.command = Some(backend_spec.command.clone());
    prepared.prompt_artifact_path = Some(prompt_path.clone());
    prepared.issue_id = Some(issue.id.clone());
    prepared.issue_identifier = Some(issue.identifier.clone());
    prepared.issue_title = Some(issue.title.clone());
    prepared.lane = Some("merge".into());
    prepared.run_id = Some(claim.run.clone());
    prepared.branch_name = Some(head_ref_name.into());
    prepared
        .env
        .insert("SHEA_SYMPHONY_AGENT_LANE".into(), "merge".into());
    prepared
        .env
        .insert("SHEA_SYMPHONY_RUN_ID".into(), claim.run.clone());
    prepared
        .env
        .insert("SHEA_SYMPHONY_CLAIM".into(), claim.render());

    let events = match backend.run(prepared) {
        Ok(events) => events,
        Err(error) => {
            let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
            return Ok(merge_agent_repair_backend_failed(
                &backend_spec.backend,
                format!("merge-agent backend unavailable: {error}"),
                &conflict_summary,
            ));
        }
    };
    let summary = backend.summarize(&events);
    record_agent_session_events(
        config,
        issue,
        AgentSessionLaneArg::Merge,
        &summary,
        &events,
        &prompt_path,
    )?;

    let agent_text = agent_events_text(&events);
    if !summary.success {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
        return Ok(merge_agent_repair_backend_failed(
            &summary.backend,
            format!("merge-agent backend did not complete: {}", summary.message),
            &conflict_summary,
        ));
    }
    if merge_agent_requests_human_input(&agent_text) {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
        return Ok(merge_agent_repair_semantic_uncertainty(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            &agent_text,
        ));
    }
    if !merge_agent_reports_repaired(&agent_text) {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
        return Ok(merge_agent_repair_semantic_uncertainty(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            "merge-agent completed without the required MERGE_AGENT_DECISION marker",
        ));
    }

    if let Err(reason) = stage_resolved_merge_agent_changes(runner, worktree_path) {
        abort_merge_repair_if_active(runner, worktree_path);
        let outcome = match reason {
            MergeAgentStageFailure::Unsafe(reason) => merge_agent_repair_verification_failed(
                &summary.backend,
                summary.session_id.clone(),
                &conflict_summary,
                reason,
            ),
            MergeAgentStageFailure::Retryable(reason) => {
                merge_agent_repair_retryable_verification_failed(
                    &summary.backend,
                    summary.session_id.clone(),
                    &conflict_summary,
                    reason,
                )
            }
        };
        return Ok(outcome);
    }
    let merge_head = runner.run(
        "git",
        &[
            "rev-parse".into(),
            "-q".into(),
            "--verify".into(),
            "MERGE_HEAD".into(),
        ],
        worktree_path,
    )?;
    if merge_head.status == 0 {
        let commit = runner.run("git", &["commit".into(), "--no-edit".into()], worktree_path)?;
        if commit.status != 0 {
            abort_merge_repair_if_active(runner, worktree_path);
            return Ok(merge_agent_repair_retryable_verification_failed(
                &summary.backend,
                summary.session_id.clone(),
                &conflict_summary,
                format!(
                    "`git commit --no-edit` failed: stdout=`{}` stderr=`{}`",
                    single_line(&commit.stdout),
                    single_line(&commit.stderr)
                ),
            ));
        }
    }

    finish_merge_agent_repaired_branch(
        config,
        issue,
        "merge_agent",
        &conflict_summary,
        &merge_agent_resolution_summary(&agent_text),
        &merge_agent_semantic_safety(&agent_text),
        vec![
            "git diff --name-only --diff-filter=U".into(),
            "git diff --check".into(),
            "git status --porcelain".into(),
        ],
        pr_ref,
        head_ref_name,
        runner,
        worktree_path,
        CommandOutput {
            status: 0,
            stdout: summary.message.clone(),
            stderr: String::new(),
        },
        summary.backend,
        summary.session_id,
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MergeAgentStageFailure {
    Retryable(String),
    Unsafe(String),
}

pub(crate) fn stage_resolved_merge_agent_changes(
    runner: &dyn HandoffCommandRunner,
    worktree_path: &std::path::Path,
) -> Result<(), MergeAgentStageFailure> {
    let diff_check = runner
        .run("git", &["diff".into(), "--check".into()], worktree_path)
        .map_err(|error| MergeAgentStageFailure::Retryable(error.to_string()))?;
    if diff_check.status != 0 {
        return Err(MergeAgentStageFailure::Retryable(format!(
            "`git diff --check` failed: stdout=`{}` stderr=`{}`",
            single_line(&diff_check.stdout),
            single_line(&diff_check.stderr)
        )));
    }

    let pre_commit_status = runner
        .run(
            "git",
            &["status".into(), "--porcelain".into()],
            worktree_path,
        )
        .map_err(|error| MergeAgentStageFailure::Retryable(error.to_string()))?;
    if pre_commit_status
        .stdout
        .lines()
        .any(|line| line.starts_with("??"))
    {
        return Err(MergeAgentStageFailure::Unsafe(
            "merge-agent left untracked files in the PR worktree".into(),
        ));
    }

    let add = runner
        .run("git", &["add".into(), "-A".into()], worktree_path)
        .map_err(|error| MergeAgentStageFailure::Retryable(error.to_string()))?;
    if add.status != 0 {
        return Err(MergeAgentStageFailure::Retryable(
            "`git add -A` failed after conflict resolution".into(),
        ));
    }

    let unresolved = runner
        .run(
            "git",
            &[
                "diff".into(),
                "--name-only".into(),
                "--diff-filter=U".into(),
            ],
            worktree_path,
        )
        .map_err(|error| MergeAgentStageFailure::Retryable(error.to_string()))?;
    if unresolved.status != 0 || !unresolved.stdout.trim().is_empty() {
        return Err(MergeAgentStageFailure::Retryable(format!(
            "unresolved conflict files remain after staging: `{}`",
            single_line(&unresolved.stdout)
        )));
    }

    Ok(())
}

fn abort_merge_repair_if_active(
    runner: &dyn HandoffCommandRunner,
    worktree_path: &std::path::Path,
) {
    let merge_head = runner.run(
        "git",
        &[
            "rev-parse".into(),
            "-q".into(),
            "--verify".into(),
            "MERGE_HEAD".into(),
        ],
        worktree_path,
    );
    if merge_head.as_ref().is_ok_and(|output| output.status == 0) {
        let _ = runner.run("git", &["merge".into(), "--abort".into()], worktree_path);
    }
}
