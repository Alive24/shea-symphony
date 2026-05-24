use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::PollingSnapshot;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::workflow::WorkflowDefinition;
use serde::Serialize;

use crate::cli::DisplayMode;
use crate::lanes::main_loop::{compact_evidence, run_loop, RunLoopOptions};
use crate::lanes::merge::{merge_loop, MergeLoopOptions};
use crate::lanes::review::{review_loop, ReviewLoopOptions};
use crate::orchestration::{current_time_ms, shell_quote_display, warn_if_temporary_workflow_path};

use super::{
    build_autopilot_plan, AutopilotIssueSummary, AutopilotLanePlan, AutopilotParkedQueue,
    AutopilotPlanSnapshot, AutopilotRetryRecord,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutopilotLoopOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) once: bool,
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
    pub(crate) recover: bool,
    pub(crate) poll_interval_ms: Option<u64>,
    pub(crate) main_max_concurrent: Option<usize>,
    pub(crate) review_max_concurrent: Option<usize>,
    pub(crate) merge_max_concurrent: Option<usize>,
    pub(crate) json: bool,
}

impl AutopilotLoopOptions {
    pub(crate) fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    fn main_worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.main_max_concurrent
            .unwrap_or(config.agent.max_concurrent_agents)
            .max(1)
    }

    fn review_worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.review_max_concurrent
            .unwrap_or(config.review.max_concurrent_workers)
            .max(1)
    }

    fn merge_worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.merge_max_concurrent
            .unwrap_or(config.merge_lane.max_concurrent_workers)
            .max(1)
    }
}

pub(crate) fn autopilot_loop(
    options: AutopilotLoopOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let cancellation = install_autopilot_cancellation_handler()?;
    autopilot_loop_with_cancellation(options, cancellation)
}

