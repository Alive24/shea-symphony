#![allow(clippy::items_after_test_module)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::PollingSnapshot;
use shea_symphony::orchestrator::Orchestrator;
use shea_symphony::workflow::WorkflowDefinition;

use crate::cli::DisplayMode;
use crate::lanes::main_loop::{compact_evidence, run_loop, RunLoopOptions};
use crate::lanes::merge::{merge_loop, MergeLoopOptions};
use crate::lanes::review::{review_loop, ReviewLoopOptions};
use crate::orchestration::{current_time_ms, shell_quote_display, warn_if_temporary_workflow_path};

use super::{
    build_autopilot_plan,
    dashboard::{render_autopilot_loop_iteration_tui, render_autopilot_loop_status_tui},
    AutopilotIssueSummary, AutopilotLanePlan, AutopilotParkedQueue, AutopilotPlanSnapshot,
    AutopilotRetryRecord,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutopilotLoopOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) once: bool,
    pub(crate) continuous: bool,
    pub(crate) write: bool,
    pub(crate) dry_run: bool,
    pub(crate) recover: bool,
    pub(crate) poll_interval_ms: Option<u64>,
    pub(crate) main_max_concurrent: Option<usize>,
    pub(crate) review_max_concurrent: Option<usize>,
    pub(crate) merge_max_concurrent: Option<usize>,
    pub(crate) display: DisplayMode,
    pub(crate) json: bool,
    pub(crate) event_json: bool,
}

impl AutopilotLoopOptions {
    pub(crate) fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else if self.continuous {
            None
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
    let work_unit_limit = options.iteration_limit();
    if work_unit_limit.is_none() && !options.continuous {
        return Err("autopilot loop requires --max-iterations, --once, or --continuous".into());
    }
    let mut had_lane_error = false;
    let mut stopped_reported = false;
    let mut transient_attempt = 0u32;
    let mut recent_transient_failures = Vec::new();
    let mut iteration = 1usize;
    let mut completed_work_units = AutopilotWorkUnitCounters::default();

    loop {
        if work_unit_limit.is_some_and(|limit| completed_work_units.total >= limit) {
            break;
        }
        if work_unit_limit.is_some_and(|limit| iteration > limit) {
            print_autopilot_stopped(
                options.event_json,
                "no_work_units",
                iteration.saturating_sub(1),
                &completed_work_units,
                work_unit_limit,
            )?;
            stopped_reported = true;
            break;
        }
        if cancellation.load(Ordering::SeqCst) {
            let status = autopilot_loop_cancelled_status(
                &options,
                iteration.saturating_sub(1),
                AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                "cancellation requested before next poll".into(),
            );
            print_autopilot_loop_status(
                &status,
                options.json,
                options.display,
                options.event_json,
            )?;
            print_autopilot_stopped(
                options.event_json,
                "cancelled",
                iteration.saturating_sub(1),
                &completed_work_units,
                work_unit_limit,
            )?;
            stopped_reported = true;
            break;
        }

        let checking_status = autopilot_loop_checking_status(
            &options,
            iteration,
            AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
            &recent_transient_failures,
        );
        print_autopilot_loop_status(
            &checking_status,
            options.json,
            options.display,
            options.event_json,
        )?;
        warn_if_temporary_workflow_path(&options.workflow_path);

        let workflow = match WorkflowDefinition::load(&options.workflow_path) {
            Ok(workflow) => workflow,
            Err(error) => {
                let status = autopilot_loop_blocked_status(
                    &options,
                    iteration,
                    format!("workflow_load_error={error}"),
                    AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                    &recent_transient_failures,
                );
                print_autopilot_loop_status(
                    &status,
                    options.json,
                    options.display,
                    options.event_json,
                )?;
                if autopilot_should_continue(iteration, work_unit_limit)
                    && autopilot_sleep_or_cancel(status.settings.poll_interval_ms, &cancellation)
                {
                    let cancelled = autopilot_loop_cancelled_status(
                        &options,
                        iteration,
                        AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                        "cancellation requested during blocked wait".into(),
                    );
                    print_autopilot_loop_status(
                        &cancelled,
                        options.json,
                        options.display,
                        options.event_json,
                    )?;
                    print_autopilot_stopped(
                        options.event_json,
                        "cancelled",
                        iteration,
                        &completed_work_units,
                        work_unit_limit,
                    )?;
                    stopped_reported = true;
                    break;
                }
                iteration = iteration.saturating_add(1);
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
                    AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                    &recent_transient_failures,
                );
                print_autopilot_loop_status(
                    &status,
                    options.json,
                    options.display,
                    options.event_json,
                )?;
                if autopilot_should_continue(iteration, work_unit_limit)
                    && autopilot_sleep_or_cancel(status.settings.poll_interval_ms, &cancellation)
                {
                    let cancelled = autopilot_loop_cancelled_status(
                        &options,
                        iteration,
                        AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                        "cancellation requested during blocked wait".into(),
                    );
                    print_autopilot_loop_status(
                        &cancelled,
                        options.json,
                        options.display,
                        options.event_json,
                    )?;
                    print_autopilot_stopped(
                        options.event_json,
                        "cancelled",
                        iteration,
                        &completed_work_units,
                        work_unit_limit,
                    )?;
                    stopped_reported = true;
                    break;
                }
                iteration = iteration.saturating_add(1);
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
                        AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                        &recent_transient_failures,
                        "retrying",
                        Some(retry_delay_ms),
                    );
                    print_autopilot_loop_status(
                        &status,
                        options.json,
                        options.display,
                        options.event_json,
                    )?;
                    if autopilot_should_continue(iteration, work_unit_limit)
                        && autopilot_sleep_or_cancel(retry_delay_ms, &cancellation)
                    {
                        let cancelled = autopilot_loop_cancelled_status(
                            &options,
                            iteration,
                            AutopilotLoopProgress::new(
                                work_unit_limit,
                                completed_work_units.clone(),
                            ),
                            "cancellation requested during retry backoff".into(),
                        );
                        print_autopilot_loop_status(
                            &cancelled,
                            options.json,
                            options.display,
                            options.event_json,
                        )?;
                        print_autopilot_stopped(
                            options.event_json,
                            "cancelled",
                            iteration,
                            &completed_work_units,
                            work_unit_limit,
                        )?;
                        stopped_reported = true;
                        break;
                    }
                    iteration = iteration.saturating_add(1);
                    continue;
                }

                let status = autopilot_loop_failure_status(
                    &options,
                    settings,
                    iteration,
                    AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
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
                print_autopilot_loop_status(
                    &status,
                    options.json,
                    options.display,
                    options.event_json,
                )?;
                if autopilot_should_continue(iteration, work_unit_limit)
                    && autopilot_sleep_or_cancel(settings.poll_interval_ms, &cancellation)
                {
                    let cancelled = autopilot_loop_cancelled_status(
                        &options,
                        iteration,
                        AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                        "cancellation requested during blocked wait".into(),
                    );
                    print_autopilot_loop_status(
                        &cancelled,
                        options.json,
                        options.display,
                        options.event_json,
                    )?;
                    print_autopilot_stopped(
                        options.event_json,
                        "cancelled",
                        iteration,
                        &completed_work_units,
                        work_unit_limit,
                    )?;
                    stopped_reported = true;
                    break;
                }
                iteration = iteration.saturating_add(1);
                continue;
            }
        };

