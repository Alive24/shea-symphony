use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::Duration;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::git_handoff::ProcessHandoffCommandRunner;
use jade_symphony::handoff::expected_merge_base_branch_for_issue;
use jade_symphony::lane_claim::{
    LaneClaim, LaneClaimActor, LaneClaimLane, LaneClaimSource, LaneClaimState,
};
use jade_symphony::merge_lane::{
    expected_merge_base_branch, fetch_pull_request_status_with_recheck, fixture_merge_output,
    merge_lane_decision, merge_lane_workpad, merge_lane_workpad_with_repair_evidence,
    pull_request_status_from_linked, repair_dirty_pull_request, update_pull_request_branch,
    MergeLaneDecisionKind,
};
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::tracker::{adapter_from_config, TrackerAdapter};
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::{
    add_timeline_comment_with_recovery, append_tracker_mutation_audit, close_issue_with_recovery,
    enforce_canonical_checkout_before_write, lane_claim_for_issue, latest_status_for_issue,
    merge_completion_recovery_key, merge_decision_recovery_key, merge_pull_request_with_recovery,
    pool_claim_eligibility, preflight_canonical_checkout_for_write_mode, print_latest_status,
    progress_spec_with_event_log, project_text_field, select_pool_worker_issues,
    set_state_with_recovery, single_line, tracker_backend_label, worker_identity,
    TrackerMutationAudit, TrackerMutationOutcome, WorkerLane,
};

mod repair;

#[cfg(test)]
pub(crate) use repair::{
    finish_merge_agent_repaired_branch, merge_agent_reports_repaired,
    merge_agent_requests_human_input,
};
use repair::{
    ineligible_merge_agent_repair_evidence, mechanical_merge_repair_evidence,
    run_merge_agent_conflict_repair,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeLoopOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) once: bool,
    pub(crate) write: bool,
    pub(crate) recover: bool,
    pub(crate) max_concurrent: Option<usize>,
}