fn autopilot_loop_with_cancellation(
    options: AutopilotLoopOptions,
    cancellation: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let max_iterations = options
        .iteration_limit()
        .ok_or("autopilot loop requires --max-iterations or --once for this bounded slice")?;
    let mut had_lane_error = false;
    let mut stopped_reported = false;
    let mut transient_attempt = 0u32;
    let mut recent_transient_failures = Vec::new();

    for iteration in 1..=max_iterations {
        if cancellation.load(Ordering::SeqCst) {
            let status = autopilot_loop_cancelled_status(
                &options,
                iteration.saturating_sub(1),
                "cancellation requested before next poll".into(),
            );
            print_autopilot_loop_status(&status, options.json)?;
            println!(
                "autopilot_loop=stopped reason=cancelled iterations={}",
                iteration.saturating_sub(1)
            );
            stopped_reported = true;
            break;
        }

        let checking_status =
            autopilot_loop_checking_status(&options, iteration, &recent_transient_failures);
        print_autopilot_loop_status(&checking_status, options.json)?;
        warn_if_temporary_workflow_path(&options.workflow_path);

        let workflow = match WorkflowDefinition::load(&options.workflow_path) {
            Ok(workflow) => workflow,
            Err(error) => {
                let status = autopilot_loop_blocked_status(
                    &options,
                    iteration,
                    format!("workflow_load_error={error}"),
                    &recent_transient_failures,
                );
                print_autopilot_loop_status(&status, options.json)?;
                if iteration < max_iterations
                    && autopilot_sleep_or_cancel(status.settings.poll_interval_ms, &cancellation)
                {
                    let cancelled = autopilot_loop_cancelled_status(
                        &options,
                        iteration,
                        "cancellation requested during blocked wait".into(),
                    );
                    print_autopilot_loop_status(&cancelled, options.json)?;
                    println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
                    stopped_reported = true;
                    break;
                }
                continue;
            }
        };
        let config = match RuntimeConfig::from_workflow(&workflow, &options.workflow_path).and_then(
            |config| {
                config.validate()?;
                Ok(config)
            },
        ) {
            Ok(config) => config,
            Err(error) => {
                let status = autopilot_loop_blocked_status(
                    &options,
                    iteration,
                    format!("workflow_config_error={error}"),
                    &recent_transient_failures,
                );
                print_autopilot_loop_status(&status, options.json)?;
                if iteration < max_iterations
                    && autopilot_sleep_or_cancel(status.settings.poll_interval_ms, &cancellation)
                {
                    let cancelled = autopilot_loop_cancelled_status(
                        &options,
                        iteration,
                        "cancellation requested during blocked wait".into(),
                    );
                    print_autopilot_loop_status(&cancelled, options.json)?;
                    println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
                    stopped_reported = true;
                    break;
                }
                continue;
            }
        };
        let settings = AutopilotLoopSettings::from_config(&config, &options);
        let tick_settings = AutopilotLoopTickSettings::from_options(&config, &options);

        let plan = match build_autopilot_plan(&options.workflow_path) {
            Ok(plan) => plan,
            Err(error) => {
                let error_text = error.to_string();
                if autopilot_failure_is_recoverable(&error_text) {
                    transient_attempt = transient_attempt.saturating_add(1);
                    let retry_delay_ms =
                        Orchestrator::new(config.clone()).retry_delay_ms(transient_attempt, false);
                    let failure = AutopilotTransientFailure {
                        at_ms: current_time_ms(),
                        attempt: transient_attempt,
                        delay_ms: retry_delay_ms,
                        error: compact_evidence(&error_text),
                        recovery_policy: "retry_with_backoff".into(),
                    };
                    recent_transient_failures.push(failure);
                    keep_recent_transient_failures(&mut recent_transient_failures);
                    let status = autopilot_loop_failure_status(
                        &options,
                        settings,
                        iteration,
                        &recent_transient_failures,
                        "retrying",
                        Some(retry_delay_ms),
                    );
                    print_autopilot_loop_status(&status, options.json)?;
                    if iteration < max_iterations
                        && autopilot_sleep_or_cancel(retry_delay_ms, &cancellation)
                    {
                        let cancelled = autopilot_loop_cancelled_status(
                            &options,
                            iteration,
                            "cancellation requested during retry backoff".into(),
                        );
                        print_autopilot_loop_status(&cancelled, options.json)?;
                        println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
                        stopped_reported = true;
                        break;
                    }
                    continue;
                }

                let status = autopilot_loop_failure_status(
                    &options,
                    settings,
                    iteration,
                    &[AutopilotTransientFailure {
                        at_ms: current_time_ms(),
                        attempt: 1,
                        delay_ms: settings.poll_interval_ms,
                        error: compact_evidence(&error_text),
                        recovery_policy: "blocked_unrecoverable".into(),
                    }],
                    "blocked",
                    Some(settings.poll_interval_ms),
                );
                print_autopilot_loop_status(&status, options.json)?;
                if iteration < max_iterations
                    && autopilot_sleep_or_cancel(settings.poll_interval_ms, &cancellation)
                {
                    let cancelled = autopilot_loop_cancelled_status(
                        &options,
                        iteration,
                        "cancellation requested during blocked wait".into(),
                    );
                    print_autopilot_loop_status(&cancelled, options.json)?;
                    println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
                    stopped_reported = true;
                    break;
                }
                continue;
            }
        };

        transient_attempt = 0;
        let status = autopilot_loop_status_from_plan(
            &plan,
            settings,
            iteration,
            Some(settings.poll_interval_ms),
            &recent_transient_failures,
            cancellation.load(Ordering::SeqCst),
        );
        print_autopilot_loop_status(&status, options.json)?;
        if status.phase == "blocked" {
            if iteration < max_iterations
                && autopilot_sleep_or_cancel(settings.poll_interval_ms, &cancellation)
            {
                let cancelled = autopilot_loop_status_from_plan(
                    &plan,
                    settings,
                    iteration,
                    None,
                    &recent_transient_failures,
                    true,
                );
                print_autopilot_loop_status(&cancelled, options.json)?;
                println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
                stopped_reported = true;
                break;
            }
            continue;
        }

        if cancellation.load(Ordering::SeqCst) {
            let cancelled = autopilot_loop_status_from_plan(
                &plan,
                settings,
                iteration,
                None,
                &recent_transient_failures,
                true,
            );
            print_autopilot_loop_status(&cancelled, options.json)?;
            println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
            stopped_reported = true;
            break;
        }

        println!(
            "autopilot_loop_iteration={} mode={} order=main,review,merge recover={} main_max_concurrent={} review_max_concurrent={} merge_max_concurrent={}",
            iteration,
            if options.write { "write" } else { "dry-run" },
            tick_settings.recover,
            tick_settings.main_max_concurrent,
            tick_settings.review_max_concurrent,
            tick_settings.merge_max_concurrent
        );

        let lane_results = vec![
            autopilot_main_tick(&options, &tick_settings, Some(&plan)),
            autopilot_review_tick(&options, &tick_settings, Some(&plan)),
            autopilot_merge_tick(&options, &tick_settings, Some(&plan)),
        ];
        had_lane_error |= lane_results.iter().any(|result| result.status == "error");

        let result = AutopilotLoopIterationResult {
            schema_version: 1,
            iteration,
            mode: if options.write { "write" } else { "dry-run" }.into(),
            execution_order: vec!["main".into(), "review".into(), "merge".into()],
            settings: tick_settings,
            lanes: lane_results,
            parked_queues: plan.parked_queues,
        };
        if options.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("{}", render_autopilot_loop_iteration_result(&result));
        }

        if iteration < max_iterations
            && autopilot_sleep_or_cancel(settings.poll_interval_ms, &cancellation)
        {
            let cancelled = autopilot_loop_cancelled_status(
                &options,
                iteration,
                "cancellation requested before next poll".into(),
            );
            print_autopilot_loop_status(&cancelled, options.json)?;
            println!("autopilot_loop=stopped reason=cancelled iterations={iteration}");
            stopped_reported = true;
            break;
        }
    }

    if had_lane_error {
        Err("one or more autopilot lane ticks failed; see per-lane results above".into())
    } else {
        if !stopped_reported {
            println!("autopilot_loop=stopped reason=max_iterations iterations={max_iterations}");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLoopTickSettings {
    pub(crate) recover: bool,
    pub(crate) main_max_concurrent: usize,
    pub(crate) review_max_concurrent: usize,
    pub(crate) merge_max_concurrent: usize,
}

impl AutopilotLoopTickSettings {
    pub(crate) fn from_options(config: &RuntimeConfig, options: &AutopilotLoopOptions) -> Self {
        Self {
            recover: options.recover,
            main_max_concurrent: options.main_worker_limit(config),
            review_max_concurrent: options.review_worker_limit(config),
            merge_max_concurrent: options.merge_worker_limit(config),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotLoopIterationResult {
    schema_version: u8,
    iteration: usize,
    mode: String,
    execution_order: Vec<String>,
    settings: AutopilotLoopTickSettings,
    lanes: Vec<AutopilotLoopLaneResult>,
    parked_queues: Vec<AutopilotParkedQueue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AutopilotLoopLaneResult {
    lane: String,
    status: String,
    action: String,
    selected_issue: Option<AutopilotIssueSummary>,
    target_state: Option<String>,
    max_concurrent: usize,
    recover: bool,
    evidence: Vec<String>,
}

fn autopilot_main_tick(
    options: &AutopilotLoopOptions,
    settings: &AutopilotLoopTickSettings,
    plan: Option<&AutopilotPlanSnapshot>,
) -> AutopilotLoopLaneResult {
    let lane_plan = autopilot_plan_lane(plan, "main");
    let result = run_loop(RunLoopOptions {
        workflow_path: options.workflow_path.clone(),
        max_iterations: Some(1),
        once: false,
        write: options.write,
        recover: settings.recover,
        max_concurrent: Some(settings.main_max_concurrent),
        display: DisplayMode::Plain,
    });
    autopilot_lane_result_from_execution(
        "main",
        lane_plan,
        settings.main_max_concurrent,
        settings.recover,
        result,
    )
}

fn autopilot_review_tick(
    options: &AutopilotLoopOptions,
    settings: &AutopilotLoopTickSettings,
    plan: Option<&AutopilotPlanSnapshot>,
) -> AutopilotLoopLaneResult {
    let lane_plan = autopilot_plan_lane(plan, "review");
    let result = review_loop(ReviewLoopOptions {
        workflow_path: options.workflow_path.clone(),
        max_iterations: Some(1),
        once: false,
        write: options.write,
        fake_outcome: None,
        max_concurrent: Some(settings.review_max_concurrent),
    });
    autopilot_lane_result_from_execution(
        "review",
        lane_plan,
        settings.review_max_concurrent,
        false,
        result,
    )
}

fn autopilot_merge_tick(
    options: &AutopilotLoopOptions,
    settings: &AutopilotLoopTickSettings,
    plan: Option<&AutopilotPlanSnapshot>,
) -> AutopilotLoopLaneResult {
    let lane_plan = autopilot_plan_lane(plan, "merge");
    let result = merge_loop(MergeLoopOptions {
        workflow_path: options.workflow_path.clone(),
        max_iterations: Some(1),
        once: false,
        write: options.write,
        recover: settings.recover,
        max_concurrent: Some(settings.merge_max_concurrent),
    });
    autopilot_lane_result_from_execution(
        "merge",
        lane_plan,
        settings.merge_max_concurrent,
        settings.recover,
        result,
    )
}

fn autopilot_plan_lane<'a>(
    plan: Option<&'a AutopilotPlanSnapshot>,
    lane: &str,
) -> Option<&'a AutopilotLanePlan> {
    plan.and_then(|snapshot| {
        snapshot
            .lanes
            .iter()
            .find(|candidate| candidate.lane == lane)
    })
}

fn autopilot_lane_result_from_execution(
    lane: &str,
    lane_plan: Option<&AutopilotLanePlan>,
    max_concurrent: usize,
    recover: bool,
    result: Result<(), Box<dyn std::error::Error>>,
) -> AutopilotLoopLaneResult {
    let mut evidence = lane_plan
        .map(|plan| plan.evidence.clone())
        .unwrap_or_else(|| vec!["source=autopilot_loop".into()]);
    if let Some(plan) = lane_plan {
        evidence.push(format!("planned_status={}", plan.status));
        evidence.push(format!("planned_action={}", plan.proposed_action));
        evidence.push(format!("planned_reason={}", plan.reason));
    }
    let (status, action) = match result {
        Ok(()) => ("completed".into(), "lane_tick_completed".into()),
        Err(error) => {
            evidence.push(format!("error={}", compact_evidence(&error.to_string())));
            ("error".into(), "tick_failed".into())
        }
    };

    AutopilotLoopLaneResult {
        lane: lane.into(),
        status,
        action,
        selected_issue: lane_plan.and_then(|plan| plan.selected_issue.clone()),
        target_state: lane_plan.and_then(|plan| plan.target_state.clone()),
        max_concurrent,
        recover,
        evidence,
    }
}

fn render_autopilot_loop_iteration_result(result: &AutopilotLoopIterationResult) -> String {
    let mut lines = vec![format!(
        "autopilot_loop_result iteration={} mode={} order={} recover={} main_max_concurrent={} review_max_concurrent={} merge_max_concurrent={}",
        result.iteration,
        result.mode,
        result.execution_order.join(","),
        result.settings.recover,
        result.settings.main_max_concurrent,
        result.settings.review_max_concurrent,
        result.settings.merge_max_concurrent
    )];
    for lane in &result.lanes {
        let selected = lane
            .selected_issue
            .as_ref()
            .map(|issue| issue.identifier.as_str())
            .unwrap_or("none");
        lines.push(format!(
            "autopilot_loop_lane lane={} status={} action={} selected={} target={} max_concurrent={} recover={}",
            lane.lane,
            lane.status,
            lane.action,
            selected,
            lane.target_state.as_deref().unwrap_or("none"),
            lane.max_concurrent,
            lane.recover
        ));
    }
    for queue in &result.parked_queues {
        lines.push(format!(
            "autopilot_loop_parked_queue name={} state={} count={}",
            shell_quote_display(&queue.name),
            shell_quote_display(&queue.state),
            queue.count
        ));
    }
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLoopSettings {
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
    pub(crate) poll_interval_ms: u64,
    pub(crate) main_max_concurrent: usize,
    pub(crate) review_max_concurrent: usize,
    pub(crate) merge_max_concurrent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLoopStatusSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) workflow_path: String,
    pub(crate) iteration: usize,
    pub(crate) mode: String,
    pub(crate) phase: String,
    pub(crate) message: String,
    pub(crate) cancellation_requested: bool,
    pub(crate) polling: PollingSnapshot,
    pub(crate) settings: AutopilotLoopSettings,
    pub(crate) lane_activity: Vec<AutopilotLaneActivity>,
    pub(crate) counts: AutopilotLoopCounts,
    pub(crate) selected_issues: Vec<AutopilotIssueSummary>,
    pub(crate) active_issues: Vec<super::AutopilotActiveIssue>,
    pub(crate) parked_queues: Vec<AutopilotParkedQueue>,
    pub(crate) blocked_reasons: Vec<String>,
    pub(crate) retrying: Vec<AutopilotRetryRecord>,
    pub(crate) recent_transient_failures: Vec<AutopilotTransientFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLaneActivity {
    pub(crate) lane: String,
    pub(crate) status: String,
    pub(crate) action: String,
    pub(crate) selected_issue: Option<AutopilotIssueSummary>,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub(crate) struct AutopilotLoopCounts {
    pub(crate) running: usize,
    pub(crate) retrying: usize,
    pub(crate) blocked: usize,
    pub(crate) idle: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotTransientFailure {
    pub(crate) at_ms: u64,
    pub(crate) attempt: u32,
    pub(crate) delay_ms: u64,
    pub(crate) error: String,
    pub(crate) recovery_policy: String,
}

impl AutopilotLoopSettings {
    pub(crate) fn from_config(config: &RuntimeConfig, options: &AutopilotLoopOptions) -> Self {
        Self {
            write: options.write,
            dry_run: options.dry_run || !options.write,
            poll_interval_ms: options
                .poll_interval_ms
                .unwrap_or(config.polling.interval_ms)
                .max(1),
            main_max_concurrent: options
                .main_max_concurrent
                .unwrap_or(config.agent.max_concurrent_agents)
                .max(1),
            review_max_concurrent: options
                .review_max_concurrent
                .unwrap_or(config.review.max_concurrent_workers)
                .max(1),
            merge_max_concurrent: options
                .merge_max_concurrent
                .unwrap_or(config.merge_lane.max_concurrent_workers)
                .max(1),
        }
    }

    pub(crate) fn fallback(options: &AutopilotLoopOptions) -> Self {
        Self {
            write: options.write,
            dry_run: options.dry_run || !options.write,
            poll_interval_ms: options.poll_interval_ms.unwrap_or(30_000).max(1),
            main_max_concurrent: options.main_max_concurrent.unwrap_or(1).max(1),
            review_max_concurrent: options.review_max_concurrent.unwrap_or(1).max(1),
            merge_max_concurrent: options.merge_max_concurrent.unwrap_or(1).max(1),
        }
    }
}

pub(crate) fn autopilot_loop_checking_status(
    options: &AutopilotLoopOptions,
    iteration: usize,
    recent_transient_failures: &[AutopilotTransientFailure],
) -> AutopilotLoopStatusSnapshot {
    let settings = AutopilotLoopSettings::fallback(options);
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: "checking".into(),
        message: "checking Project, lane state, runtime state, and readiness".into(),
        cancellation_requested: false,
        polling: PollingSnapshot {
            checking: true,
            next_poll_in_ms: None,
            poll_interval_ms: settings.poll_interval_ms,
        },
        settings,
        lane_activity: Vec::new(),
        counts: AutopilotLoopCounts::default(),
        selected_issues: Vec::new(),
        active_issues: Vec::new(),
        parked_queues: Vec::new(),
        blocked_reasons: Vec::new(),
        retrying: retry_records_from_transient_failures(recent_transient_failures),
        recent_transient_failures: recent_transient_failures.to_vec(),
    }
}

pub(crate) fn autopilot_loop_status_from_plan(
    plan: &AutopilotPlanSnapshot,
    settings: AutopilotLoopSettings,
    iteration: usize,
    next_poll_in_ms: Option<u64>,
    recent_transient_failures: &[AutopilotTransientFailure],
    cancellation_requested: bool,
) -> AutopilotLoopStatusSnapshot {
    let lane_activity = plan
        .lanes
        .iter()
        .map(|lane| AutopilotLaneActivity {
            lane: lane.lane.clone(),
            status: lane.status.clone(),
            action: lane.proposed_action.clone(),
            selected_issue: lane.selected_issue.clone(),
            reason: lane.reason.clone(),
        })
        .collect::<Vec<_>>();
    let selected_issues = plan
        .lanes
        .iter()
        .filter_map(|lane| lane.selected_issue.clone())
        .collect::<Vec<_>>();
    let readiness_blocker_count = plan.readiness.blockers.len();
    let mut blocked_reasons = plan.readiness.blockers.clone();
    blocked_reasons.extend(
        plan.lanes
            .iter()
            .filter(|lane| lane.status == "blocked")
            .map(|lane| format!("{}:{}", lane.lane, lane.reason)),
    );
    let mut retrying = plan.runtime.retrying.clone();
    retrying.extend(retry_records_from_transient_failures(
        recent_transient_failures,
    ));
    let counts = autopilot_loop_counts(
        &lane_activity,
        readiness_blocker_count,
        plan.runtime
            .retrying_count
            .saturating_add(recent_transient_failures.len()),
    );
    let phase = if cancellation_requested {
        "cancelled".into()
    } else {
        autopilot_loop_phase(&counts, &plan.readiness.status)
    };
    let message = match phase.as_str() {
        "cancelled" => "cancellation requested; no further lane work will be started",
        "blocked" => "blocked state is visible and non-mutating",
        "retrying" => "retry/backoff is active; loop remains alive",
        "running" => "one or more lanes have useful work ready",
        "idle" => "healthy idle; waiting for the next poll",
        _ => "checking",
    }
    .to_string();
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: plan.workflow_path.clone(),
        iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase,
        message,
        cancellation_requested,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms,
            poll_interval_ms: settings.poll_interval_ms,
        },
        settings,
        lane_activity,
        counts,
        selected_issues,
        active_issues: plan.runtime.active_issues.clone(),
        parked_queues: plan.parked_queues.clone(),
        blocked_reasons,
        retrying,
        recent_transient_failures: recent_transient_failures.to_vec(),
    }
}

fn autopilot_loop_blocked_status(
    options: &AutopilotLoopOptions,
    iteration: usize,
    reason: String,
    recent_transient_failures: &[AutopilotTransientFailure],
) -> AutopilotLoopStatusSnapshot {
    let settings = AutopilotLoopSettings::fallback(options);
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: "blocked".into(),
        message: "blocked before mutation; operator intervention or config repair is required"
            .into(),
        cancellation_requested: false,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms: Some(settings.poll_interval_ms),
            poll_interval_ms: settings.poll_interval_ms,
        },
        settings,
        lane_activity: Vec::new(),
        counts: AutopilotLoopCounts {
            blocked: 1,
            retrying: recent_transient_failures.len(),
            ..Default::default()
        },
        selected_issues: Vec::new(),
        active_issues: Vec::new(),
        parked_queues: Vec::new(),
        blocked_reasons: vec![reason],
        retrying: retry_records_from_transient_failures(recent_transient_failures),
        recent_transient_failures: recent_transient_failures.to_vec(),
    }
}

pub(crate) fn autopilot_loop_failure_status(
    options: &AutopilotLoopOptions,
    settings: AutopilotLoopSettings,
    iteration: usize,
    recent_failures: &[AutopilotTransientFailure],
    phase: &str,
    next_poll_in_ms: Option<u64>,
) -> AutopilotLoopStatusSnapshot {
    let retrying = retry_records_from_transient_failures(recent_failures);
    let blocked_reasons = if phase == "blocked" {
        recent_failures
            .last()
            .map(|failure| vec![failure.error.clone()])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: phase.into(),
        message: if phase == "blocked" {
            "unrecoverable preflight failure; no mutation attempted".into()
        } else {
            "recoverable backend failure recorded; retry/backoff scheduled".into()
        },
        cancellation_requested: false,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms,
            poll_interval_ms: settings.poll_interval_ms,
        },
        settings,
        lane_activity: Vec::new(),
        counts: AutopilotLoopCounts {
            retrying: retrying.len(),
            blocked: usize::from(phase == "blocked"),
            ..Default::default()
        },
        selected_issues: Vec::new(),
        active_issues: Vec::new(),
        parked_queues: Vec::new(),
        blocked_reasons,
        retrying,
        recent_transient_failures: recent_failures.to_vec(),
    }
}

pub(crate) fn autopilot_loop_cancelled_status(
    options: &AutopilotLoopOptions,
    iteration: usize,
    message: String,
) -> AutopilotLoopStatusSnapshot {
    let settings = AutopilotLoopSettings::fallback(options);
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: "cancelled".into(),
        message,
        cancellation_requested: true,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms: None,
            poll_interval_ms: settings.poll_interval_ms,
        },
        settings,
        lane_activity: Vec::new(),
        counts: AutopilotLoopCounts::default(),
        selected_issues: Vec::new(),
        active_issues: Vec::new(),
        parked_queues: Vec::new(),
        blocked_reasons: Vec::new(),
        retrying: Vec::new(),
        recent_transient_failures: Vec::new(),
    }
}

