use jade_symphony::config::RuntimeConfig;
use jade_symphony::handoff::IssueHandoffPlan;
use jade_symphony::handoff::{evaluate_agent_review_handoff, render_agent_review_handoff_workpad};
use jade_symphony::lane_claim::{LaneClaim, LaneClaimState};
use jade_symphony::model::TrackerIssue;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::review::transition_allowed_for_main_agent;
use jade_symphony::runtime_state::{
    mark_runtime_state_updated, record_runtime_retry, remove_runtime_state_for_issue,
    upsert_runtime_state, RuntimeState,
};
use jade_symphony::tracker::TrackerAdapter;

use super::super::dispatch::RunLoopWorkerOutcome;
use super::super::{
    append_runtime_supervision_event, reconcile_main_handoff_runtime_state,
    run_loop_agent_review_handoff_evidence, run_loop_runtime_state_with_transition,
    run_loop_usage_limit_pause_workpad, IssueExecutionResult,
};
use crate::{
    append_tracker_mutation_audit, current_time_ms, latest_status_for_issue, print_latest_status,
    recovery_key, set_state_with_recovery, stable_recovery_hash, upsert_workpad_with_recovery,
    write_lane_claim_state, TrackerMutationAudit, WorkerLane,
};

pub(super) fn apply_terminal_transition(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    latest: &TrackerIssue,
    main_claim: &LaneClaim,
    handoff: &IssueHandoffPlan,
    workpad: &str,
    runtime_state: RuntimeState,
    result: &IssueExecutionResult,
) -> Result<RunLoopWorkerOutcome, Box<dyn std::error::Error>> {
    if result.success {
        return complete_successful_run(
            config,
            adapter,
            latest,
            main_claim,
            handoff,
            workpad,
            runtime_state,
            result,
        );
    }

    complete_failed_run(config, adapter, latest, main_claim, runtime_state, result)
}

