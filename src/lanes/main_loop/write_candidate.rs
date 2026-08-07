use std::fs;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::git_handoff::{prepare_issue_worktree, ProcessHandoffCommandRunner};
use shea_symphony::handoff::{BranchTargetRole, IssueHandoffPlan};
use shea_symphony::lane_claim::{LaneClaimActor, LaneClaimSource};
use shea_symphony::model::{normalize_state, LatestStatus, TrackerIssue};
use shea_symphony::ownership::{runtime_ownership_decision, RuntimeOwnershipDecision};
use shea_symphony::progress::run_with_progress_heartbeat;
use shea_symphony::runtime_profile::{
    persist_runtime_readiness_failure, resolve_runtime_readiness,
};
use shea_symphony::runtime_state::{
    load_runtime_states, mark_runtime_state_updated, runtime_state_for_issue, upsert_runtime_state,
};
use shea_symphony::tracker::TrackerAdapter;
use shea_symphony::workflow::WorkflowDefinition;
use shea_symphony::workpad_templates::{render_workpad_template, WorkpadTemplateId};

use super::dispatch::RunLoopWorkerOutcome;
mod live_handoff;
mod terminal;

use live_handoff::apply_live_handoff_steps;
use terminal::{apply_terminal_transition, TerminalTransitionContext};