impl MergeLoopOptions {
    pub(crate) fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    pub(crate) fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.merge_lane.max_concurrent_workers)
            .max(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeOnceOutcome {
    NoMergingIssue,
    DryRun,
    Merged,
    Routed,
    Skipped,
}

pub(crate) fn merge_once(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    merge_once_tick(workflow_path, write, false).map(|_| ())
}

pub(crate) fn merge_loop(options: MergeLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    let max_concurrent = options.worker_limit(&config);
    let mut stopped = false;
    let mut iteration = 0usize;

    loop {
        if let Some(max) = limit {
            if iteration >= max {
                println!("merge_loop=stopped reason=max_iterations iterations={iteration}");
                break;
            }
        }

        iteration += 1;
        let mut should_sleep = false;
        println!(
            "merge_loop_iteration={} mode={} recover={} max_concurrent={max_concurrent}",
            iteration,
            if options.write { "write" } else { "dry-run" },
            options.recover
        );
        for slot in 1..=max_concurrent {
            match merge_once_tick(
                options.workflow_path.clone(),
                options.write,
                options.recover,
            )? {
                MergeOnceOutcome::NoMergingIssue => {
                    if limit.is_none() {
                        should_sleep = true;
                        println!(
                            "merge_loop_idle action=sleep reason=no_merging_issue delay_ms={} iterations={iteration} slot={slot}",
                            config.polling.interval_ms
                        );
                    } else {
                        println!(
                            "merge_loop=stopped reason=no_merging_issue iterations={iteration} slot={slot}"
                        );
                        stopped = true;
                    }
                    break;
                }
                MergeOnceOutcome::DryRun if !options.write => {
                    println!("merge_loop_action=dry_run_tick iterations={iteration} slot={slot}");
                    if limit.is_none() {
                        should_sleep = true;
                        break;
                    } else if max_concurrent > 1 {
                        println!(
                            "merge_loop=stopped reason=dry_run_would_repeat_without_mutation iterations={iteration}"
                        );
                        stopped = true;
                        break;
                    }
                }
                MergeOnceOutcome::Merged => {
                    println!("merge_loop_action=merged iterations={iteration} slot={slot}");
                    if options.write && config.tracker.fixture_path.is_none() {
                        refresh_canonical_checkout_after_merge(&config)?;
                    }
                }
                MergeOnceOutcome::Routed => {
                    println!("merge_loop_action=routed iterations={iteration} slot={slot}");
                }
                MergeOnceOutcome::Skipped => {
                    println!("merge_loop_action=skipped iterations={iteration} slot={slot}");
                }
                MergeOnceOutcome::DryRun => {}
            }
        }
        if stopped {
            break;
        }
        if should_sleep {
            thread::sleep(Duration::from_millis(config.polling.interval_ms));
        }
    }

    Ok(())
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
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("dirty-merge decision missing pull request URL")?;
        let head_ref_name = status
            .as_ref()
            .and_then(|status| status.head_ref_name.as_deref())
            .or_else(|| {
                linked_pull_requests
                    .first()
                    .and_then(|pull_request| pull_request.head_ref_name.as_deref())
            });
        let repair = repair_dirty_pull_request(
            pr_ref,
            head_ref_name,
            &expected_base,
            &runner,
            &std::env::current_dir()?,
            merge_rehearsal_mode(&config, &issue),
        )?;
        if repair.repaired {
            let mut repaired_decision = decision.clone();
            repaired_decision.reason = repair.reason.clone();
            let evidence = mechanical_merge_repair_evidence(&repair, &expected_base);
            let workpad = merge_lane_workpad_with_repair_evidence(
                &issue,
                &repaired_decision,
                Some(&repair.output),
                Some(&evidence),
            );
            let comment_outcome = record_merge_timeline_comment_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                &repaired_decision,
                &workpad,
                "merge lane safe conflict repair evidence",
            )?;
            println!(
                "merge_once_action=safe_conflict_repaired issue={} target_state=merging evidence={}",
                issue.identifier,
                comment_outcome.as_str()
            );
            return Ok(MergeOnceOutcome::Skipped);
        }

        if repair.is_agent_repair_eligible() {
            let agent_repair = run_merge_agent_conflict_repair(
                &workflow,
                &config,
                &issue,
                &merge_claim,
                pr_ref,
                head_ref_name.unwrap_or_default(),
                &expected_base,
                &repair,
                &runner,
            )?;
            let mut agent_decision = decision.clone();
            agent_decision.reason = agent_repair.reason.clone();
            agent_decision.target_state = if agent_repair.repaired {
                None
            } else {
                Some("need_human_input")
            };
            let workpad = merge_lane_workpad_with_repair_evidence(
                &issue,
                &agent_decision,
                Some(&agent_repair.output),
                Some(&agent_repair.evidence),
            );
            record_merge_timeline_comment_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
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
                    "merge_once_action=merge_agent_conflict_repaired issue={} target_state=merging backend={} session={}",
                    issue.identifier,
                    agent_repair.backend,
                    agent_repair.session_id.as_deref().unwrap_or("n/a")
                );
                return Ok(MergeOnceOutcome::Skipped);
            }

            let state_outcome = set_merge_state_with_recovery(
                &config,
                adapter.as_ref(),
                &issue,
                "need_human_input",
                agent_decision.pr_url.clone(),
                "merge-agent conflict repair needs human input",
            )?;
            println!(
                "merge_once_action=routed issue={} target_state=need_human_input outcome={}",
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
            &issue,
            &failed_repair,
            Some(&repair.output),
            Some(&evidence),
        );
        record_merge_timeline_comment_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            &failed_repair,
            &workpad,
            "merge lane conflict repair failure evidence",
        )?;
        let state_outcome = set_merge_state_with_recovery(
            &config,
            adapter.as_ref(),
            &issue,
            "need_human_input",
            failed_repair.pr_url.clone(),
            "merge lane conflict repair needs human input",
        )?;
        println!(
            "merge_once_action=routed issue={} target_state=need_human_input outcome={}",
            issue.identifier,
            state_outcome.as_str()
        );
        return Ok(MergeOnceOutcome::Routed);
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MergeWorkerSelection {
    pub(crate) issue: TrackerIssue,
    pub(crate) recovery_reason: Option<String>,
}

pub(crate) fn select_merge_worker_issues(
    issues: &[TrackerIssue],
    worker_id: &str,
    pool: usize,
    config: &RuntimeConfig,
    recover: bool,
) -> Vec<MergeWorkerSelection> {
    let limit = pool.max(1);
    let mut selected = Vec::new();

    if recover {
        let mut recovery_candidates = issues
            .iter()
            .filter_map(|issue| {
                merge_recovery_reason(issue, worker_id, config).map(|reason| MergeWorkerSelection {
                    issue: issue.clone(),
                    recovery_reason: Some(reason),
                })
            })
            .collect::<Vec<_>>();
        recovery_candidates.sort_by_key(|candidate| candidate.issue.priority.unwrap_or(i64::MAX));
        for candidate in recovery_candidates {
            if selected.len() >= limit {
                break;
            }
            selected.push(candidate);
        }
    }

    let remaining = limit.saturating_sub(selected.len());
    if remaining > 0 {
        for issue in
            select_pool_worker_issues(issues, WorkerLane::Merging, worker_id, remaining, config)
        {
            if selected.iter().any(|candidate: &MergeWorkerSelection| {
                candidate.issue.identifier == issue.identifier
            }) {
                continue;
            }
            selected.push(MergeWorkerSelection {
                issue,
                recovery_reason: None,
            });
        }
    }

    selected
}