        transient_attempt = 0;
        let status = autopilot_loop_status_from_plan_with_work_units(
            &plan,
            settings,
            iteration,
            AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
            Some(settings.poll_interval_ms),
            &recent_transient_failures,
            cancellation.load(Ordering::SeqCst),
        );
        print_autopilot_loop_status(&status, options.json, options.display, options.event_json)?;
        if status.phase == "blocked" {
            if autopilot_should_continue(iteration, work_unit_limit)
                && autopilot_sleep_or_cancel(settings.poll_interval_ms, &cancellation)
            {
                let cancelled = autopilot_loop_status_from_plan_with_work_units(
                    &plan,
                    settings,
                    iteration,
                    AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                    None,
                    &recent_transient_failures,
                    true,
                );
                print_autopilot_loop_status(
                    &cancelled,
                    options.json,
                    options.display,
                    options.event_json,
                )?;
                print_autopilot_stopped(
                    options.event_json,
                    "cancelled",
                    iteration,
                    &completed_work_units,
                    work_unit_limit,
                )?;
                stopped_reported = true;
                break;
            }
            iteration = iteration.saturating_add(1);
            continue;
        }

        if cancellation.load(Ordering::SeqCst) {
            let cancelled = autopilot_loop_status_from_plan_with_work_units(
                &plan,
                settings,
                iteration,
                AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                None,
                &recent_transient_failures,
                true,
            );
            print_autopilot_loop_status(
                &cancelled,
                options.json,
                options.display,
                options.event_json,
            )?;
            print_autopilot_stopped(
                options.event_json,
                "cancelled",
                iteration,
                &completed_work_units,
                work_unit_limit,
            )?;
            stopped_reported = true;
            break;
        }

        if !options.event_json {
            println!(
                "autopilot_loop_iteration={} mode={} order=main,review,merge recover={} main_max_concurrent={} review_max_concurrent={} merge_max_concurrent={}",
                iteration,
                if options.write { "write" } else { "dry-run" },
                tick_settings.recover,
                tick_settings.main_max_concurrent,
                tick_settings.review_max_concurrent,
                tick_settings.merge_max_concurrent
            );
        }
        print_autopilot_event(
            options.event_json,
            "autopilot_loop_iteration",
            json!({
                "iteration": iteration,
                "supervisor_cycle": iteration,
                "work_unit_limit": work_unit_limit,
                "completed_work_units": completed_work_units.total,
                "lane_work_units": completed_work_units.lanes.clone(),
                "mode": if options.write { "write" } else { "dry-run" },
                "order": ["main", "review", "merge"],
                "settings": &tick_settings,
            }),
        )?;