use super::{
    append_runtime_supervision_event, current_gh_login, execute_issue_once_with_options,
    handle_run_loop_gate_failure, handle_run_loop_handoff_failure, main_recovery_plan,
    main_recovery_plan_applicable, main_session_active_recoverable, reconcile_pending_main_session,
    run_loop_apply_recovery_handoff, run_loop_assignee_ownership_decision,
    run_loop_assignee_ownership_workpad, run_loop_claim_action, run_loop_handoff_plan,
    run_loop_handoff_workpad, run_loop_live_handoff_enabled, run_loop_ownership_workpad,
    run_loop_preflight_launch_workspace, run_loop_recovery_preflight_launch_workspace,
    run_loop_runtime_ownership, run_loop_runtime_state_for_issue,
    run_loop_runtime_state_with_result, selected_profile_github_login, AssigneeOwnershipDecision,
    IssueExecutionOptions, IssueExecutionResult, MainSessionReconciliation, RunLoopClaimAction,
    RunLoopOptions,
};
use crate::commands::gate::evaluate_issue_for_current_source;
use crate::lanes::claim::{
    lane_claim_for_issue, pool_claim_eligibility, project_text_field, write_lane_claim_field,
    WorkerLane,
};
use crate::lanes::main_loop::compact_evidence;
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
    let mut latest = if recover && normalize_state(&issue.state) == "in progress" {
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
    ensure_parent_integration_branch_evidence(config, adapter, &latest, &handoff)?;

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
    let mut recovery_handoff_applied = false;
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
                recovery_handoff_applied = true;
                println!(
                    "run_loop_action=recovery_handoff issue={} evidence={}",
                    latest.identifier, evidence
                );
            }
        }
    }
    let workspace_preflight_result = if recovery_handoff_applied {
        run_loop_recovery_preflight_launch_workspace(config, &latest, &mut handoff)
    } else {
        run_loop_preflight_launch_workspace(config, &latest, &mut handoff)
    };
    let workspace_preflight = match workspace_preflight_result {
        Ok(preflight) => preflight,
        Err(error) => {
            handle_run_loop_handoff_failure(adapter, &latest, &error, options, config)?;
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    };
    for evidence in &workspace_preflight.evidence {
        println!(
            "run_loop_action=workspace_preflight issue={} evidence={}",
            latest.identifier, evidence
        );
    }

    // A Main worktree is local preparation, not a tracker claim. Resolve or
    // adopt it first so readiness observes the exact filesystem that the
    // backend and handoff verification will use.
    let live_worktree = if run_loop_live_handoff_enabled(config) {
        let runner = ProcessHandoffCommandRunner;
        let repo_root = std::env::current_dir()?;
        let worktree = prepare_issue_worktree(&repo_root, &handoff, &runner)?;
        println!(
            "run_loop_action=worktree issue={} workspace={} branch={} created={} phase=pre_claim",
            latest.identifier,
            worktree.workspace_path.display(),
            worktree.branch_name,
            worktree.created
        );
        print_latest_status(&LatestStatus {
            lane: "main".into(),
            category: "preflight".into(),
            action: "worktree_ready".into(),
            issue_identifier: Some(latest.identifier.clone()),
            issue_title: Some(latest.title.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            workspace: Some(worktree.workspace_path.display().to_string()),
            branch: Some(worktree.branch_name.clone()),
            session_id: None,
            next: Some("runtime readiness".into()),
        });
        Some(worktree)
    } else {
        None
    };
    let readiness_workspace = live_worktree
        .as_ref()
        .map(|worktree| worktree.workspace_path.as_path())
        .unwrap_or(handoff.workspace_path.as_path());
    let readiness = match resolve_runtime_readiness(
        &config.runtime_profile,
        &config.tracker,
        readiness_workspace,
    ) {
        Ok(readiness) => readiness,
        Err(error) => {
            let evidence_path = persist_runtime_readiness_failure(
                &config.observability.logs_root,
                &latest.identifier,
                &config.runtime_profile,
                readiness_workspace,
                &error,
            )?;
            println!(
                "run_loop_action=skip issue={} reason=runtime_readiness detail={} evidence={} tracker_mutation=false",
                latest.identifier,
                compact_evidence(&error.to_string()),
                evidence_path.display()
            );
            print_latest_status(&latest_status_for_issue(
                config,
                &latest,
                "main",
                "blocked",
                "runtime_readiness",
                Some("rerun repository onboarding".into()),
            ));
            return Ok(RunLoopWorkerOutcome::Completed);
        }
    };
    for evidence in &readiness.report.evidence {
        println!(
            "run_loop_action=runtime_readiness issue={} status={} evidence={}",
            latest.identifier,
            readiness.report.status,
            compact_evidence(evidence)
        );
    }

    // Read tracker truth again only after local readiness succeeds. The claim
    // below is based on this fresh ownership/dependency/status snapshot.
    let post_readiness_issue_ref = latest.identifier.clone();
    latest = run_with_progress_heartbeat(
        progress_spec_with_event_log(config, "github_project_read")
            .issue(post_readiness_issue_ref.clone())
            .backend(tracker_backend_label(config))
            .next("main_post_readiness_issue_read"),
        || adapter.get_issue(&post_readiness_issue_ref),
    )?
    .ok_or_else(|| format!("issue disappeared after readiness: {post_readiness_issue_ref}"))?;
    let eligibility = pool_claim_eligibility(&latest, WorkerLane::Main, worker_id, config);
    if !eligibility.is_claimable() {
        println!(
            "run_loop_action=skip issue={} reason=post_readiness_{}",
            latest.identifier,
            eligibility.skip_reason()
        );
        return Ok(RunLoopWorkerOutcome::Completed);
    }
    let refreshed_gate = evaluate_issue_for_current_source(config, &latest)?;
    if !refreshed_gate.is_dispatchable() {
        handle_run_loop_gate_failure(adapter, &latest, &refreshed_gate, options, config)?;
        return Ok(RunLoopWorkerOutcome::Completed);
    }
    match run_loop_assignee_ownership_decision(
        &latest,
        config,
        active_login.as_deref(),
        profile_login.as_deref(),
    ) {
        AssigneeOwnershipDecision::Allowed => {}
        AssigneeOwnershipDecision::Block { reason } => {
            println!(
                "run_loop_action=skip issue={} reason=post_readiness_assignee_ownership detail={}",
                latest.identifier, reason
            );
            return Ok(RunLoopWorkerOutcome::Completed);
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
    let mut ownership_workpad = run_loop_ownership_workpad(&latest, &ownership, event, &main_claim);
    ownership_workpad.push_str("\n\n### Runtime Readiness\n");
    ownership_workpad.push_str(&format!(
        "- Status: `{}`\n- Profile: `{}`\n- Profile path: `{}`\n- Workspace: `{}`\n",
        readiness.report.status,
        readiness
            .report
            .profile_id
            .as_deref()
            .unwrap_or("not_configured"),
        readiness.report.profile_path.display(),
        readiness.report.workspace.display()
    ));
    for evidence in &readiness.report.evidence {
        ownership_workpad.push_str(&format!("- Evidence: `{}`\n", compact_evidence(evidence)));
    }
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
        None => {
            let recovery_plan = if recover
                && ((runtime_state.backend == "codex"
                    && config.codex.command.contains("app-server"))
                    || runtime_state.backend == "claude-code")
                && main_recovery_plan_applicable(&runtime_state)
            {
                Some(main_recovery_plan(config, &latest, &runtime_state)?)
            } else {
                None
            };
            if let Some(recovery_plan) = &recovery_plan {
                println!(
                    "run_loop_action=recovery_mode issue={} mode={} evidence={}",
                    latest.identifier,
                    recovery_plan.mode.as_str(),
                    recovery_plan.evidence
                );
                append_runtime_supervision_event(
                    config,
                    Some(&runtime_state),
                    "MainRecoveryMode",
                    &format!(
                        "issue={} mode={} evidence={}",
                        latest.identifier,
                        recovery_plan.mode.as_str(),
                        recovery_plan.evidence
                    ),
                )?;
                if let Some(thread_id) = recovery_plan.app_server_resume_thread_id.as_deref() {
                    println!(
                        "run_loop_action=app_server_resume issue={} thread={} mode={} input=Continue",
                        latest.identifier,
                        thread_id,
                        recovery_plan.mode.as_str()
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(&runtime_state),
                        "CodexAppServerResume",
                        &format!(
                            "issue={} thread={} mode={} input=Continue",
                            latest.identifier,
                            thread_id,
                            recovery_plan.mode.as_str()
                        ),
                    )?;
                }
                if let Some(session_id) = recovery_plan.claude_resume_session_id.as_deref() {
                    println!(
                        "run_loop_action=claude_resume issue={} session={} mode={} input=Continue",
                        latest.identifier,
                        session_id,
                        recovery_plan.mode.as_str()
                    );
                    append_runtime_supervision_event(
                        config,
                        Some(&runtime_state),
                        "ClaudeStreamJsonResume",
                        &format!(
                            "issue={} session={} mode={} input=Continue",
                            latest.identifier,
                            session_id,
                            recovery_plan.mode.as_str()
                        ),
                    )?;
                }
            }
            execute_issue_once_with_options(
                workflow,
                config,
                &latest,
                &handoff.workspace_key,
                runtime_state.attempt_count,
                Some(&main_claim),
                IssueExecutionOptions {
                    app_server_resume_thread_id: recovery_plan
                        .as_ref()
                        .and_then(|plan| plan.app_server_resume_thread_id.clone()),
                    claude_resume_session_id: recovery_plan
                        .as_ref()
                        .and_then(|plan| plan.claude_resume_session_id.clone()),
                    prompt_override: recovery_plan
                        .as_ref()
                        .and_then(|plan| plan.prompt_override.clone()),
                    runtime_profile_was_resolved: true,
                    runtime_profile: readiness.profile.clone(),
                },
            )?
        }
    };
    let salvage_failed_backend_with_live_handoff =
        failed_backend_can_use_live_handoff(&result) && live_worktree.is_some();
    if salvage_failed_backend_with_live_handoff {
        append_runtime_supervision_event(
            config,
            Some(&runtime_state),
            "MainBackendFailureLiveHandoffSalvage",
            &format!(
                "issue={} backend={} message={}",
                latest.identifier, result.backend, result.message
            ),
        )?;
        println!(
            "run_loop_action=salvage_live_handoff issue={} reason=backend_failed_after_local_work message={}",
            latest.identifier,
            compact_evidence(&result.message)
        );
        result.success = true;
        result.message = format!(
            "backend failed after local work; attempting live handoff salvage: {}",
            result.message
        );
    }
    if result.success {
        apply_live_handoff_steps(
            config,
            adapter,
            &latest,
            &handoff,
            live_worktree,
            readiness.profile.as_ref(),
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

pub(crate) fn failed_backend_can_use_live_handoff(result: &IssueExecutionResult) -> bool {
    result.backend == "codex"
        && !result.success
        && !result.pending_session
        && result.usage_limit_pause.is_none()
        && result.live_handoff.is_none()
        && failed_backend_has_salvageable_transport_evidence(result)
}

fn failed_backend_has_salvageable_transport_evidence(result: &IssueExecutionResult) -> bool {
    const APP_SERVER_STALL: &str = "Codex app-server stalled waiting for turn event";
    if result.message.contains(APP_SERVER_STALL) {
        return true;
    }

    let Some(path) = result.backend_log_path.as_ref() else {
        return false;
    };
    fs::read_to_string(path)
        .map(|content| content.contains(APP_SERVER_STALL))
        .unwrap_or(false)
}

fn ensure_parent_integration_branch_evidence(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
) -> Result<(), Box<dyn std::error::Error>> {
    if handoff.branch_target.role != BranchTargetRole::Subissue {
        return Ok(());
    }
    let Some(parent_issue_ref) = handoff.branch_target.parent_issue.as_deref() else {
        return Ok(());
    };
    let Some(parent_integration_branch) =
        handoff.branch_target.parent_integration_branch.as_deref()
    else {
        return Ok(());
    };

    let parent_issue = run_with_progress_heartbeat(
        progress_spec_with_event_log(config, "github_project_read")
            .issue(parent_issue_ref.to_string())
            .backend(tracker_backend_label(config))
            .next("parent_topology_read"),
        || adapter.get_issue(parent_issue_ref),
    )?
    .ok_or_else(|| {
        format!(
            "native parent issue {parent_issue_ref} disappeared before Main parent topology ensure"
        )
    })?;
    if parent_issue_has_integration_branch_evidence(&parent_issue, parent_integration_branch) {
        println!(
            "run_loop_action=parent_topology issue={} parent={} branch={} outcome=already_recorded",
            issue.identifier, parent_issue_ref, parent_integration_branch
        );
        return Ok(());
    }

    let workpad = run_loop_parent_topology_workpad(issue, &parent_issue, handoff);
    let topology_key = recovery_key(
        "main-parent-topology-workpad",
        parent_issue_ref,
        &format!(
            "{}|{}|{}",
            parent_issue_ref, issue.identifier, parent_integration_branch
        ),
    );
    let topology_outcome = upsert_workpad_with_recovery(
        adapter,
        parent_issue_ref,
        Some(&parent_issue),
        &workpad,
        &topology_key,
    )?;
    if topology_outcome.should_record_audit() {
        append_tracker_mutation_audit(
            config,
            TrackerMutationAudit {
                command: "main loop",
                mutation_type: "workpad_write",
                issue_ref: Some(parent_issue_ref),
                target: Some(issue.identifier.clone()),
                from_state: Some(parent_issue.state.clone()),
                to_state: None,
                reason: "parent integration branch evidence",
            },
        );
    }
    println!(
        "run_loop_action=parent_topology issue={} parent={} branch={} outcome={}",
        issue.identifier,
        parent_issue_ref,
        parent_integration_branch,
        topology_outcome.as_str()
    );
    Ok(())
}

fn parent_issue_has_integration_branch_evidence(issue: &TrackerIssue, branch: &str) -> bool {
    issue.branch_name.as_deref() == Some(branch)
        || issue
            .description
            .as_deref()
            .is_some_and(|description| description.contains(branch))
        || issue
            .project_fields
            .values()
            .any(|value| project_field_contains_branch(value, branch))
}

fn project_field_contains_branch(value: &serde_json::Value, branch: &str) -> bool {
    match value {
        serde_json::Value::String(value) => value == branch || value.contains(branch),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| project_field_contains_branch(value, branch)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|value| project_field_contains_branch(value, branch)),
        _ => false,
    }
}

fn run_loop_parent_topology_workpad(
    issue: &TrackerIssue,
    parent_issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
) -> String {
    let parent_integration_branch = handoff
        .branch_target
        .parent_integration_branch
        .as_deref()
        .unwrap_or("n/a");
    let parent_final_base_branch = handoff
        .branch_target
        .parent_final_base_branch
        .as_deref()
        .unwrap_or("n/a");
    render_workpad_template(
        None,
        WorkpadTemplateId::ParentTopology,
        &[
            ("parent_issue_ref", parent_issue.identifier.clone()),
            ("parent_issue_title", parent_issue.title.clone()),
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            (
                "parent_integration_branch",
                parent_integration_branch.into(),
            ),
            ("parent_final_base_branch", parent_final_base_branch.into()),
        ],
    )
    .expect("centralized parent topology workpad template must render")
}
