use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::{prepare_issue_worktree, ProcessHandoffCommandRunner};
use shea_symphony::lane_claim::{LaneClaimActor, LaneClaimSource};
use shea_symphony::model::{normalize_state, LatestStatus, TrackerIssue};
use shea_symphony::ownership::{runtime_ownership_decision, RuntimeOwnershipDecision};
use shea_symphony::progress::run_with_progress_heartbeat;
use shea_symphony::runtime_state::{
    load_runtime_states, mark_runtime_state_updated, runtime_state_for_issue, upsert_runtime_state,
};
use shea_symphony::tracker::TrackerAdapter;
use shea_symphony::workflow::WorkflowDefinition;

use super::dispatch::RunLoopWorkerOutcome;
mod live_handoff;
mod terminal;

use live_handoff::apply_live_handoff_steps;
use terminal::{apply_terminal_transition, TerminalTransitionContext};

use super::{
    append_runtime_supervision_event, current_gh_login, execute_issue_once_with_workspace_key,
    handle_run_loop_gate_failure, handle_run_loop_handoff_failure, main_session_active_recoverable,
    reconcile_pending_main_session, run_loop_apply_recovery_handoff,
    run_loop_assignee_ownership_decision, run_loop_assignee_ownership_workpad,
    run_loop_claim_action, run_loop_handoff_plan, run_loop_handoff_workpad,
    run_loop_live_handoff_enabled, run_loop_ownership_workpad, run_loop_runtime_ownership,
    run_loop_runtime_state_for_issue, run_loop_runtime_state_with_result,
    selected_profile_github_login, AssigneeOwnershipDecision, MainSessionReconciliation,
    RunLoopClaimAction, RunLoopOptions,
};
use crate::commands::gate::evaluate_issue_for_current_source;
use crate::lanes::claim::{
    lane_claim_for_issue, pool_claim_eligibility, project_text_field, write_lane_claim_field,
    WorkerLane,
};
use crate::orchestration::{
    append_tracker_mutation_audit, current_time_ms, latest_status_for_issue, live_github_tracker,
    print_latest_status, progress_spec_with_event_log, recovery_key, set_state_with_recovery,
    stable_recovery_hash, tracker_backend_label, upsert_workpad_with_recovery,
    TrackerMutationAudit,
};

