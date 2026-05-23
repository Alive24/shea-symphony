use jade_symphony::config::RuntimeConfig;
use jade_symphony::git_handoff::{
    commit_issue_worktree_changes, ensure_pull_request_ready, prepare_issue_worktree,
    publish_issue_pull_request, ProcessHandoffCommandRunner,
};
use jade_symphony::handoff::{evaluate_agent_review_handoff, render_agent_review_handoff_workpad};
use jade_symphony::lane_claim::{LaneClaimActor, LaneClaimSource, LaneClaimState};
use jade_symphony::model::{normalize_state, LatestStatus, TrackerIssue};
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::ownership::{runtime_ownership_decision, RuntimeOwnershipDecision};
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::review::transition_allowed_for_main_agent;
use jade_symphony::runtime_state::{
    load_runtime_states, mark_runtime_state_updated, record_runtime_retry,
    remove_runtime_state_for_issue, runtime_state_for_issue, upsert_runtime_state,
};
use jade_symphony::tracker::TrackerAdapter;
use jade_symphony::workflow::WorkflowDefinition;

use super::dispatch::RunLoopWorkerOutcome;
use super::{
    append_runtime_supervision_event, apply_live_handoff_pr_link, current_gh_login,
    execute_issue_once_with_workspace_key, handle_run_loop_gate_failure,
    handle_run_loop_handoff_failure, main_session_active_recoverable,
    reconcile_main_handoff_runtime_state, reconcile_pending_main_session, run_handoff_verification,
    run_loop_agent_review_handoff_evidence, run_loop_apply_recovery_handoff,
    run_loop_assignee_ownership_decision, run_loop_assignee_ownership_workpad,
    run_loop_claim_action, run_loop_handoff_plan, run_loop_handoff_workpad,
    run_loop_live_handoff_enabled, run_loop_ownership_workpad, run_loop_runtime_ownership,
    run_loop_runtime_state_for_issue, run_loop_runtime_state_with_result,
    run_loop_runtime_state_with_transition, run_loop_usage_limit_pause_workpad,
    selected_profile_github_login, AssigneeOwnershipDecision, HandoffVerification,
    MainSessionReconciliation, RunLoopClaimAction, RunLoopLiveHandoff, RunLoopOptions,
};
use crate::{
    append_tracker_mutation_audit, current_time_ms, evaluate_issue_for_current_source,
    lane_claim_for_issue, latest_status_for_issue, live_github_tracker, pool_claim_eligibility,
    print_latest_status, progress_spec_with_event_log, project_text_field, recovery_key,
    set_state_with_recovery, stable_recovery_hash, tracker_backend_label,
    upsert_workpad_with_recovery, write_lane_claim_field, write_lane_claim_state,
    TrackerMutationAudit, WorkerLane,
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
        if let Some(worktree) = live_worktree {
            let runner = ProcessHandoffCommandRunner;
            if result.backend == "codex" {
                let commit_message = format!(
                    "Implement {}: {}",
                    latest.identifier,
                    latest.title.replace(['\n', '\r'], " ")
                );
                match commit_issue_worktree_changes(&handoff, &runner, &commit_message) {
                    Ok(commit) => {
                        println!(
                            "run_loop_action=commit issue={} committed={} hash={}",
                            latest.identifier,
                            commit.committed,
                            commit.commit_hash.as_deref().unwrap_or("n/a")
                        );
                    }
                    Err(error) => {
                        result.success = false;
                        result.message = format!("handoff commit failed: {error}");
                    }
                }
            }
            let verification = if result.success {
                run_handoff_verification(&handoff.workspace_path, config)
            } else {
                HandoffVerification {
                    success: false,
                    summary: result.message.clone(),
                }
            };
            println!(
                "run_loop_action=verify issue={} success={} summary={}",
                latest.identifier, verification.success, verification.summary
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                if verification.success {
                    "handoff"
                } else {
                    "failed"
                },
                "verify",
                Some(if verification.success {
                    "publish PR".into()
                } else {
                    "record failure".into()
                }),
            ));
            result.handoff_verification = Some(verification.summary.clone());
            if verification.success {
                match publish_issue_pull_request(&handoff, &runner) {
                    Ok(publication) => {
                        println!(
                            "run_loop_action=pr issue={} url={} created={}",
                            latest.identifier, publication.pr_url, publication.pr_created
                        );
                        print_latest_status(&LatestStatus {
                            lane: "main".into(),
                            category: "handoff".into(),
                            action: "pr_ready".into(),
                            issue_identifier: Some(latest.identifier.clone()),
                            issue_title: Some(latest.title.clone()),
                            actor_label: Some(config.identity.actor_label.clone()),
                            workspace: Some(worktree.workspace_path.display().to_string()),
                            branch: Some(worktree.branch_name.clone()),
                            session_id: result.session_id.clone(),
                            next: Some("link PR".into()),
                        });
                        result.live_handoff = Some(RunLoopLiveHandoff {
                            worktree,
                            publication,
                            verification: verification.summary,
                            project_pr_link_verified: None,
                            pull_request_ready: None,
                        });
                    }
                    Err(error) => {
                        result.success = false;
                        result.message = format!("handoff publication failed: {error}");
                    }
                }
            } else {
                result.success = false;
                result.message = format!("handoff verification failed: {}", verification.summary);
            }
        }
        if result.success {
            let linked = apply_live_handoff_pr_link(adapter, &latest.identifier, &mut result);
            if linked {
                append_tracker_mutation_audit(
                    config,
                    TrackerMutationAudit {
                        command: "main loop",
                        mutation_type: "pr_link",
                        issue_ref: Some(&latest.identifier),
                        target: result
                            .live_handoff
                            .as_ref()
                            .map(|handoff| handoff.publication.pr_url.clone()),
                        from_state: Some(latest.state.clone()),
                        to_state: None,
                        reason: "live handoff PR link",
                    },
                );
                println!(
                    "run_loop_action=link_pr issue={} evidence=live_handoff",
                    latest.identifier
                );
            }
        }
        if result.success {
            if let Some(handoff) = result.live_handoff.as_mut() {
                match ensure_pull_request_ready(
                    &handoff.publication.pr_url,
                    &ProcessHandoffCommandRunner,
                    &handoff.worktree.workspace_path,
                ) {
                    Ok(ready) => {
                        println!(
                            "run_loop_action=pr_ready issue={} url={} was_draft={} marked_ready={}",
                            latest.identifier, ready.pr_url, ready.was_draft, ready.marked_ready
                        );
                        handoff.pull_request_ready = Some(ready);
                    }
                    Err(error) => {
                        result.success = false;
                        result.message = format!("handoff PR ready check failed: {error}");
                        println!(
                        "run_loop_action=blocked issue={} reason=pr_ready_check_failed error={}",
                        latest.identifier, error
                    );
                    }
                }
            }
        }
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

    if result.success {
        if !transition_allowed_for_main_agent("agent_review") {
            return Err("main implementation agent cannot set requested review state".into());
        }
        let evidence =
            run_loop_agent_review_handoff_evidence(&latest, &result, &handoff, Some(&workpad));
        let handoff_report = evaluate_agent_review_handoff(&evidence);
        let handoff_workpad =
            render_agent_review_handoff_workpad(&latest, &evidence, &handoff_report);
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
            Some(&latest),
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
                &latest,
                WorkerLane::Main,
                &main_claim,
                LaneClaimState::Failed,
            )?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
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
                &latest,
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
            &latest,
            WorkerLane::Main,
            &main_claim,
            LaneClaimState::Done,
        )?;
        let state_outcome = set_state_with_recovery(
            adapter,
            &latest.identifier,
            Some(&latest),
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
            &latest,
            "main",
            "handoff",
            "agent_review",
            Some("Review Agent".into()),
        ));
    } else {
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
                run_loop_usage_limit_pause_workpad(&latest, &result, pause, retry_delay_ms);
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
                Some(&latest),
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
                &latest,
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
                &latest,
                WorkerLane::Main,
                &main_claim,
                LaneClaimState::Failed,
            )?;
            let state_outcome = set_state_with_recovery(
                adapter,
                &latest.identifier,
                Some(&latest),
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
                &latest,
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
                &latest,
                "main",
                "retrying",
                "retry_scheduled",
                Some(format!("retry in {retry_delay_ms}ms")),
            ));
            return Ok(RunLoopWorkerOutcome::StopIteration);
        } else {
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
                Some(&latest),
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
                &latest,
                "main",
                "failed",
                "need_human_input",
                Some("operator repair".into()),
            ));
        }
    }

    Ok(RunLoopWorkerOutcome::Completed)
}
