use std::path::PathBuf;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::git_handoff::ProcessHandoffCommandRunner;
use jade_symphony::handoff::expected_merge_base_branch_for_issue;
use jade_symphony::lane_claim::{LaneClaimActor, LaneClaimSource};
use jade_symphony::merge_lane::{
    expected_merge_base_branch, fetch_pull_request_status_with_recheck, fixture_merge_output,
    merge_lane_decision, merge_lane_workpad, pull_request_status_from_linked,
    update_pull_request_branch, MergeLaneDecision, MergeLaneDecisionKind, PullRequestMergeStatus,
};
use jade_symphony::model::{normalize_state, LinkedPullRequest, TrackerIssue};
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::tracker::adapter_from_config;
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::{
    lane_claim_for_issue, latest_status_for_issue, merge_pull_request_with_recovery,
    pool_claim_eligibility, preflight_canonical_checkout_for_write_mode, print_latest_status,
    progress_spec_with_event_log, project_text_field, single_line, tracker_backend_label,
    worker_identity, WorkerLane,
};

use super::evidence::{
    close_completed_issue, record_done_merge_lane_completion,
    record_merge_timeline_comment_with_recovery, set_merge_state_with_recovery,
};
use super::selection::{merge_recovery_reason, select_merge_worker_issues};

mod dirty;

use dirty::handle_dirty_merge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeOnceOutcome {
    NoMergingIssue,
    DryRun,
    Merged,
    Routed,
    Skipped,
}

