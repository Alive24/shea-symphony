use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::ProcessHandoffCommandRunner;
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::merge_lane::{
    merge_lane_workpad_with_repair_evidence, repair_dirty_pull_request, MergeLaneDecision,
    PullRequestMergeStatus,
};
use shea_symphony::model::{LinkedPullRequest, TrackerIssue};
use shea_symphony::tracker::TrackerAdapter;
use shea_symphony::workflow::WorkflowDefinition;

use super::super::evidence::{
    record_merge_timeline_comment_with_recovery, set_merge_state_with_recovery,
};
use super::super::repair::{
    ineligible_merge_agent_repair_evidence, mechanical_merge_repair_evidence,
    run_merge_agent_conflict_repair,
};
use super::{merge_rehearsal_mode, MergeOnceOutcome, MergeTickOutputScope};

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_dirty_merge(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    merge_claim: &LaneClaim,
    decision: &MergeLaneDecision,
    status: Option<&PullRequestMergeStatus>,
    linked_pull_requests: &[LinkedPullRequest],
    expected_base: &str,
    runner: &ProcessHandoffCommandRunner,
    output_scope: MergeTickOutputScope,
) -> Result<MergeOnceOutcome, Box<dyn std::error::Error>> {
    let pr_ref = decision
        .pr_url
        .as_deref()
        .ok_or("dirty-merge decision missing pull request URL")?;
    let head_ref_name = status
        .and_then(|status| status.head_ref_name.as_deref())
        .or_else(|| {
            linked_pull_requests
                .first()
                .and_then(|pull_request| pull_request.head_ref_name.as_deref())
        });
    let repair = repair_dirty_pull_request(
        pr_ref,
        head_ref_name,
        expected_base,
        runner,
        &std::env::current_dir()?,
        merge_rehearsal_mode(config, issue),
    )?;
    if repair.repaired {
        let mut repaired_decision = decision.clone();
        repaired_decision.reason = repair.reason.clone();
        let evidence = mechanical_merge_repair_evidence(&repair, expected_base);
        let workpad = merge_lane_workpad_with_repair_evidence(
            issue,
            &repaired_decision,
            Some(&repair.output),
            Some(&evidence),
        );
        let comment_outcome = record_merge_timeline_comment_with_recovery(
            config,
            adapter,
            issue,
            &repaired_decision,
            &workpad,
            "merge lane safe conflict repair evidence",
        )?;
        println!(
            "{}=safe_conflict_repaired issue={} target_state=merging evidence={}",
            output_scope.action_prefix(),
            issue.identifier,
            comment_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Skipped);
    }

    if repair.is_agent_repair_eligible() {
        let agent_repair = run_merge_agent_conflict_repair(
            workflow,
            config,
            issue,
            merge_claim,
            pr_ref,
            head_ref_name.unwrap_or_default(),
            expected_base,
            &repair,
            runner,
        )?;
        let mut agent_decision = decision.clone();
        agent_decision.reason = agent_repair.reason.clone();
        agent_decision.target_state = if agent_repair.repaired || agent_repair.retryable {
            None
        } else {
            Some("need_human_input")
        };
        let workpad = merge_lane_workpad_with_repair_evidence(
            issue,
            &agent_decision,
            Some(&agent_repair.output),
            Some(&agent_repair.evidence),
        );
        record_merge_timeline_comment_with_recovery(
            config,
            adapter,
            issue,
            &agent_decision,
            &workpad,
            if agent_repair.repaired {
                "merge lane merge-agent conflict repair evidence"
            } else {
                "merge lane merge-agent conflict repair failure evidence"
            },
        )?;
        if agent_repair.repaired {
            println!(
                "{}=merge_agent_conflict_repaired issue={} target_state=merging backend={} session={}",
                output_scope.action_prefix(),
                issue.identifier,
                agent_repair.backend,
                agent_repair.session_id.as_deref().unwrap_or("n/a")
            );
            return Ok(MergeOnceOutcome::Skipped);
        }
        if agent_repair.retryable {
            println!(
                "{}=merge_agent_conflict_retryable issue={} target_state=merging backend={} session={}",
                output_scope.action_prefix(),
                issue.identifier,
                agent_repair.backend,
                agent_repair.session_id.as_deref().unwrap_or("n/a")
            );
            return Ok(MergeOnceOutcome::Skipped);
        }

        let state_outcome = set_merge_state_with_recovery(
            config,
            adapter,
            issue,
            "need_human_input",
            agent_decision.pr_url.clone(),
            "merge-agent conflict repair needs human input",
        )?;
        println!(
            "{}=routed issue={} target_state=need_human_input outcome={}",
            output_scope.action_prefix(),
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
    }

    let mut failed_repair = decision.clone();
    failed_repair.target_state = Some("need_human_input");
    failed_repair.reason = repair.reason.clone();
    let evidence = ineligible_merge_agent_repair_evidence(&repair);
    let workpad = merge_lane_workpad_with_repair_evidence(
        issue,
        &failed_repair,
        Some(&repair.output),
        Some(&evidence),
    );
    record_merge_timeline_comment_with_recovery(
        config,
        adapter,
        issue,
        &failed_repair,
        &workpad,
        "merge lane conflict repair failure evidence",
    )?;
    let state_outcome = set_merge_state_with_recovery(
        config,
        adapter,
        issue,
        "need_human_input",
        failed_repair.pr_url.clone(),
        "merge lane conflict repair needs human input",
    )?;
    println!(
        "{}=routed issue={} target_state=need_human_input outcome={}",
        output_scope.action_prefix(),
        issue.identifier,
        state_outcome.as_str()
    );
    Ok(MergeOnceOutcome::Routed)
}