fn complete_successful_run(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    latest: &TrackerIssue,
    main_claim: &LaneClaim,
    handoff: &IssueHandoffPlan,
    workpad: &str,
    mut runtime_state: RuntimeState,
    result: &IssueExecutionResult,
) -> Result<RunLoopWorkerOutcome, Box<dyn std::error::Error>> {
    if !transition_allowed_for_main_agent("agent_review") {
        return Err("main implementation agent cannot set requested review state".into());
    }
    let evidence = run_loop_agent_review_handoff_evidence(latest, result, handoff, Some(workpad));
    let handoff_report = evaluate_agent_review_handoff(&evidence);
    let handoff_workpad = render_agent_review_handoff_workpad(latest, &evidence, &handoff_report);
    let review_handoff_key = recovery_key(
        "agent-review-handoff-workpad",
        &latest.identifier,
        &format!(
            "{}|{}|{}",
            latest.identifier,
            main_claim.run,
            stable_recovery_hash(&handoff_workpad)
        ),
    );
    let review_handoff_outcome = upsert_workpad_with_recovery(
        adapter,
        &latest.identifier,
        Some(latest),
        &handoff_workpad,
        &review_handoff_key,
    )?;
    if review_handoff_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "workpad_write",
                issue_ref: Some(&latest.identifier),
                target: result
                    .live_handoff
                    .as_ref()
                    .map(|handoff| handoff.publication.pr_url.clone()),
                from_state: Some(latest.state.clone()),
                to_state: Some("agent_review".into()),
                reason: "agent review handoff evidence",
            },
        );
    }
    if !handoff_report.is_ready() {
        runtime_state = run_loop_runtime_state_with_transition(
            runtime_state,
            Some(latest.state.clone()),
            "need_human_input",
            "agent review handoff invariant failed",
        );
        upsert_runtime_state(config, &runtime_state)?;
        write_lane_claim_state(
            config,
            adapter,
            latest,
            WorkerLane::Main,
            main_claim,
            LaneClaimState::Failed,
        )?;
        let state_outcome = set_state_with_recovery(
            adapter,
            &latest.identifier,
            Some(latest),
            "need_human_input",
            "state_change",
        )?;
        if state_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "main loop",
                    mutation_type: "state_change",
                    issue_ref: Some(&latest.identifier),
                    target: None,
                    from_state: Some(latest.state.clone()),
                    to_state: Some("need_human_input".into()),
                    reason: "agent review handoff invariant failed",
                },
            );
        }
        remove_runtime_state_for_issue(config, &latest.identifier)?;
        println!(
            "run_loop_action=blocked issue={} target_state=need_human_input reason=handoff_invariant_failed",
            latest.identifier
        );
        print_latest_status(&latest_status_for_issue(
            config,
            latest,
            "main",
            "blocked",
            "handoff_invariant_failed",
            Some("Need Human Input".into()),
        ));
        return Ok(RunLoopWorkerOutcome::Completed);
    }
    runtime_state = run_loop_runtime_state_with_transition(
        runtime_state,
        Some(latest.state.clone()),
        "agent_review",
        "main agent completed",
    );
    mark_runtime_state_updated(&mut runtime_state, current_time_ms());
    upsert_runtime_state(config, &runtime_state)?;
    write_lane_claim_state(
        config,
        adapter,
        latest,
        WorkerLane::Main,
        main_claim,
        LaneClaimState::Done,
    )?;
    let state_outcome = set_state_with_recovery(
        adapter,
        &latest.identifier,
        Some(latest),
        "agent_review",
        "state_change",
    )?;
    reconcile_main_handoff_runtime_state(config, &latest.identifier, "agent_review")?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "state_change",
                issue_ref: Some(&latest.identifier),
                target: result
                    .live_handoff
                    .as_ref()
                    .map(|handoff| handoff.publication.pr_url.clone()),
                from_state: Some(latest.state.clone()),
                to_state: Some("agent_review".into()),
                reason: "main agent completed",
            },
        );
    }
    remove_runtime_state_for_issue(config, &latest.identifier)?;
    println!(
        "run_loop_action=handoff issue={} target_state=agent_review",
        latest.identifier
    );
    print_latest_status(&latest_status_for_issue(
        config,
        latest,
        "main",
        "handoff",
        "agent_review",
        Some("Review Agent".into()),
    ));
    Ok(RunLoopWorkerOutcome::Completed)
}