        let cycle_start_work_units = completed_work_units.total;
        let mut latest_plan = plan.clone();
        let mut lane_results = Vec::new();

        let main_plan = autopilot_plan_lane(Some(&latest_plan), "main");
        let mut main_result = if tick_settings.main_max_concurrent > 0
            && (autopilot_lane_plan_should_tick(main_plan)
                || autopilot_main_recovery_should_tick(Some(&latest_plan), &tick_settings))
        {
            print_autopilot_lane_running(
                "main",
                main_plan,
                tick_settings.main_max_concurrent,
                tick_settings.recover,
                options.event_json,
            )?;
            autopilot_main_tick(&options, &tick_settings, Some(&latest_plan))
        } else {
            autopilot_lane_result_from_skip(
                "main",
                main_plan,
                tick_settings.main_max_concurrent,
                tick_settings.recover,
            )
        };
        completed_work_units.record_lane_result(&mut main_result);
        print_autopilot_lane_result(&main_result, options.event_json)?;
        lane_results.push(main_result);
        latest_plan = refresh_autopilot_plan_or_keep(&options.workflow_path, latest_plan);

        let review_plan = autopilot_plan_lane(Some(&latest_plan), "review");
        let mut review_result = if tick_settings.review_max_concurrent > 0
            && autopilot_lane_plan_should_tick(review_plan)
        {
            print_autopilot_lane_running(
                "review",
                review_plan,
                tick_settings.review_max_concurrent,
                false,
                options.event_json,
            )?;
            autopilot_review_tick(&options, &tick_settings, Some(&latest_plan))
        } else {
            autopilot_lane_result_from_skip(
                "review",
                review_plan,
                tick_settings.review_max_concurrent,
                false,
            )
        };
        completed_work_units.record_lane_result(&mut review_result);
        print_autopilot_lane_result(&review_result, options.event_json)?;
        lane_results.push(review_result);
        latest_plan = refresh_autopilot_plan_or_keep(&options.workflow_path, latest_plan);

        let merge_plan = autopilot_plan_lane(Some(&latest_plan), "merge");
        let mut merge_result = if tick_settings.merge_max_concurrent > 0
            && autopilot_lane_plan_should_tick(merge_plan)
        {
            print_autopilot_lane_running(
                "merge",
                merge_plan,
                tick_settings.merge_max_concurrent,
                tick_settings.recover,
                options.event_json,
            )?;
            autopilot_merge_tick(&options, &tick_settings, Some(&latest_plan))
        } else {
            autopilot_lane_result_from_skip(
                "merge",
                merge_plan,
                tick_settings.merge_max_concurrent,
                tick_settings.recover,
            )
        };
        completed_work_units.record_lane_result(&mut merge_result);
        print_autopilot_lane_result(&merge_result, options.event_json)?;
        lane_results.push(merge_result);
        latest_plan = refresh_autopilot_plan_or_keep(&options.workflow_path, latest_plan);

        had_lane_error |= lane_results.iter().any(|result| result.status == "error");
        let work_units_completed_this_cycle = completed_work_units
            .total
            .saturating_sub(cycle_start_work_units);

        let result = AutopilotLoopIterationResult {
            schema_version: 1,
            iteration,
            supervisor_cycle: iteration,
            mode: if options.write { "write" } else { "dry-run" }.into(),
            work_unit_limit,
            completed_work_units: completed_work_units.total,
            work_units_completed_this_cycle,
            lane_work_units: completed_work_units.lanes.clone(),
            execution_order: vec!["main".into(), "review".into(), "merge".into()],
            settings: tick_settings,
            lanes: lane_results,
            parked_queues: latest_plan.parked_queues,
        };
        if options.event_json {
            // JSON signal mode emits the structured iteration event below instead of
            // legacy key=value lane lines that are intentionally parser-hostile.
        } else if options.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if options.display == DisplayMode::Tui {
            println!("{}", render_autopilot_loop_iteration_tui(&result));
        } else {
            println!("{}", render_autopilot_loop_iteration_result(&result));
        }
        print_autopilot_event(
            options.event_json,
            "autopilot_loop_result",
            serde_json::to_value(&result)?,
        )?;

        if work_unit_limit.is_some() && work_units_completed_this_cycle == 0 {
            print_autopilot_stopped(
                options.event_json,
                "no_work_units",
                iteration,
                &completed_work_units,
                work_unit_limit,
            )?;
            stopped_reported = true;
            break;
        }