fn merge_recovery_reason(
    issue: &TrackerIssue,
    worker_id: &str,
    config: &RuntimeConfig,
) -> Option<String> {
    let normalized_state = issue.normalized_state();
    if normalized_state != normalize_state(&config.tracker.state_map.merging) {
        return None;
    }

    let owner = project_text_field(issue, WorkerLane::Merging.claim_field())?;
    let claim = LaneClaim::parse(&owner).ok()?;
    if claim.lane != LaneClaimLane::Merge
        || claim.issue != issue.identifier
        || claim.state != LaneClaimState::Active
        || !matches!(claim.source, LaneClaimSource::Loop | LaneClaimSource::Goal)
        || claim.worker.as_deref() == Some(worker_id)
    {
        return None;
    }

    Some(format!(
        "recover_active_merge_claim previous_worker={} run={} source={}",
        claim.worker.as_deref().unwrap_or("unknown"),
        claim.run,
        claim.source.as_str()
    ))
}

fn refresh_canonical_checkout_after_merge(
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    println!("merge_loop_action=refresh_canonical_checkout reason=post_merge");
    let output = ProcessCommand::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(&root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "merge loop failed to refresh canonical checkout after merge: status={} stdout={} stderr={}",
            output.status.code().unwrap_or(-1),
            single_line(&String::from_utf8_lossy(&output.stdout)),
            single_line(&String::from_utf8_lossy(&output.stderr))
        )
        .into());
    }
    println!(
        "merge_loop_action=refreshed_canonical_checkout stdout=\"{}\"",
        single_line(&String::from_utf8_lossy(&output.stdout))
    );
    enforce_canonical_checkout_before_write(config, "merge_loop")?;
    Ok(())
}

fn record_merge_timeline_comment_with_recovery(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    decision: &jade_symphony::merge_lane::MergeLaneDecision,
    workpad: &str,
    reason: &'static str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    let key = if matches!(
        decision.kind,
        MergeLaneDecisionKind::ReadyToMerge | MergeLaneDecisionKind::AlreadyMerged
    ) {
        merge_completion_recovery_key(issue, decision.pr_url.as_deref().unwrap_or("missing-pr"))
    } else {
        merge_decision_recovery_key(issue, decision)
    };
    let outcome = add_timeline_comment_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        workpad,
        &key,
        "timeline_comment",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "timeline_comment",
                issue_ref: Some(&issue.identifier),
                target: decision.pr_url.clone(),
                from_state: Some(issue.state.clone()),
                to_state: decision.target_state.map(ToOwned::to_owned),
                reason,
            },
        );
    }
    Ok(outcome)
}

fn set_merge_state_with_recovery(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    target_state: &str,
    pr_url: Option<String>,
    reason: &'static str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    let outcome = set_state_with_recovery(
        adapter,
        &issue.identifier,
        Some(issue),
        target_state,
        "state_change",
    )?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "state_change",
                issue_ref: Some(&issue.identifier),
                target: pr_url,
                from_state: Some(issue.state.clone()),
                to_state: Some(target_state.into()),
                reason,
            },
        );
    }
    Ok(outcome)
}

pub(crate) fn record_done_merge_lane_completion(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    workpad: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let pr_url = issue
        .linked_pull_requests
        .first()
        .and_then(|pr| pr.url.clone());
    let completion_decision = jade_symphony::merge_lane::MergeLaneDecision {
        kind: MergeLaneDecisionKind::ReadyToMerge,
        issue_ref: issue.identifier.clone(),
        pr_url: pr_url.clone(),
        target_state: Some("done"),
        reason: "merge completed".into(),
    };
    record_merge_timeline_comment_with_recovery(
        config,
        adapter,
        issue,
        &completion_decision,
        workpad,
        "merge completion evidence",
    )?;
    set_merge_state_with_recovery(config, adapter, issue, "done", pr_url, "merge completed")?;
    close_completed_issue(config, adapter, &issue.identifier, Some(issue))?;
    Ok(())
}

fn close_completed_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
) -> Result<(), Box<dyn std::error::Error>> {
    let outcome = close_issue_with_recovery(adapter, issue_ref, initial_issue)?;
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "merge once",
                mutation_type: "issue_close",
                issue_ref: Some(issue_ref),
                target: None,
                from_state: initial_issue.map(|issue| issue.state.clone()),
                to_state: Some("closed".into()),
                reason: "merge completed issue closure",
            },
        );
    }
    println!(
        "merge_once_action=closed_issue issue={} outcome={}",
        issue_ref,
        outcome.as_str()
    );
    Ok(())
}

pub(crate) fn merge_preflight_status(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    linked_pull_requests: &[jade_symphony::model::LinkedPullRequest],
    runner: &ProcessHandoffCommandRunner,
) -> Result<Option<jade_symphony::merge_lane::PullRequestMergeStatus>, Box<dyn std::error::Error>> {
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

fn merge_rehearsal_mode(config: &RuntimeConfig, issue: &TrackerIssue) -> bool {
    config.tracker.fixture_path.is_some() || issue.tracker_kind == "memory"
}

fn print_merge_dry_run_actions(decision: &jade_symphony::merge_lane::MergeLaneDecision) {
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