pub(crate) fn run_loop_dispatch_write_candidate(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: TrackerIssue,
    recover: bool,
    worker_id: &str,
    options: &RunLoopOptions,
) -> Result<RunLoopWorkerOutcome, Box<dyn std::error::Error>> {
    let latest = if recover && normalize_state(&issue.state) == "in progress" {
        issue.clone()
    } else {
        run_with_progress_heartbeat(
            progress_spec_with_event_log(config, "github_project_read")
                .issue(issue.identifier.clone())
                .backend(tracker_backend_label(config))
                .next("main_issue_read"),
            || adapter.get_issue(&issue.identifier),
        )?
        .ok_or_else(|| format!("issue disappeared before claim: {}", issue.identifier))?
    };
    let eligibility = pool_claim_eligibility(&latest, WorkerLane::Main, worker_id, config);
    if !eligibility.is_claimable() {
        println!(
            "run_loop_action=skip issue={} reason={}",
            latest.identifier,
            eligibility.skip_reason()
        );
        return Ok(RunLoopWorkerOutcome::Completed);
    }
    let latest_gate = evaluate_issue_for_current_source(config, &latest)?;
    if !latest_gate.is_dispatchable() {
        handle_run_loop_gate_failure(adapter, &latest, &latest_gate, options, config)?;
        return Ok(RunLoopWorkerOutcome::Completed);
    }

    let mut handoff = match run_loop_handoff_plan(config, &latest) {
        Ok(handoff) => handoff,
        Err(error) => {
            handle_run_loop_handoff_failure(adapter, &latest, &error, options, config)?;
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    };

    let profile_login = selected_profile_github_login(config)?;
    let active_login = if live_github_tracker(config) && profile_login.is_none() {
        current_gh_login()?
    } else {
        None
    };
    match run_loop_assignee_ownership_decision(
        &latest,
        config,
        active_login.as_deref(),
        profile_login.as_deref(),
    ) {
        AssigneeOwnershipDecision::Allowed => {}
        AssigneeOwnershipDecision::Block { reason } => {
            let workpad = run_loop_assignee_ownership_workpad(&latest, &reason);
            let workpad_key = recovery_key(
                "main-assignee-ownership-workpad",
                &latest.identifier,
                &format!("{}|{}", latest.identifier, stable_recovery_hash(&workpad)),
            );
            upsert_workpad_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                &workpad,
                &workpad_key,
            )?;
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "assignee_ownership",
                Some("operator intervention".into()),
            ));
            println!(
                "run_loop_action=skip issue={} reason=assignee_ownership detail={}",
                latest.identifier, reason
            );
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    }

    let existing_runtime_states = load_runtime_states(config)?;
    let existing_runtime_state =
        runtime_state_for_issue(&existing_runtime_states, &latest.identifier);
    if let Some(state) = existing_runtime_state {
        if let Some(active_issue) = &state.active_issue {
            println!(
                "run_loop_runtime_state action=loaded active_issue={} attempt={}",
                active_issue.identifier, state.attempt_count
            );
        }
        if recover {
            if let Some(evidence) =
                run_loop_apply_recovery_handoff(config, &latest, &mut handoff, state)?
            {
                println!(
                    "run_loop_action=recovery_handoff issue={} evidence={}",
                    latest.identifier, evidence
                );
            }
        }
    }

    let ownership = run_loop_runtime_ownership(&latest, config, &handoff)?;
    let claim_action = run_loop_claim_action(&latest, config);
    let main_claim = lane_claim_for_issue(
        &latest,
        WorkerLane::Main.claim_lane(),
        LaneClaimActor::Codex,
        LaneClaimSource::Loop,
        project_text_field(&latest, WorkerLane::Main.claim_field()).as_deref(),
    )
    .with_worker(worker_id);
    if matches!(claim_action, RunLoopClaimAction::Resume) {
        if let RuntimeOwnershipDecision::Mismatched { reason, .. } =
            runtime_ownership_decision(latest.description.as_deref(), &ownership)
        {
            println!(
                "run_loop_action=skip issue={} reason=ownership_mismatch detail={reason}",
                latest.identifier
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "ownership_mismatch",
                Some("inspect runtime owner".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    }

    let event = match claim_action {
        RunLoopClaimAction::Claim => {
            write_lane_claim_field(
                config,
                adapter,
                &latest,
                WorkerLane::Main,
                &main_claim,
                true,
            )?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
                "in_progress",
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
                        to_state: Some("in_progress".into()),
                        reason: "main worker claim",
                    },
                );
            }
            println!(
                "run_loop_action=claim issue={} target_state=in_progress outcome={}",
                latest.identifier,
                state_outcome.as_str()
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "running",
                "claimed",
                Some("write runtime ownership".into()),
            ));
            "Claimed"
        }
        RunLoopClaimAction::Resume => {
            write_lane_claim_field(
                config,
                adapter,
                &latest,
                WorkerLane::Main,
                &main_claim,
                true,
            )?;
            println!("run_loop_action=resume issue={}", latest.identifier);
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "running",
                "resumed",
                Some("continue backend work".into()),
            ));
            "Resumed"
        }
        RunLoopClaimAction::StopAndReplan { current_state } => {
            println!(
                "run_loop_action=skip issue={} reason=external_state_change current_state={:?}",
                latest.identifier, current_state
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "waiting",
                "external_state_change",
                Some("replan".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    };
    let ownership_workpad = run_loop_ownership_workpad(&latest, &ownership, event, &main_claim);
    let ownership_key = recovery_key(
        "main-ownership-workpad",
        &latest.identifier,
        &format!(
            "{}|{}|{}",
            latest.identifier,
            main_claim.run,
            stable_recovery_hash(&ownership_workpad)
        ),
    );
    let ownership_outcome = upsert_workpad_with_recovery(
        adapter,
        &latest.identifier,
        Some(&latest),
        &ownership_workpad,
        &ownership_key,
    )?;
    if ownership_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "workpad_write",
                issue_ref: Some(&latest.identifier),
                target: ownership.profile_id.clone(),
                from_state: Some(latest.state.clone()),
                to_state: None,
                reason: "runtime ownership evidence",
            },
        );
    }
    println!(
        "run_loop_action=ownership issue={} profile={} branch={} outcome={}",
        latest.identifier,
        ownership.profile_id.as_deref().unwrap_or("n/a"),
        ownership.branch_name,
        ownership_outcome.as_str()
    );

    let mut runtime_state = run_loop_runtime_state_for_issue(
        existing_runtime_state,
        &latest,
        config,
        event,
        &main_claim,
    );
    runtime_state.branch_name = Some(handoff.branch_name.clone());
    mark_runtime_state_updated(&mut runtime_state, current_time_ms());
    upsert_runtime_state(config, &runtime_state)?;
    println!(
        "run_loop_runtime_state action=saved issue={} event={event}",
        latest.identifier
    );

    let live_worktree = if run_loop_live_handoff_enabled(config) {
        let runner = ProcessHandoffCommandRunner;
        let repo_root = std::env::current_dir()?;
        let worktree = prepare_issue_worktree(&repo_root, &handoff, &runner)?;
        println!(
            "run_loop_action=worktree issue={} workspace={} branch={} created={}",
            latest.identifier,
            worktree.workspace_path.display(),
            worktree.branch_name,
            worktree.created
        );
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: "running".into(),
            action: "worktree_ready".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: Some(worktree.workspace_path.display().to_string()),
            branch: Some(worktree.branch_name.clone()),
            session_id: runtime_state.backend_session_id.clone(),
            next: Some("run backend".into()),
        });
        Some(worktree)
    } else {
        None
    };

    let mut session_reconciliation =
        reconcile_pending_main_session(config, &latest, &handoff, &runtime_state)?;
    if let Some(MainSessionReconciliation::Active {
        status,
        source,
        evidence,
    }) = &session_reconciliation
    {
        let recover_active = recover && main_session_active_recoverable(status, evidence);
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            if recover_active {
                "MainSessionRecovering"
            } else {
                "MainSessionStillActive"
            },
            &format!(
                "issue={} status={} source={} evidence={}",
                latest.identifier, status, source, evidence
            ),
        )?;
        println!(
            "run_loop_action=session_observed issue={} status={} source={} evidence={:?}",
            latest.identifier, status, source, evidence
        );
        if recover_active {
            println!(
                "run_loop_action=recover issue={} status={} source={} evidence={:?}",
                latest.identifier, status, source, evidence
            );
            session_reconciliation = None;
        } else {
            print_latest_status(&LatestStatus {
                lane: "main".into(),
                category: status.clone(),
                action: "session_observed".into(),
                issue_identifier: Some(latest.identifier.clone()),
                issue_title: Some(latest.title.clone()),
                actor_label: Some(config.identity.actor_label.clone()),
                workspace: runtime_state
                    .workspace_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
                branch: runtime_state.branch_name.clone(),
                session_id: runtime_state.backend_session_id.clone(),
                next: runtime_state.backend_attach_command.clone(),
            });
            return Ok(RunLoopWorkerOutcome::StopIteration);
        }
    }
    if let Some(MainSessionReconciliation::Active {
        status,
        source: _,
        evidence: _,
    }) = &session_reconciliation
    {
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: status.clone(),
            action: "session_observed".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: runtime_state
                .workspace_path
                .as_ref()
                .map(|path| path.display().to_string()),
            branch: runtime_state.branch_name.clone(),
            session_id: runtime_state.backend_session_id.clone(),
            next: runtime_state.backend_attach_command.clone(),
        });
        return Ok(RunLoopWorkerOutcome::StopIteration);
    }

    print_latest_status(&latest_status_for_issue(
        config,
        &latest,
        "main",
        if session_reconciliation.is_some() {
            "reconciling"
        } else {
            "running"
        },
        if session_reconciliation.is_some() {
            "session_terminal"
        } else {
            "backend"
        },
        Some("save result".into()),
    ));
    let mut result = match session_reconciliation {
        Some(MainSessionReconciliation::Terminal(result)) => *result,
        Some(MainSessionReconciliation::Active { .. }) => unreachable!(),
        None => execute_issue_once_with_workspace_key(
            workflow,
            config,
            &latest,
            &handoff.workspace_key,
            runtime_state.attempt_count,
            Some(&main_claim),
        )?,
    };
    if result.success {
        apply_live_handoff_steps(
            config,
            adapter,
            &latest,
            &handoff,
            live_worktree,
            &mut result,
        )?;
    }
    runtime_state = run_loop_runtime_state_with_result(runtime_state, &result);
    mark_runtime_state_updated(&mut runtime_state, current_time_ms());
    upsert_runtime_state(config, &runtime_state)?;
    println!(
        "run_loop_runtime_state action=updated issue={} event={}",
        latest.identifier,
        runtime_state.last_event.as_deref().unwrap_or("unknown")
    );

    let workpad = run_loop_handoff_workpad(&latest, &result, &handoff, Some(&ownership));
    let handoff_key = recovery_key(
        "main-handoff-workpad",
        &latest.identifier,
        &format!(
            "{}|{}|{}",
            latest.identifier,
            main_claim.run,
            stable_recovery_hash(&workpad)
        ),
    );
    let handoff_outcome = upsert_workpad_with_recovery(
        adapter,
        &latest.identifier,
        Some(&latest),
        &workpad,
        &handoff_key,
    )?;
    if handoff_outcome.should_record_audit() {
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
                to_state: None,
                reason: "main worker handoff evidence",
            },
        );
    }

    if result.pending_session {
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            "TmuxSessionRunning",
            &format!(
                "issue={} session={} attach_command={} log_path={}",
                latest.identifier,
                result.session_id.as_deref().unwrap_or("n/a"),
                result.backend_attach_command.as_deref().unwrap_or("n/a"),
                result
                    .backend_log_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "n/a".into())
            ),
        )?;
        println!(
        "run_loop_action=session_started issue={} backend={} session={} attach_command=\"{}\" log_path={}",
        latest.identifier,
        result.backend,
        result.session_id.as_deref().unwrap_or("n/a"),
        result
            .backend_attach_command
            .as_deref()
            .unwrap_or("n/a"),
        result
            .backend_log_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "n/a".into())
    );
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: "running".into(),
            action: "session_started".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: Some(result.workspace_path.display().to_string()),
            branch: runtime_state.branch_name.clone(),
            session_id: result.session_id.clone(),
            next: result.backend_attach_command.clone(),
        });
        return Ok(RunLoopWorkerOutcome::Completed);
    }

    apply_terminal_transition(
        TerminalTransitionContext {
            config,
            adapter,
            latest: &latest,
            main_claim: &main_claim,
            handoff: &handoff,
            workpad: &workpad,
        },
        runtime_state,
        &result,
    )
}