pub(crate) fn merge_once_tick(
    workflow_path: PathBuf,
    write: bool,
    recover: bool,
) -> Result<MergeOnceOutcome, Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;
    if config.tracker.fixture_path.is_none() {
        preflight_canonical_checkout_for_write_mode(&config, "merge_loop", write)?;
    }
    let _merge_prompt = workflow.prompt_for_lane(AgentLane::MergeAgent);

    let adapter = adapter_from_config(&config);
    let merging_state = config.tracker.state_map.merging.clone();
    let mut issues = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("merge_queue_scan"),
        || adapter.fetch_issues_by_states(std::slice::from_ref(&merging_state)),
    )?;
    if issues.is_empty() {
        println!("merge_once=stopped reason=no_merging_issue");
        return Ok(MergeOnceOutcome::NoMergingIssue);
    }

    issues.sort_by_key(|issue| issue.priority.unwrap_or(i64::MAX));
    let worker_id = worker_identity(&config, WorkerLane::Merging);
    let Some(selected) = select_merge_worker_issues(&issues, &worker_id, 1, &config, recover)
        .into_iter()
        .next()
    else {
        println!("merge_once=stopped reason=no_unclaimed_merging_issue");
        return Ok(MergeOnceOutcome::NoMergingIssue);
    };
    let latest_issue = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .issue(selected.issue.identifier.clone())
            .backend(tracker_backend_label(&config))
            .next("merge_issue_read"),
        || adapter.get_issue(&selected.issue.identifier),
    )?;
    let (issue, recovery_reason) = match latest_issue {
        Some(issue) => {
            let recovery_reason = recover
                .then(|| merge_recovery_reason(&issue, &worker_id, &config))
                .flatten();
            (issue, recovery_reason)
        }
        None => {
            let recovery_reason = recover.then_some(selected.recovery_reason).flatten();
            (selected.issue.clone(), recovery_reason)
        }
    };
    if let Some(reason) = recovery_reason.as_deref() {
        println!(
            "merge_loop_recovery_candidate issue={} reason={}",
            issue.identifier, reason
        );
    }
    let eligibility = pool_claim_eligibility(&issue, WorkerLane::Merging, &worker_id, &config);
    if !eligibility.is_claimable() && recovery_reason.is_none() {
        println!(
            "merge_once_action=skipped issue={} reason={}",
            issue.identifier,
            eligibility.skip_reason()
        );
        return Ok(MergeOnceOutcome::Skipped);
    }
    let merge_claim = lane_claim_for_issue(
        &issue,
        WorkerLane::Merging.claim_lane(),
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        project_text_field(&issue, WorkerLane::Merging.claim_field()).as_deref(),
    )
    .with_worker(&worker_id);
    crate::write_lane_claim_field(
        &config,
        adapter.as_ref(),
        &issue,
        WorkerLane::Merging,
        &merge_claim,
        write,
    )?;
    let linked_pull_requests = run_with_progress_heartbeat(
        progress_spec_with_event_log(&config, "github_project_read")
            .issue(issue.identifier.clone())
            .backend(tracker_backend_label(&config))
            .next("linked_pr_read"),
        || adapter.list_linked_pull_requests(&issue.identifier),
    )?;
    let runner = ProcessHandoffCommandRunner;
    let default_expected_base = expected_merge_base_branch(&config);
    let expected_base = expected_merge_base_branch_for_issue(&issue, default_expected_base);
    let status = merge_preflight_status(&config, &issue, &linked_pull_requests, &runner)?;
    let decision = merge_lane_decision(
        &issue,
        &merging_state,
        &expected_base,
        &linked_pull_requests,
        status.as_ref(),
    );

    println!(
        "merge_once issue={} decision={:?} target_state={} write={}",
        issue.identifier,
        decision.kind,
        decision.target_state.unwrap_or("none"),
        write
    );
    print_latest_status(&latest_status_for_issue(
        &config,
        &issue,
        "merge",
        if decision.kind.is_merge_ready() {
            "handoff"
        } else if decision.target_state.is_some() {
            "blocked"
        } else {
            "waiting"
        },
        "merge_decision",
        decision.target_state.map(str::to_string),
    ));
    println!("reason={}", decision.reason);
    if let Some(pr_url) = decision.pr_url.as_deref() {
        println!("pull_request={pr_url}");
    }

    if !write {
        print_merge_dry_run_actions(&decision);
        return Ok(MergeOnceOutcome::DryRun);
    }

    if decision.kind == MergeLaneDecisionKind::StaleBranch {
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("stale-branch decision missing pull request URL")?;
        let output = update_pull_request_branch(pr_ref, &runner, &std::env::current_dir()?)?;
        if output.status == 0 {
            let workpad = merge_lane_workpad(&issue, &decision, Some(&output));
            let comment_outcome = record_merge_timeline_comment_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                &decision,
                &workpad,
                "merge lane stale branch update evidence",
            )?;
            println!(
                "merge_once_action=stale_branch_updated issue={} target_state=merging evidence={}",
                issue.identifier,
                comment_outcome.as_str()
            );
            return Ok(MergeOnceOutcome::Skipped);
        }

        let mut failed_update = decision.clone();
        failed_update.kind = MergeLaneDecisionKind::MergeDirty;
        failed_update.target_state = Some("need_human_input");
        failed_update.reason = format!(
            "safe PR branch update failed with status {}: stdout={} stderr={}",
            output.status,
            single_line(&output.stdout),
            single_line(&output.stderr)
        );
        let workpad = merge_lane_workpad(&issue, &failed_update, Some(&output));
        record_merge_timeline_comment_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            &failed_update,
            &workpad,
            "merge lane stale branch update failure evidence",
        )?;
        let state_outcome = set_merge_state_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            "need_human_input",
            failed_update.pr_url.clone(),
            "merge lane stale branch update failed",
        )?;
        println!(
            "merge_once_action=routed issue={} target_state=need_human_input outcome={}",
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
    }

    if decision.kind == MergeLaneDecisionKind::MergeDirty {
        return handle_dirty_merge(
            &workflow,
            &config,
            adapter.as_ref(),
            &issue,
            &merge_claim,
            &decision,
            status.as_ref(),
            &linked_pull_requests,
            &expected_base,
            &runner,
        );
    }

    if decision.kind.is_merge_ready() {
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("merge-ready decision missing pull request URL")?;
        let output = if merge_rehearsal_mode(&config, &issue) {
            fixture_merge_output(pr_ref)
        } else {
            let (output, merge_outcome) =
                merge_pull_request_with_recovery(pr_ref, &runner, &std::env::current_dir()?)?;
            println!(
                "merge_once_action=merge_command issue={} pr={} outcome={}",
                issue.identifier,
                pr_ref,
                merge_outcome.as_str()
            );
            output
        };
        let workpad = merge_lane_workpad(&issue, &decision, Some(&output));
        record_done_merge_lane_completion(&config, adapter.as_ref(), &issue, &workpad)?;
        println!(
            "merge_once_action=merged issue={} target_state=done",
            issue.identifier
        );
        return Ok(MergeOnceOutcome::Merged);
    }

    let workpad = merge_lane_workpad(&issue, &decision, None);
    record_merge_timeline_comment_with_recovery(
        &config,
        adapter.as_ref(),
        &issue,
        &decision,
        &workpad,
        "merge lane routing evidence",
    )?;
    if let Some(target_state) = decision.target_state {
        let state_outcome = set_merge_state_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            target_state,
            decision.pr_url.clone(),
            "merge lane routing",
        )?;
        if decision.kind == MergeLaneDecisionKind::AlreadyMerged
            && normalize_state(target_state) == "done"
        {
            close_completed_issue(&config, adapter.as_ref(), &issue.identifier, Some(&issue))?;
        }
        println!(
            "merge_once_action=routed issue={} target_state={target_state} outcome={}",
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
    } else {
        println!("merge_once_action=skipped issue={}", issue.identifier);
    }

    Ok(MergeOnceOutcome::Skipped)
}