fn complete_failed_run(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    latest: &TrackerIssue,
    main_claim: &LaneClaim,
    mut runtime_state: RuntimeState,
    result: &IssueExecutionResult,
) -> Result<RunLoopWorkerOutcome, Box<dyn std::error::Error>> {
    let retry_delay_ms =
        Orchestrator::new(config.clone()).retry_delay_ms(runtime_state.attempt_count, false);
    if let Some(pause) = &result.usage_limit_pause {
        record_runtime_retry(
            &mut runtime_state,
            current_time_ms(),
            retry_delay_ms,
            format!("usage-limit pause: {}", pause.evidence),
        );
        upsert_runtime_state(config, &runtime_state)?;
        let pause_workpad =
            run_loop_usage_limit_pause_workpad(latest, result, pause, retry_delay_ms);
        let pause_key = recovery_key(
            "main-usage-limit-workpad",
            &latest.identifier,
            &format!(
                "{}|{}|{}",
                latest.identifier,
                main_claim.run,
                stable_recovery_hash(&pause_workpad)
            ),
        );
        let pause_outcome = upsert_workpad_with_recovery(
            adapter,
            &latest.identifier,
            Some(latest),
            &pause_workpad,
            &pause_key,
        )?;
        if pause_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "main loop",
                    mutation_type: "workpad_write",
                    issue_ref: Some(&latest.identifier),
                    target: Some(pause.classifier.clone()),
                    from_state: Some(latest.state.clone()),
                    to_state: None,
                    reason: "usage-limit pause evidence",
                },
            );
        }
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            "UsageLimitPaused",
            &format!(
                "issue={} classifier={} due_in_ms={} evidence={}",
                latest.identifier, pause.classifier, retry_delay_ms, pause.evidence
            ),
        )?;
        println!(
            "run_loop_action=usage_limit_paused issue={} classifier={} due_in_ms={}",
            latest.identifier, pause.classifier, retry_delay_ms
        );
        print_latest_status(&latest_status_for_issue(
            config,
            latest,
            "main",
            "retrying",
            "usage_limit_paused",
            Some(format!("retry in {retry_delay_ms}ms")),
        ));
        return Ok(RunLoopWorkerOutcome::StopIteration);
    }
    if result.message.contains("handoff PR link") {
        runtime_state = run_loop_runtime_state_with_transition(
            runtime_state,
            Some(latest.state.clone()),
            "need_human_input",
            "handoff PR linkage invariant failed",
        );
        mark_runtime_state_updated(&mut runtime_state, current_time_ms());
        upsert_runtime_state(config, &runtime_state)?;
        write_lane_claim_state(
            config,
            adapter,
            latest,
            WorkerLane::Main,
            main_claim,
            LaneClaimState::Failed,
        )?;
        let state_outcome = set_state_with_recovery(
            adapter,
            &latest.identifier,
            Some(latest),
            "need_human_input",
            "state_change",
        )?;
        if state_outcome.should_record_audit() {
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "main loop",
                    mutation_type: "state_change",
                    issue_ref: Some(&latest.identifier),
                    target: result
                        .live_handoff
                        .as_ref()
                        .map(|handoff| handoff.publication.pr_url.clone()),
                    from_state: Some(latest.state.clone()),
                    to_state: Some("need_human_input".into()),
                    reason: "handoff PR linkage invariant failed",
                },
            );
        }
        remove_runtime_state_for_issue(config, &latest.identifier)?;
        println!(
            "run_loop_action=blocked issue={} target_state=need_human_input reason=handoff_pr_linkage_invariant_failed",
            latest.identifier
        );
        print_latest_status(&latest_status_for_issue(
            config,
            latest,
            "main",
            "blocked",
            "handoff_pr_linkage",
            Some("Need Human Input".into()),
        ));
        return Ok(RunLoopWorkerOutcome::Completed);
    }
    if runtime_state.attempt_count < config.agent.max_turns {
        record_runtime_retry(
            &mut runtime_state,
            current_time_ms(),
            retry_delay_ms,
            result.message.clone(),
        );
        upsert_runtime_state(config, &runtime_state)?;
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            "RetryScheduled",
            &format!(
                "issue={} attempt={} due_in_ms={} error={}",
                latest.identifier, runtime_state.attempt_count, retry_delay_ms, result.message
            ),
        )?;
        println!(
            "run_loop_action=retry_scheduled issue={} attempt={} due_in_ms={}",
            latest.identifier, runtime_state.attempt_count, retry_delay_ms
        );
        print_latest_status(&latest_status_for_issue(
            config,
            latest,
            "main",
            "retrying",
            "retry_scheduled",
            Some(format!("retry in {retry_delay_ms}ms")),
        ));
        return Ok(RunLoopWorkerOutcome::StopIteration);
    }

    runtime_state = run_loop_runtime_state_with_transition(
        runtime_state,
        Some(latest.state.clone()),
        "need_human_input",
        "backend run failed after retry limit",
    );
    mark_runtime_state_updated(&mut runtime_state, current_time_ms());
    upsert_runtime_state(config, &runtime_state)?;
    let state_outcome = set_state_with_recovery(
        adapter,
        &latest.identifier,
        Some(latest),
        "need_human_input",
        "state_change",
    )?;
    if state_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "state_change",
                issue_ref: Some(&latest.identifier),
                target: None,
                from_state: Some(latest.state.clone()),
                to_state: Some("need_human_input".into()),
                reason: "backend run failed after retry limit",
            },
        );
    }
    remove_runtime_state_for_issue(config, &latest.identifier)?;
    println!(
        "run_loop_action=blocked issue={} target_state=need_human_input",
        latest.identifier
    );
    print_latest_status(&latest_status_for_issue(
        config,
        latest,
        "main",
        "failed",
        "need_human_input",
        Some("operator repair".into()),
    ));
    Ok(RunLoopWorkerOutcome::Completed)
}