        if autopilot_should_continue_by_work_units(&completed_work_units, work_unit_limit)
            && autopilot_sleep_or_cancel(settings.poll_interval_ms, &cancellation)
        {
            let cancelled = autopilot_loop_cancelled_status(
                &options,
                iteration,
                AutopilotLoopProgress::new(work_unit_limit, completed_work_units.clone()),
                "cancellation requested before next poll".into(),
            );
            print_autopilot_loop_status(
                &cancelled,
                options.json,
                options.display,
                options.event_json,
            )?;
            print_autopilot_stopped(
                options.event_json,
                "cancelled",
                iteration,
                &completed_work_units,
                work_unit_limit,
            )?;
            stopped_reported = true;
            break;
        }
        iteration = iteration.saturating_add(1);
    }

    if had_lane_error && !options.continuous {
        Err("one or more autopilot lane ticks failed; see per-lane results above".into())
    } else {
        if !stopped_reported {
            if let Some(work_unit_limit) = work_unit_limit {
                print_autopilot_stopped(
                    options.event_json,
                    "work_unit_limit",
                    iteration.saturating_sub(1),
                    &completed_work_units,
                    Some(work_unit_limit),
                )?;
            }
        }
        Ok(())
    }
}

fn autopilot_should_continue(iteration: usize, max_iterations: Option<usize>) -> bool {
    max_iterations
        .map(|limit| iteration < limit)
        .unwrap_or(true)
}

