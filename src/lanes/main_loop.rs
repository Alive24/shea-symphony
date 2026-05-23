use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::lane_claim::{LaneClaimActor, LaneClaimSource};
use jade_symphony::model::LatestStatus;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::presentation::{render_run_loop_panel, RunLoopPanel};
use jade_symphony::progress::run_with_progress_heartbeat;
use jade_symphony::runtime_state::{load_runtime_states, save_runtime_states};
use jade_symphony::status_surface::render_snapshot;
use jade_symphony::tracker::adapter_from_config;
use jade_symphony::workflow::WorkflowDefinition;

use crate::cli::DisplayMode;
use crate::{
    current_time_ms, evaluate_issue_for_current_source, hydrate_issue_for_evidence,
    lane_claim_for_issue, latest_status_for_issue, preflight_canonical_checkout_for_write_mode,
    print_latest_status, progress_spec_with_event_log, project_text_field, tracker_backend_label,
    unbounded_loop_sleep_ms, warn_if_temporary_workflow_path, worker_identity,
    write_lane_claim_field, WorkerLane,
};

mod dispatch;
mod dry_run;
mod execution;
mod failure;
mod handoff;
mod preflight;
mod runtime;
mod selection;
mod session;
mod supervision;
pub(crate) use dispatch::{run_loop_dispatch_write_candidates, RunLoopWorkerOutcome};
pub(crate) use dry_run::print_run_loop_dry_run_actions;
pub(crate) use execution::{
    execute_issue_once, execute_issue_once_with_workspace_key, IssueExecutionResult,
};
pub(crate) use failure::{handle_run_loop_gate_failure, handle_run_loop_handoff_failure};
pub(crate) use handoff::{
    apply_live_handoff_pr_link, compact_evidence, linked_pull_requests_contain,
    pull_request_number_from_url, run_handoff_verification, run_loop_agent_review_handoff_evidence,
    run_loop_apply_recovery_handoff, run_loop_assignee_ownership_workpad,
    run_loop_handoff_failure_workpad, run_loop_handoff_plan, run_loop_handoff_workpad,
    run_loop_live_handoff_enabled, run_loop_ownership_workpad, run_loop_runtime_ownership,
    run_loop_usage_limit_pause_workpad, HandoffVerification, RunLoopLiveHandoff,
};
pub(crate) use preflight::{ensure_write_mode_main_agent_backend, main_app_server_smoke_gate};
#[cfg(test)]
pub(crate) use runtime::RuntimeRecoveryCandidate;
#[cfg(test)]
pub(crate) use runtime::{run_loop_resume_preflight, ResumePreflightAction};
pub(crate) use runtime::{run_loop_resume_preflight_many, runtime_state_issue_identifier};
pub(crate) use selection::{
    no_dispatch_action, run_loop_assignee_ownership_decision, run_loop_claim_action,
    select_main_run_loop_issues, AssigneeOwnershipDecision, NoDispatchAction, RunLoopClaimAction,
};
pub(crate) use session::{
    main_session_active_recoverable, reconcile_main_handoff_runtime_state,
    reconcile_pending_main_session, run_loop_runtime_state_for_issue,
    run_loop_runtime_state_with_result, run_loop_runtime_state_with_transition,
    MainSessionReconciliation,
};
pub(crate) use supervision::append_runtime_supervision_event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunLoopOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) once: bool,
    pub(crate) write: bool,
    pub(crate) recover: bool,
    pub(crate) max_concurrent: Option<usize>,
    pub(crate) display: DisplayMode,
}

impl RunLoopOptions {
    pub(crate) fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    pub(crate) fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.agent.max_concurrent_agents)
            .max(1)
    }
}

