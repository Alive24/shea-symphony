use std::path::Path;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
use jade_symphony::lane_claim::LaneClaim;
use jade_symphony::merge_lane::{MergeConflictRepairOutcome, MergeRepairEvidence};
use jade_symphony::model::{AgentEvent, TrackerIssue};
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::{
    agent_session_backend, agent_session_backend_spec, record_agent_session_events,
    render_prompt_with_claim, rendered_lane_prompt_artifact_path, single_line, AgentSessionLaneArg,
};

pub(crate) struct MergeAgentConflictRepairOutcome {
    pub(crate) repaired: bool,
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
        .insert("JADE_SYMPHONY_AGENT_LANE".into(), "merge".into());
    prepared
        .env
        .insert("JADE_SYMPHONY_RUN_ID".into(), claim.run.clone());
    prepared
        .env
        .insert("JADE_SYMPHONY_CLAIM".into(), claim.render());

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

    let unresolved = runner.run(
        "git",
        &[
            "diff".into(),
            "--name-only".into(),
            "--diff-filter=U".into(),
        ],
        worktree_path,
    )?;
    if unresolved.status != 0 || !unresolved.stdout.trim().is_empty() {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            format!(
                "unresolved conflict files remain: `{}`",
                single_line(&unresolved.stdout)
            ),
        ));
    }

    let diff_check = runner.run("git", &["diff".into(), "--check".into()], worktree_path)?;
    if diff_check.status != 0 {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            format!(
                "`git diff --check` failed: stdout=`{}` stderr=`{}`",
                single_line(&diff_check.stdout),
                single_line(&diff_check.stderr)
            ),
        ));
    }

    let pre_commit_status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        worktree_path,
    )?;
    if pre_commit_status
        .stdout
        .lines()
        .any(|line| line.starts_with("??"))
    {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            "merge-agent left untracked files in the PR worktree".into(),
        ));
    }

    let add = runner.run("git", &["add".into(), "-A".into()], worktree_path)?;
    if add.status != 0 {
        return Ok(merge_agent_repair_verification_failed(
            &summary.backend,
            summary.session_id.clone(),
            &conflict_summary,
            "`git add -A` failed after conflict resolution".into(),
        ));
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
            return Ok(merge_agent_repair_verification_failed(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_merge_agent_repaired_branch(
    _config: &RuntimeConfig,
    _issue: &TrackerIssue,
    method: &str,
    conflict_summary: &str,
    resolution_summary: &str,
    semantic_safety: &str,
    verification_commands: Vec<String>,
    _pr_ref: &str,
    head_ref_name: &str,
    runner: &dyn HandoffCommandRunner,
    worktree_path: &Path,
    initial_output: CommandOutput,
    backend: String,
    session_id: Option<String>,
) -> Result<MergeAgentConflictRepairOutcome, Box<dyn std::error::Error>> {
    let post_status = runner.run(
        "git",
        &["status".into(), "--porcelain".into()],
        worktree_path,
    )?;
    if post_status.status != 0 || !post_status.stdout.trim().is_empty() {
        return Ok(merge_agent_repair_verification_failed(
            &backend,
            session_id,
            conflict_summary,
            format!(
                "repaired branch was not clean before push: `{}`",
                single_line(&post_status.stdout)
            ),
        ));
    }
    let push = runner.run(
        "git",
        &["push".into(), "origin".into(), head_ref_name.into()],
        worktree_path,
    )?;
    if push.status != 0 {
        return Ok(merge_agent_repair_verification_failed(
            &backend,
            session_id,
            conflict_summary,
            format!(
                "push failed: stdout=`{}` stderr=`{}`",
                single_line(&push.stdout),
                single_line(&push.stderr)
            ),
        ));
    }
    Ok(MergeAgentConflictRepairOutcome {
        repaired: true,
        output: CommandOutput {
            status: 0,
            stdout: format!(
                "{}\n{}",
                single_line(&initial_output.stdout),
                single_line(&push.stdout)
            ),
            stderr: single_line(&push.stderr),
        },
        evidence: MergeRepairEvidence {
            method: method.into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: resolution_summary.into(),
            semantic_safety: semantic_safety.into(),
            verification: verification_commands.join("; "),
            push_evidence: format!(
                "`git push origin {head_ref_name}` exit status `{}`",
                push.status
            ),
            next_state_rationale: "Successful merge-agent repair stays in `Merging` so the next merge tick rereads GitHub mergeability before landing.".into(),
        },
        reason: "merge-agent repaired the conflicted approved PR branch, verification passed, and the existing branch was pushed".into(),
        backend,
        session_id,
    })
}

fn merge_agent_repair_blocked(
    reason: &str,
    mechanical_repair: &MergeConflictRepairOutcome,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: mechanical_repair.output.clone(),
        evidence: MergeRepairEvidence {
            method: "merge_agent_not_started".into(),
            conflict_summary: mechanical_repair.reason.clone(),
            resolution_summary: reason.into(),
            semantic_safety: "Trusted repair preconditions failed before the merge-agent could safely edit files.".into(),
            verification: "No agent verification ran.".into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Route to `Need Human Input` because the merge lane cannot prove safe branch repair.".into(),
        },
        reason: reason.into(),
        backend: "not-started".into(),
        session_id: None,
    }
}

fn merge_agent_repair_backend_failed(
    backend: &str,
    reason: String,
    conflict_summary: &str,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: reason.clone(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_backend".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: reason.clone(),
            semantic_safety: "Backend failure prevents semantic-safety proof.".into(),
            verification: "No completed repair verification.".into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale:
                "Route to `Need Human Input` because the repair backend could not complete safely."
                    .into(),
        },
        reason,
        backend: backend.into(),
        session_id: None,
    }
}

fn merge_agent_repair_semantic_uncertainty(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: &str,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: CommandOutput {
            status: 1,
            stdout: reason.into(),
            stderr: String::new(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_semantic_uncertainty".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: single_line(reason),
            semantic_safety: "The merge-agent did not provide a positive semantic-safety proof.".into(),
            verification: "Repair verification was skipped or incomplete because semantic safety was uncertain.".into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Route to `Need Human Input` with a concrete semantic-safety question.".into(),
        },
        reason: "merge-agent repair could not prove semantic safety".into(),
        backend: backend.into(),
        session_id,
    }
}

fn merge_agent_repair_verification_failed(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: String,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        output: CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: reason.clone(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_verification_failed".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: reason.clone(),
            semantic_safety: "Verification failure prevents treating the repair as safe.".into(),
            verification: reason.clone(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Route to `Need Human Input` because the repaired branch was not clean and verified.".into(),
        },
        reason,
        backend: backend.into(),
        session_id,
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_agent_conflict_repair_prompt(
    workflow: &WorkflowDefinition,
    issue: &TrackerIssue,
    claim: &LaneClaim,
    pr_ref: &str,
    head_ref_name: &str,
    expected_base: &str,
    conflict_summary: &str,
    mechanical_output: &CommandOutput,
) -> Result<String, jade_symphony::prompt::PromptError> {
    let mut prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(AgentLane::MergeAgent),
        issue,
        None,
        Some(claim),
    )?;
    prompt.push_str(
        "\n\n## Merge-Agent Conflict Repair Task\n\n\
You are repairing the existing approved PR branch in place. Preserve the intent that already passed Agent Review and Human Review. Resolve only conflicts caused by merging the target base into this PR branch. Do not create a replacement PR, do not switch workspaces, and do not route through Rework.\n\n",
    );
    prompt.push_str(&format!("- Pull request: `{pr_ref}`\n"));
    prompt.push_str(&format!("- Head branch: `{head_ref_name}`\n"));
    prompt.push_str(&format!("- Expected base: `{expected_base}`\n"));
    prompt.push_str(&format!("- Conflict summary: {conflict_summary}\n"));
    prompt.push_str(&format!(
        "- Mechanical merge stderr: `{}`\n",
        single_line(&mechanical_output.stderr)
    ));
    prompt.push_str(
        "\n### Required Output Marker\n\n\
End your final response with one of these exact markers:\n\
- `MERGE_AGENT_DECISION: repaired` only if the resolution preserves reviewed intent and verification can proceed.\n\
- `MERGE_AGENT_DECISION: needs_human_input` if there is semantic uncertainty, unrelated drift, unsafe branch/worktree state, or missing verification confidence.\n\n\
Also include `RESOLUTION_SUMMARY:` and `SEMANTIC_SAFETY:` lines. Leave the merge resolution staged or ready for `git add -A`; the merge lane will commit, verify cleanliness, push, and keep the issue in `Merging` for the next tick.\n",
    );
    Ok(prompt)
}

fn agent_events_text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Message { text, .. } => Some(text.as_str()),
            AgentEvent::Completed { summary, .. } => Some(summary.as_str()),
            AgentEvent::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn merge_agent_reports_repaired(text: &str) -> bool {
    text.contains("MERGE_AGENT_DECISION: repaired")
}

pub(crate) fn merge_agent_requests_human_input(text: &str) -> bool {
    text.contains("MERGE_AGENT_DECISION: needs_human_input")
        || text.to_ascii_lowercase().contains("semantic uncertainty")
}

fn merge_agent_resolution_summary(text: &str) -> String {
    marker_line(text, "RESOLUTION_SUMMARY:")
        .unwrap_or_else(|| "Merge-agent reported repaired conflict resolution.".into())
}

fn merge_agent_semantic_safety(text: &str) -> String {
    marker_line(text, "SEMANTIC_SAFETY:").unwrap_or_else(|| {
        "Merge-agent reported that reviewed implementation intent was preserved.".into()
    })
}

fn marker_line(text: &str, marker: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(marker).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