fn autopilot_should_continue_by_work_units(
    completed_work_units: &AutopilotWorkUnitCounters,
    work_unit_limit: Option<usize>,
) -> bool {
    work_unit_limit
        .map(|limit| completed_work_units.total < limit)
        .unwrap_or(true)
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
pub(super) struct AutopilotLoopIterationResult {
    pub(super) schema_version: u8,
    /// Compatibility alias for older consumers. New consumers should read
    /// `supervisor_cycle` for lifecycle and `completed_work_units` for progress.
    pub(super) iteration: usize,
    pub(super) supervisor_cycle: usize,
    pub(super) mode: String,
    pub(super) work_unit_limit: Option<usize>,
    pub(super) completed_work_units: usize,
    pub(super) work_units_completed_this_cycle: usize,
    pub(super) lane_work_units: BTreeMap<String, usize>,
    pub(super) execution_order: Vec<String>,
    pub(super) settings: AutopilotLoopTickSettings,
    pub(super) lanes: Vec<AutopilotLoopLaneResult>,
    pub(super) parked_queues: Vec<AutopilotParkedQueue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct AutopilotLoopLaneResult {
    pub(super) lane: String,
    pub(super) status: String,
    pub(super) action: String,
    pub(super) work_unit_completed: bool,
    pub(super) completed_work_units: usize,
    pub(super) issue_ref: Option<String>,
    pub(super) latest_result: AutopilotLaneLatestResult,
    pub(super) selected_issue: Option<AutopilotIssueSummary>,
    pub(super) target_state: Option<String>,
    pub(super) max_concurrent: usize,
    pub(super) recover: bool,
    pub(super) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct AutopilotLaneLatestResult {
    pub(super) status: String,
    pub(super) action: String,
    pub(super) issue_ref: Option<String>,
    pub(super) target_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub(crate) struct AutopilotWorkUnitCounters {
    pub(crate) total: usize,
    pub(crate) lanes: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutopilotLoopProgress {
    pub(crate) work_unit_limit: Option<usize>,
    pub(crate) completed_work_units: AutopilotWorkUnitCounters,
}

impl AutopilotLoopProgress {
    fn new(
        work_unit_limit: Option<usize>,
        completed_work_units: AutopilotWorkUnitCounters,
    ) -> Self {
        Self {
            work_unit_limit,
            completed_work_units,
        }
    }
}

impl AutopilotWorkUnitCounters {
    fn record_lane_result(&mut self, lane: &mut AutopilotLoopLaneResult) {
        lane.work_unit_completed = lane.status == "completed";
        if lane.work_unit_completed {
            self.total = self.total.saturating_add(1);
            *self.lanes.entry(lane.lane.clone()).or_default() += 1;
        }
        lane.completed_work_units = self.lanes.get(&lane.lane).copied().unwrap_or_default();
        lane.latest_result = AutopilotLaneLatestResult {
            status: lane.status.clone(),
            action: lane.action.clone(),
            issue_ref: lane.issue_ref.clone(),
            target_state: lane.target_state.clone(),
        };
    }
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

fn refresh_autopilot_plan_or_keep(
    workflow_path: &Path,
    fallback: AutopilotPlanSnapshot,
) -> AutopilotPlanSnapshot {
    build_autopilot_plan(workflow_path).unwrap_or(fallback)
}

fn autopilot_lane_plan_should_tick(lane_plan: Option<&AutopilotLanePlan>) -> bool {
    lane_plan
        .map(|plan| plan.status == "ready" && plan.selected_issue.is_some())
        .unwrap_or(false)
}

fn autopilot_main_recovery_should_tick(
    plan: Option<&AutopilotPlanSnapshot>,
    settings: &AutopilotLoopTickSettings,
) -> bool {
    let Some(plan) = plan else {
        return false;
    };
    settings.recover
        && settings.main_max_concurrent > 0
        && autopilot_main_recovery_blocker_is_lane_local(plan)
}

fn autopilot_main_recovery_can_tick(
    plan: &AutopilotPlanSnapshot,
    settings: AutopilotLoopSettings,
) -> bool {
    settings.write
        && settings.main_max_concurrent > 0
        && (settings.recover && autopilot_main_recovery_blocker_is_lane_local(plan)
            || autopilot_main_parent_topology_can_tick(plan))
}

fn autopilot_main_recovery_blocker_is_lane_local(plan: &AutopilotPlanSnapshot) -> bool {
    !plan.readiness.blockers.is_empty()
        && plan
            .readiness
            .blockers
            .iter()
            .all(|blocker| autopilot_main_runtime_recovery_blocker(plan, blocker))
        && plan
            .lanes
            .iter()
            .all(|lane| !lane.status.eq_ignore_ascii_case("blocked"))
}

fn autopilot_active_runtime_blocker(blocker: &str) -> bool {
    blocker.starts_with("active_runtime_states=")
}

fn autopilot_session_attention_blocker(blocker: &str) -> bool {
    blocker.starts_with("session_attention=")
}

fn autopilot_main_runtime_recovery_blocker(plan: &AutopilotPlanSnapshot, blocker: &str) -> bool {
    if autopilot_active_runtime_blocker(blocker) {
        return autopilot_runtime_active_issues_are_main_local(plan);
    }
    autopilot_session_attention_blocker(blocker)
        && autopilot_runtime_active_issues_are_main_local(plan)
}

fn autopilot_runtime_active_issues_are_main_local(plan: &AutopilotPlanSnapshot) -> bool {
    !plan.runtime.active_issues.is_empty()
        && plan
            .runtime
            .active_issues
            .iter()
            .all(|issue| issue.lane.eq_ignore_ascii_case("main"))
}

fn autopilot_main_parent_topology_can_tick(plan: &AutopilotPlanSnapshot) -> bool {
    plan.canonical_checkout.safe_for_write
        && plan.doctor.blocker_codes == ["parent_topology_missing_integration_branch"]
        && plan.readiness.blockers == ["doctor_blockers=1"]
        && autopilot_lane_plan_should_tick(autopilot_plan_lane(Some(plan), "main"))
}

fn autopilot_readiness_blocker_is_main_recoverable(
    plan: &AutopilotPlanSnapshot,
    blocker: &str,
) -> bool {
    autopilot_main_runtime_recovery_blocker(plan, blocker)
        || (blocker == "doctor_blockers=1" && autopilot_main_parent_topology_can_tick(plan))
}

fn print_autopilot_lane_running(
    lane: &str,
    lane_plan: Option<&AutopilotLanePlan>,
    max_concurrent: usize,
    recover: bool,
    event_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let selected = lane_plan
        .and_then(|plan| plan.selected_issue.as_ref())
        .map(|issue| issue.identifier.as_str())
        .unwrap_or("none");
    if !event_json {
        println!(
            "autopilot_loop_lane lane={} status=running action=tick_started selected={} target={} max_concurrent={} recover={}",
            lane,
            selected,
            lane_plan
                .and_then(|plan| plan.target_state.as_deref())
                .unwrap_or("none"),
            max_concurrent,
            recover
        );
    }
    print_autopilot_event(
        event_json,
        "autopilot_loop_lane",
        json!({
            "lane": lane,
            "status": "running",
            "action": "tick_started",
            "selected_issue": lane_plan.and_then(|plan| plan.selected_issue.clone()),
            "target_state": lane_plan.and_then(|plan| plan.target_state.clone()),
            "max_concurrent": max_concurrent,
            "recover": recover,
        }),
    )?;
    Ok(())
}

fn print_autopilot_lane_result(
    lane: &AutopilotLoopLaneResult,
    event_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let selected = lane
        .selected_issue
        .as_ref()
        .map(|issue| issue.identifier.as_str())
        .unwrap_or("none");
    if !event_json {
        println!(
            "autopilot_loop_lane lane={} status={} action={} selected={} target={} max_concurrent={} recover={}",
            lane.lane,
            lane.status,
            lane.action,
            selected,
            lane.target_state.as_deref().unwrap_or("none"),
            lane.max_concurrent,
            lane.recover
        );
    }
    print_autopilot_event(
        event_json,
        "autopilot_loop_lane",
        serde_json::to_value(lane)?,
    )?;
    Ok(())
}

fn autopilot_lane_result_from_skip(
    lane: &str,
    lane_plan: Option<&AutopilotLanePlan>,
    max_concurrent: usize,
    recover: bool,
) -> AutopilotLoopLaneResult {
    let mut evidence = lane_plan
        .map(|plan| plan.evidence.clone())
        .unwrap_or_else(|| vec!["source=autopilot_loop".into()]);
    if let Some(plan) = lane_plan {
        evidence.push(format!("planned_status={}", plan.status));
        evidence.push(format!("planned_action={}", plan.proposed_action));
        evidence.push(format!("planned_reason={}", plan.reason));
    }
    evidence.push("skip_reason=no_ready_selected_issue".into());

    AutopilotLoopLaneResult {
        lane: lane.into(),
        status: "skipped".into(),
        action: "lane_tick_skipped".into(),
        work_unit_completed: false,
        completed_work_units: 0,
        issue_ref: lane_plan
            .and_then(|plan| plan.selected_issue.as_ref())
            .map(|issue| issue.identifier.clone()),
        latest_result: AutopilotLaneLatestResult {
            status: "skipped".into(),
            action: "lane_tick_skipped".into(),
            issue_ref: lane_plan
                .and_then(|plan| plan.selected_issue.as_ref())
                .map(|issue| issue.identifier.clone()),
            target_state: lane_plan.and_then(|plan| plan.target_state.clone()),
        },
        selected_issue: lane_plan.and_then(|plan| plan.selected_issue.clone()),
        target_state: lane_plan.and_then(|plan| plan.target_state.clone()),
        max_concurrent,
        recover,
        evidence,
    }
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
    let (status, action): (String, String) = match result {
        Ok(()) => ("completed".into(), "lane_tick_completed".into()),
        Err(error) => {
            evidence.push(format!("error={}", compact_evidence(&error.to_string())));
            ("error".into(), "tick_failed".into())
        }
    };
    let issue_ref = lane_plan
        .and_then(|plan| plan.selected_issue.as_ref())
        .map(|issue| issue.identifier.clone());
    let target_state = lane_plan.and_then(|plan| plan.target_state.clone());

    AutopilotLoopLaneResult {
        lane: lane.into(),
        status: status.clone(),
        action: action.clone(),
        work_unit_completed: false,
        completed_work_units: 0,
        issue_ref: issue_ref.clone(),
        latest_result: AutopilotLaneLatestResult {
            status,
            action,
            issue_ref,
            target_state: target_state.clone(),
        },
        selected_issue: lane_plan.and_then(|plan| plan.selected_issue.clone()),
        target_state,
        max_concurrent,
        recover,
        evidence,
    }
}

fn render_autopilot_loop_iteration_result(result: &AutopilotLoopIterationResult) -> String {
    let mut lines = vec![format!(
            "autopilot_loop_result supervisor_cycle={} iteration={} mode={} work_units={} work_unit_limit={} order={} recover={} main_max_concurrent={} review_max_concurrent={} merge_max_concurrent={}",
            result.supervisor_cycle,
            result.iteration,
            result.mode,
            result.completed_work_units,
            result
                .work_unit_limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".into()),
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
            "autopilot_loop_lane lane={} status={} action={} selected={} target={} work_unit_completed={} completed_work_units={} max_concurrent={} recover={}",
            lane.lane,
            lane.status,
            lane.action,
            selected,
            lane.target_state.as_deref().unwrap_or("none"),
            lane.work_unit_completed,
            lane.completed_work_units,
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
    pub(crate) recover: bool,
    pub(crate) poll_interval_ms: u64,
    pub(crate) main_max_concurrent: usize,
    pub(crate) review_max_concurrent: usize,
    pub(crate) merge_max_concurrent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLoopStatusSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) workflow_path: String,
    /// Compatibility alias for older consumers.
    pub(crate) iteration: usize,
    pub(crate) supervisor_cycle: usize,
    pub(crate) mode: String,
    pub(crate) phase: String,
    pub(crate) message: String,
    pub(crate) cancellation_requested: bool,
    pub(crate) work_unit_limit: Option<usize>,
    pub(crate) completed_work_units: usize,
    pub(crate) lane_work_units: BTreeMap<String, usize>,
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
            recover: options.recover,
            poll_interval_ms: options
                .poll_interval_ms
                .unwrap_or(config.polling.interval_ms)
                .max(1),
            main_max_concurrent: options
                .main_max_concurrent
                .unwrap_or(config.agent.max_concurrent_agents),
            review_max_concurrent: options
                .review_max_concurrent
                .unwrap_or(config.review.max_concurrent_workers),
            merge_max_concurrent: options
                .merge_max_concurrent
                .unwrap_or(config.merge_lane.max_concurrent_workers),
        }
    }

    pub(crate) fn fallback(options: &AutopilotLoopOptions) -> Self {
        Self {
            write: options.write,
            dry_run: options.dry_run || !options.write,
            recover: options.recover,
            poll_interval_ms: options.poll_interval_ms.unwrap_or(30_000).max(1),
            main_max_concurrent: options.main_max_concurrent.unwrap_or(1),
            review_max_concurrent: options.review_max_concurrent.unwrap_or(1),
            merge_max_concurrent: options.merge_max_concurrent.unwrap_or(1),
        }
    }
}

pub(crate) fn autopilot_loop_checking_status(
    options: &AutopilotLoopOptions,
    iteration: usize,
    progress: AutopilotLoopProgress,
    recent_transient_failures: &[AutopilotTransientFailure],
) -> AutopilotLoopStatusSnapshot {
    let settings = AutopilotLoopSettings::fallback(options);
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        supervisor_cycle: iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: "checking".into(),
        message: "checking Project, lane state, runtime state, and readiness".into(),
        cancellation_requested: false,
        work_unit_limit: progress.work_unit_limit,
        completed_work_units: progress.completed_work_units.total,
        lane_work_units: progress.completed_work_units.lanes,
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

#[cfg(test)]
pub(crate) fn autopilot_loop_status_from_plan(
    plan: &AutopilotPlanSnapshot,
    settings: AutopilotLoopSettings,
    iteration: usize,
    next_poll_in_ms: Option<u64>,
    recent_transient_failures: &[AutopilotTransientFailure],
    cancellation_requested: bool,
) -> AutopilotLoopStatusSnapshot {
    autopilot_loop_status_from_plan_with_work_units(
        plan,
        settings,
        iteration,
        AutopilotLoopProgress::new(None, AutopilotWorkUnitCounters::default()),
        next_poll_in_ms,
        recent_transient_failures,
        cancellation_requested,
    )
}

pub(crate) fn autopilot_loop_status_from_plan_with_work_units(
    plan: &AutopilotPlanSnapshot,
    settings: AutopilotLoopSettings,
    iteration: usize,
    progress: AutopilotLoopProgress,
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
    let main_recovery_can_tick = autopilot_main_recovery_can_tick(plan, settings);
    let effective_readiness_blockers = if main_recovery_can_tick {
        plan.readiness
            .blockers
            .iter()
            .filter(|blocker| !autopilot_readiness_blocker_is_main_recoverable(plan, blocker))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        plan.readiness.blockers.clone()
    };
    let readiness_blocker_count = effective_readiness_blockers.len();
    let mut blocked_reasons = effective_readiness_blockers.clone();
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
    let mut counts = autopilot_loop_counts(
        &lane_activity,
        readiness_blocker_count,
        plan.runtime
            .retrying_count
            .saturating_add(recent_transient_failures.len()),
    );
    if main_recovery_can_tick && counts.running == 0 {
        counts.running = 1;
    }
    let readiness_status = if main_recovery_can_tick && effective_readiness_blockers.is_empty() {
        "ready"
    } else {
        plan.readiness.status.as_str()
    };
    let phase = if cancellation_requested {
        "cancelled".into()
    } else {
        autopilot_loop_phase(&counts, readiness_status)
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
        supervisor_cycle: iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase,
        message,
        cancellation_requested,
        work_unit_limit: progress.work_unit_limit,
        completed_work_units: progress.completed_work_units.total,
        lane_work_units: progress.completed_work_units.lanes,
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
    progress: AutopilotLoopProgress,
    recent_transient_failures: &[AutopilotTransientFailure],
) -> AutopilotLoopStatusSnapshot {
    let settings = AutopilotLoopSettings::fallback(options);
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        supervisor_cycle: iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: "blocked".into(),
        message: "blocked before mutation; operator intervention or config repair is required"
            .into(),
        cancellation_requested: false,
        work_unit_limit: progress.work_unit_limit,
        completed_work_units: progress.completed_work_units.total,
        lane_work_units: progress.completed_work_units.lanes,
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
    progress: AutopilotLoopProgress,
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
        supervisor_cycle: iteration,
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
        work_unit_limit: progress.work_unit_limit,
        completed_work_units: progress.completed_work_units.total,
        lane_work_units: progress.completed_work_units.lanes,
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
    progress: AutopilotLoopProgress,
    message: String,
) -> AutopilotLoopStatusSnapshot {
    let settings = AutopilotLoopSettings::fallback(options);
    AutopilotLoopStatusSnapshot {
        schema_version: 1,
        workflow_path: options.workflow_path.display().to_string(),
        iteration,
        supervisor_cycle: iteration,
        mode: autopilot_loop_mode(settings).into(),
        phase: "cancelled".into(),
        message,
        cancellation_requested: true,
        work_unit_limit: progress.work_unit_limit,
        completed_work_units: progress.completed_work_units.total,
        lane_work_units: progress.completed_work_units.lanes,
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

fn print_autopilot_event(
    enabled: bool,
    event: &str,
    payload: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if enabled {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "schema_version": 1,
                "source": "shea-symphony",
                "event": event,
                "payload": payload,
            }))?
        );
    }
    Ok(())
}

fn print_autopilot_stopped(
    event_json: bool,
    reason: &str,
    supervisor_cycles: usize,
    completed_work_units: &AutopilotWorkUnitCounters,
    work_unit_limit: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    print_autopilot_event(
        event_json,
        "autopilot_loop_stopped",
        json!({
            "reason": reason,
            "iterations": supervisor_cycles,
            "supervisor_cycles": supervisor_cycles,
            "work_units": completed_work_units.total,
            "completed_work_units": completed_work_units.total,
            "work_unit_limit": work_unit_limit,
            "lane_work_units": completed_work_units.lanes.clone(),
        }),
    )?;
    if !event_json {
        println!(
            "autopilot_loop=stopped reason={reason} supervisor_cycles={supervisor_cycles} iterations={supervisor_cycles} work_units={} work_unit_limit={}",
            completed_work_units.total,
            work_unit_limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".into())
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autopilot_lane_tick_requires_ready_selected_issue() {
        let idle = AutopilotLanePlan {
            lane: "merge".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_merging_issue".into(),
            evidence: vec![],
        };
        let ready = AutopilotLanePlan {
            lane: "review".into(),
            status: "ready".into(),
            selected_issue: Some(AutopilotIssueSummary {
                identifier: "#364".into(),
                title: "Add Forge and Project relationship support".into(),
                state: "Agent Review".into(),
                assignees: Vec::new(),
                url: None,
                priority: None,
                pull_request: None,
            }),
            proposed_action: "start_independent_review".into(),
            target_state: Some("Human Review | Rework | Need Human Input | unchanged".into()),
            reason: "agent_review_issue".into(),
            evidence: vec![],
        };

        assert!(!autopilot_lane_plan_should_tick(Some(&idle)));
        assert!(autopilot_lane_plan_should_tick(Some(&ready)));
        assert!(!autopilot_lane_plan_should_tick(None));
    }

    #[test]
    fn skipped_lane_result_does_not_report_tick_failure() {
        let idle = AutopilotLanePlan {
            lane: "merge".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_merging_issue".into(),
            evidence: vec!["source=merge loop dry-run selection".into()],
        };

        let result = autopilot_lane_result_from_skip("merge", Some(&idle), 3, true);

        assert_eq!(result.status, "skipped");
        assert_eq!(result.action, "lane_tick_skipped");
        assert_eq!(result.selected_issue, None);
        assert!(result
            .evidence
            .contains(&"skip_reason=no_ready_selected_issue".into()));
    }

    #[test]
    fn lane_work_unit_counters_only_count_completed_lane_results() {
        let ready = AutopilotLanePlan {
            lane: "review".into(),
            status: "ready".into(),
            selected_issue: Some(AutopilotIssueSummary {
                identifier: "#412".into(),
                title: "Report autopilot lane work units in run events".into(),
                state: "Agent Review".into(),
                assignees: Vec::new(),
                url: None,
                priority: None,
                pull_request: None,
            }),
            proposed_action: "start_independent_review".into(),
            target_state: Some("Merging".into()),
            reason: "agent_review_issue".into(),
            evidence: vec!["source=test".into()],
        };
        let idle = AutopilotLanePlan {
            lane: "merge".into(),
            status: "idle".into(),
            selected_issue: None,
            proposed_action: "idle".into(),
            target_state: None,
            reason: "no_merging_issue".into(),
            evidence: vec!["source=test".into()],
        };
        let mut counters = AutopilotWorkUnitCounters::default();
        let mut completed =
            autopilot_lane_result_from_execution("review", Some(&ready), 2, false, Ok(()));
        let mut skipped = autopilot_lane_result_from_skip("merge", Some(&idle), 1, true);

        counters.record_lane_result(&mut completed);
        counters.record_lane_result(&mut skipped);

        assert_eq!(counters.total, 1);
        assert_eq!(counters.lanes.get("review").copied(), Some(1));
        assert_eq!(counters.lanes.get("merge"), None);
        assert!(completed.work_unit_completed);
        assert_eq!(completed.completed_work_units, 1);
        assert_eq!(completed.issue_ref.as_deref(), Some("#412"));
        assert!(!skipped.work_unit_completed);
        assert_eq!(skipped.completed_work_units, 0);
    }
}

fn print_autopilot_loop_status(
    status: &AutopilotLoopStatusSnapshot,
    json: bool,
    display: DisplayMode,
    event_json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    print_autopilot_event(
        event_json,
        "autopilot_loop_status",
        serde_json::to_value(status)?,
    )?;
    if event_json {
        return Ok(());
    }
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
    } else if display == DisplayMode::Tui {
        println!("{}", render_autopilot_loop_status_tui(status));
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