fn autopilot_loop_counts(
    lane_activity: &[AutopilotLaneActivity],
    blocked_reasons: usize,
    retrying: usize,
) -> AutopilotLoopCounts {
    let mut counts = AutopilotLoopCounts {
        retrying,
        blocked: blocked_reasons,
        ..Default::default()
    };
    for lane in lane_activity {
        match lane.status.as_str() {
            "ready" | "waiting" => counts.running += 1,
            "blocked" => counts.blocked += 1,
            "idle" => counts.idle += 1,
            "retrying" => counts.retrying += 1,
            _ => {}
        }
    }
    counts
}

fn autopilot_loop_phase(counts: &AutopilotLoopCounts, readiness_status: &str) -> String {
    if counts.retrying > 0 {
        "retrying".into()
    } else if readiness_status.starts_with("blocked") || counts.blocked > 0 {
        "blocked".into()
    } else if counts.running > 0 {
        "running".into()
    } else {
        "idle".into()
    }
}

fn retry_records_from_transient_failures(
    failures: &[AutopilotTransientFailure],
) -> Vec<AutopilotRetryRecord> {
    failures
        .iter()
        .filter(|failure| failure.recovery_policy == "retry_with_backoff")
        .map(|failure| AutopilotRetryRecord {
            lane: "autopilot".into(),
            issue_identifier: None,
            attempt: failure.attempt,
            due_in_ms: failure.delay_ms,
            next_retry_at_ms: failure.at_ms.saturating_add(failure.delay_ms),
            error: failure.error.clone(),
        })
        .collect()
}