pub(crate) fn run_loop(options: RunLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let mut iterations = 0usize;

    loop {
        if let Some(max) = limit {
            if iterations >= max {
                println!("run_loop=stopped reason=max_iterations iterations={iterations}");
                break;
            }
        }

        iterations += 1;
        warn_if_temporary_workflow_path(&options.workflow_path);
        let workflow = WorkflowDefinition::load(&options.workflow_path)?;
        let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
        config.validate()?;
        let max_concurrent = options.worker_limit(&config);
        if options.write {
            ensure_write_mode_main_agent_backend(&options.workflow_path, &config, "main loop")?;
        }
        preflight_canonical_checkout_for_write_mode(&config, "run_loop", options.write)?;
        let adapter = adapter_from_config(&config);
        let mut active_main_workers = 0usize;
        let mut recoverable_runtime_states = Vec::new();
        if options.write {
            let runtime_states = load_runtime_states(&config)?;
            let preflight = run_loop_resume_preflight_many(
                adapter.as_ref(),
                &config,
                &runtime_states,
                current_time_ms(),
                options.recover,
            )?;
            save_runtime_states(&config, &preflight.retained_states)?;
            active_main_workers = preflight.active_main_workers;
            recoverable_runtime_states = preflight.recoverable_states;
            if let Some(reason) = preflight.blocked {
                println!("run_loop=stopped reason=resume_preflight_blocked detail={reason}");
                break;
            }
        }
        let issues = run_with_progress_heartbeat(
            progress_spec_with_event_log(&config, "github_project_read")
                .backend(tracker_backend_label(&config))
                .next("main_queue_scan"),
            || adapter.list_queue_scan_issues(),
        )?;
        let orchestrator = Orchestrator::new(config.clone());
        let mut plan = orchestrator.plan_dispatch(issues.clone());
        plan.integration_gaps.extend(adapter.integration_gaps());
        plan.snapshot.integration_gaps = plan.integration_gaps.clone();
        plan.snapshot.event_log_path = Some(
            config
                .observability
                .logs_root
                .join("jade-symphony.jsonl")
                .display()
                .to_string(),
        );

        let available_slots = if options.write {
            max_concurrent.saturating_sub(active_main_workers)
        } else {
            max_concurrent
        };
        if options.write && available_slots == 0 {
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "run_loop_idle action=sleep reason=max_concurrent_reached active_workers={} max_concurrent={} delay_ms={delay_ms} iterations={iterations}",
                    active_main_workers, max_concurrent
                );
                thread::sleep(Duration::from_millis(delay_ms));
                continue;
            } else {
                println!(
                    "run_loop=stopped reason=max_concurrent_reached active_workers={} max_concurrent={}",
                    active_main_workers, max_concurrent
                );
                break;
            }
        }
        let worker_id = worker_identity(&config, WorkerLane::Main);
        let selected = select_main_run_loop_issues(
            if options.write && options.recover {
                &recoverable_runtime_states
            } else {
                &[]
            },
            &plan.selected,
            available_slots,
            &worker_id,
            &config,
        );

        let Some(issue) = selected.first().cloned() else {
            plan.snapshot.latest_status = Some(LatestStatus {
                lane: "main".into(),
                category: "idle".into(),
                action: "no_dispatchable_issue".into(),
                issue_identifier: None,
                issue_title: None,
                actor_label: Some(config.identity.actor_label.clone()),
                workspace: None,
                branch: None,
                session_id: None,
                next: Some("wait for Todo/Rework or stop".into()),
            });
            if options.display == DisplayMode::Tui {
                println!(
                    "{}",
                    render_run_loop_panel(RunLoopPanel {
                        snapshot: &plan.snapshot,
                        issue: None,
                        handoff: None,
                        actor_role: "Main Agent",
                        mode: if options.write { "write" } else { "dry-run" },
                        max_concurrent,
                        selected_count: 0,
                    })
                );
            } else {
                println!("{}", render_snapshot(&plan.snapshot));
            }
            match no_dispatch_action(limit, config.polling.interval_ms) {
                NoDispatchAction::Stop { reason } => {
                    println!("run_loop=stopped reason={reason} iterations={iterations}");
                    break;
                }
                NoDispatchAction::SleepAndContinue { delay_ms } => {
                    println!(
                        "run_loop_idle action=sleep delay_ms={delay_ms} iterations={iterations}"
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                    continue;
                }
            }
        };
        let dispatch_candidates = selected.clone();
        if options.write {
            let should_stop_iteration = run_loop_dispatch_write_candidates(
                &workflow,
                &config,
                dispatch_candidates,
                &options,
                options.recover,
                iterations,
                max_concurrent,
            )?;
            if should_stop_iteration {
                break;
            }
            continue;
        }

        let issue = hydrate_issue_for_evidence(adapter.as_ref(), issue, &issues)?;

        let decision = evaluate_issue_for_current_source(&config, &issue)?;
        if !decision.is_dispatchable() {
            if options.display == DisplayMode::Tui {
                println!(
                    "{}",
                    render_run_loop_panel(RunLoopPanel {
                        snapshot: &plan.snapshot,
                        issue: Some(&issue),
                        handoff: None,
                        actor_role: "Main Agent",
                        mode: if options.write { "write" } else { "dry-run" },
                        max_concurrent,
                        selected_count: selected.len(),
                    })
                );
            }
            handle_run_loop_gate_failure(adapter.as_ref(), &issue, &decision, &options, &config)?;
            continue;
        }

        print_latest_status(&latest_status_for_issue(
            &config,
            &issue,
            "main",
            if options.write { "running" } else { "waiting" },
            if options.write {
                "selected"
            } else {
                "dry_run_plan"
            },
            Some(if options.write {
                "claim or resume".into()
            } else {
                "would claim and hand off to Agent Review".into()
            }),
        ));
        println!(
            "run_loop_iteration={} issue={} title={:?} mode={} max_concurrent={} selected_count={}",
            iterations,
            issue.identifier,
            issue.title,
            if options.write { "write" } else { "dry-run" },
            max_concurrent,
            selected.len()
        );

        let handoff = match run_loop_handoff_plan(&config, &issue) {
            Ok(handoff) => handoff,
            Err(error) => {
                handle_run_loop_handoff_failure(
                    adapter.as_ref(),
                    &issue,
                    &error,
                    &options,
                    &config,
                )?;
                continue;
            }
        };

        if !options.write {
            for candidate in &selected {
                let claim = lane_claim_for_issue(
                    candidate,
                    WorkerLane::Main.claim_lane(),
                    LaneClaimActor::Codex,
                    LaneClaimSource::Loop,
                    project_text_field(candidate, WorkerLane::Main.claim_field()).as_deref(),
                )
                .with_worker(&worker_id);
                write_lane_claim_field(
                    &config,
                    adapter.as_ref(),
                    candidate,
                    WorkerLane::Main,
                    &claim,
                    false,
                )?;
            }
            print_latest_status(&LatestStatus {
                lane: "main".into(),
                category: "handoff".into(),
                action: "dry_run_handoff_plan".into(),
                issue_identifier: Some(issue.identifier.clone()),
                issue_title: Some(issue.title.clone()),
                actor_label: Some(config.identity.actor_label.clone()),
                workspace: Some(handoff.workspace_path.display().to_string()),
                branch: Some(handoff.branch_name.clone()),
                session_id: None,
                next: Some("Agent Review".into()),
            });
            if options.display == DisplayMode::Tui {
                println!(
                    "{}",
                    render_run_loop_panel(RunLoopPanel {
                        snapshot: &plan.snapshot,
                        issue: Some(&issue),
                        handoff: Some(&handoff),
                        actor_role: "Main Agent",
                        mode: "dry-run",
                        max_concurrent,
                        selected_count: selected.len(),
                    })
                );
            }
            print_run_loop_dry_run_actions(&issue, &handoff, &config)?;
            if let Some(delay_ms) = unbounded_loop_sleep_ms(limit, config.polling.interval_ms) {
                println!(
                    "run_loop_idle action=sleep reason=dry_run_would_repeat_without_mutation delay_ms={delay_ms} iterations={iterations}"
                );
                thread::sleep(Duration::from_millis(delay_ms));
            }
            continue;
        }
    }

    Ok(())
}