pub(crate) fn merge_preflight_status(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    linked_pull_requests: &[LinkedPullRequest],
    runner: &ProcessHandoffCommandRunner,
) -> Result<Option<PullRequestMergeStatus>, Box<dyn std::error::Error>> {
    if linked_pull_requests.len() != 1 {
        return Ok(None);
    }

    let linked = &linked_pull_requests[0];
    let number_ref = linked.number.map(|number| number.to_string());
    let Some(pr_ref) = linked.url.as_deref().or(number_ref.as_deref()) else {
        return Ok(None);
    };

    if config.tracker.fixture_path.is_some() || issue.tracker_kind == "memory" {
        return Ok(pull_request_status_from_linked(linked));
    }

    match fetch_pull_request_status_with_recheck(pr_ref, runner, &std::env::current_dir()?, 2) {
        Ok(status) => Ok(Some(status)),
        Err(error) => {
            eprintln!("merge_preflight_warning={error}");
            Ok(None)
        }
    }
}

pub(super) fn merge_rehearsal_mode(config: &RuntimeConfig, issue: &TrackerIssue) -> bool {
    config.tracker.fixture_path.is_some() || issue.tracker_kind == "memory"
}

fn print_merge_dry_run_actions(decision: &MergeLaneDecision) {
    match decision.kind {
        MergeLaneDecisionKind::ReadyToMerge => {
            println!("merge_once_dry_run action=merge");
            println!("merge_once_dry_run action=timeline_comment evidence=merge_result");
            println!("merge_once_dry_run action=set_state target_state=done");
            println!("merge_once_dry_run action=close_issue");
        }
        MergeLaneDecisionKind::AlreadyMerged => {
            println!("merge_once_dry_run action=timeline_comment evidence=already_merged");
            println!("merge_once_dry_run action=set_state target_state=done");
            println!("merge_once_dry_run action=close_issue");
        }
        MergeLaneDecisionKind::StaleBranch => {
            println!("merge_once_dry_run action=update_pr_branch");
            println!("merge_once_dry_run action=timeline_comment evidence=stale_branch_update");
            println!("merge_once_dry_run action=keep_state target_state=merging");
        }
        MergeLaneDecisionKind::MergeDirty => {
            println!("merge_once_dry_run action=attempt_safe_conflict_repair");
            println!("merge_once_dry_run fallback=attempt_merge_agent_conflict_repair");
            println!("merge_once_dry_run action=timeline_comment evidence=conflict_repair_result");
            println!("merge_once_dry_run fallback=set_state target_state=need_human_input");
        }
        _ => {
            println!("merge_once_dry_run action=timeline_comment evidence=preflight_blocker");
            if let Some(target_state) = decision.target_state {
                println!("merge_once_dry_run action=set_state target_state={target_state}");
            }
        }
    }
}