fn keep_recent_transient_failures(failures: &mut Vec<AutopilotTransientFailure>) {
    const MAX_RECENT_FAILURES: usize = 5;
    if failures.len() > MAX_RECENT_FAILURES {
        let extra = failures.len() - MAX_RECENT_FAILURES;
        failures.drain(0..extra);
    }
}

fn autopilot_loop_mode(settings: AutopilotLoopSettings) -> &'static str {
    if settings.write {
        "write"
    } else {
        "dry-run"
    }
}

pub(crate) fn autopilot_failure_is_recoverable(error: &str) -> bool {
    let value = error.to_ascii_lowercase();
    if value.contains("permission denied")
        || value.contains("invalid workflow")
        || value.contains("workflow_config_error")
        || value.contains("canonical checkout")
        || value.contains("not found in worker path")
        || value.contains("configured command")
    {
        return false;
    }
    value.contains("network")
        || value.contains("timed out")
        || value.contains("timeout")
        || value.contains("rate limit")
        || value.contains("retry-after")
        || value.contains("http 5")
        || value.contains("502")
        || value.contains("503")
        || value.contains("504")
        || value.contains("temporarily unavailable")
        || value.contains("github graphql operation failed")
        || value.contains("mergeability")
        || value.contains("merge state is `unknown`")
        || value.contains("gemini")
        || value.contains("please retry")
}

