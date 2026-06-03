use std::path::Path;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
use shea_symphony::merge_lane::{MergeConflictRepairOutcome, MergeRepairEvidence};
use shea_symphony::model::TrackerIssue;

use crate::orchestration::single_line;

use super::MergeAgentConflictRepairOutcome;

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
        retryable: false,
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

pub(super) fn merge_agent_repair_blocked(
    reason: &str,
    mechanical_repair: &MergeConflictRepairOutcome,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        retryable: false,
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

pub(super) fn merge_agent_repair_backend_failed(
    backend: &str,
    reason: String,
    conflict_summary: &str,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        retryable: true,
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
                "Keep the issue in `Merging` for retry because the repair backend did not reach a semantic decision."
                    .into(),
        },
        reason,
        backend: backend.into(),
        session_id: None,
    }
}

pub(super) fn merge_agent_repair_semantic_uncertainty(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: &str,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        retryable: false,
        output: CommandOutput {
            status: 1,
            stdout: reason.into(),
            stderr: String::new(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_semantic_uncertainty".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: single_line(reason),
            semantic_safety: "The merge-agent did not provide a positive semantic-safety proof."
                .into(),
            verification:
                "Repair verification was skipped or incomplete because semantic safety was uncertain."
                    .into(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale:
                "Route to `Need Human Input` with a concrete semantic-safety question.".into(),
        },
        reason: "merge-agent repair could not prove semantic safety".into(),
        backend: backend.into(),
        session_id,
    }
}

pub(super) fn merge_agent_repair_verification_failed(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: String,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        retryable: false,
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

pub(super) fn merge_agent_repair_retryable_verification_failed(
    backend: &str,
    session_id: Option<String>,
    conflict_summary: &str,
    reason: String,
) -> MergeAgentConflictRepairOutcome {
    MergeAgentConflictRepairOutcome {
        repaired: false,
        retryable: true,
        output: CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: reason.clone(),
        },
        evidence: MergeRepairEvidence {
            method: "merge_agent_retryable_verification_failed".into(),
            conflict_summary: conflict_summary.into(),
            resolution_summary: reason.clone(),
            semantic_safety:
                "The merge lane cleaned up the interrupted repair attempt before retrying."
                    .into(),
            verification: reason.clone(),
            push_evidence: "No push attempted.".into(),
            next_state_rationale: "Keep the issue in `Merging` because the failed repair attempt was cleaned up and can be retried automatically.".into(),
        },
        reason,
        backend: backend.into(),
        session_id,
    }
}