fn install_autopilot_cancellation_handler() -> Result<Arc<AtomicBool>, Box<dyn std::error::Error>> {
    let cancellation = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancellation))?;
    Ok(cancellation)
}

fn autopilot_sleep_or_cancel(delay_ms: u64, cancellation: &Arc<AtomicBool>) -> bool {
    let mut slept_ms = 0u64;
    while slept_ms < delay_ms {
        if cancellation.load(Ordering::SeqCst) {
            return true;
        }
        let step_ms = (delay_ms - slept_ms).min(250);
        thread::sleep(Duration::from_millis(step_ms));
        slept_ms = slept_ms.saturating_add(step_ms);
    }
    cancellation.load(Ordering::SeqCst)
}

fn print_autopilot_loop_status(
    status: &AutopilotLoopStatusSnapshot,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
    } else {
        println!("{}", render_autopilot_loop_status_human(status));
    }
    Ok(())
}

pub(crate) fn render_autopilot_loop_status_human(status: &AutopilotLoopStatusSnapshot) -> String {
    let mut lines = vec![
        "Autopilot Loop".to_string(),
        format!(
            "iteration={} mode={} phase={} message={}",
            status.iteration, status.mode, status.phase, status.message
        ),
        format!(
            "polling: checking={} interval_ms={} next_poll_in_ms={}",
            status.polling.checking,
            status.polling.poll_interval_ms,
            status
                .polling
                .next_poll_in_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".into())
        ),
        format!(
            "counts: running={} retrying={} blocked={} idle={}",
            status.counts.running,
            status.counts.retrying,
            status.counts.blocked,
            status.counts.idle
        ),
        format!(
            "settings: main={} review={} merge={} dry_run={} write={}",
            status.settings.main_max_concurrent,
            status.settings.review_max_concurrent,
            status.settings.merge_max_concurrent,
            status.settings.dry_run,
            status.settings.write
        ),
    ];
    if !status.lane_activity.is_empty() {
        lines.push("lanes:".into());
        for lane in &status.lane_activity {
            let selected = lane
                .selected_issue
                .as_ref()
                .map(|issue| issue.identifier.clone())
                .unwrap_or_else(|| "none".into());
            lines.push(format!(
                "- {} status={} action={} selected={} reason={}",
                lane.lane, lane.status, lane.action, selected, lane.reason
            ));
        }
    }
    if !status.parked_queues.is_empty() {
        lines.push("parked queues:".into());
        for queue in &status.parked_queues {
            lines.push(format!(
                "- {} state={} count={}",
                queue.name, queue.state, queue.count
            ));
        }
    }
    if !status.active_issues.is_empty() {
        lines.push("active issues:".into());
        for issue in &status.active_issues {
            lines.push(format!(
                "- {} lane={} backend={} session={}",
                issue.identifier,
                issue.lane,
                issue.backend,
                issue.session_id.as_deref().unwrap_or("n/a")
            ));
        }
    }
    if !status.retrying.is_empty() {
        lines.push("retrying:".into());
        for retry in &status.retrying {
            lines.push(format!(
                "- lane={} issue={} attempt={} due_in_ms={} error={}",
                retry.lane,
                retry.issue_identifier.as_deref().unwrap_or("n/a"),
                retry.attempt,
                retry.due_in_ms,
                retry.error
            ));
        }
    }
    if !status.blocked_reasons.is_empty() {
        lines.push("blocked reasons:".into());
        for reason in &status.blocked_reasons {
            lines.push(format!("- {reason}"));
        }
    }
    if !status.recent_transient_failures.is_empty() {
        lines.push("recent transient failures:".into());
        for failure in &status.recent_transient_failures {
            lines.push(format!(
                "- attempt={} delay_ms={} policy={} error={}",
                failure.attempt, failure.delay_ms, failure.recovery_policy, failure.error
            ));
        }
    }
    lines.join("\n")
}
