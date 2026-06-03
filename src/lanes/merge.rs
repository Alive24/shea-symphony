use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::Duration;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::workflow::WorkflowDefinition;

use crate::orchestration::{enforce_canonical_checkout_before_write, single_line};

mod evidence;
mod repair;
mod selection;
mod tick;

#[cfg(test)]
pub(crate) use evidence::record_done_merge_lane_completion;
#[cfg(test)]
pub(crate) use repair::{
    finish_merge_agent_repaired_branch, merge_agent_reports_repaired,
    merge_agent_requests_human_input, stage_resolved_merge_agent_changes, MergeAgentStageFailure,
};
#[cfg(test)]
pub(crate) use selection::select_merge_worker_issues;
pub(crate) use tick::{
    merge_once_tick, merge_preflight_status, MergeOnceOutcome, MergeTickOutputScope,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeLoopOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) max_iterations: Option<usize>,
    pub(crate) once: bool,
    pub(crate) write: bool,
    pub(crate) recover: bool,
    pub(crate) max_concurrent: Option<usize>,
    pub(crate) quiet_idle: bool,
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

pub(crate) fn merge_once(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    merge_once_tick(
        workflow_path,
        write,
        false,
        false,
        MergeTickOutputScope::Direct,
    )
    .map(|_| ())
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
                if !options.quiet_idle {
                    println!("merge_loop=stopped reason=max_iterations iterations={iteration}");
                }
                break;
            }
        }

        iteration += 1;
        let mut should_sleep = false;
        if !options.quiet_idle {
            println!(
                "merge_loop_iteration={} mode={} recover={} max_concurrent={max_concurrent}",
                iteration,
                if options.write { "write" } else { "dry-run" },
                options.recover
            );
        }
        for slot in 1..=max_concurrent {
            match merge_once_tick(
                options.workflow_path.clone(),
                options.write,
                options.recover,
                options.quiet_idle,
                MergeTickOutputScope::Loop,
            )? {
                MergeOnceOutcome::NoMergingIssue => {
                    if limit.is_none() {
                        should_sleep = true;
                        if !options.quiet_idle {
                            println!(
                                "merge_loop_idle action=sleep reason=no_merging_issue delay_ms={} iterations={iteration} slot={slot}",
                                config.polling.interval_ms
                            );
                        }
                    } else {
                        if !options.quiet_idle {
                            println!(
                                "merge_loop=stopped reason=no_merging_issue iterations={iteration} slot={slot}"
                            );
                        }
                        stopped = true;
                    }
                    break;
                }
                MergeOnceOutcome::DryRun if !options.write => {
                    if !options.quiet_idle {
                        println!(
                            "merge_loop_action=dry_run_tick iterations={iteration} slot={slot}"
                        );
                    }
                    if limit.is_none() {
                        should_sleep = true;
                        break;
                    } else if max_concurrent > 1 {
                        if !options.quiet_idle {
                            println!(
                                "merge_loop=stopped reason=dry_run_would_repeat_without_mutation iterations={iteration}"
                            );
                        }
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
                    if !options.quiet_idle {
                        println!("merge_loop_action=skipped iterations={iteration} slot={slot}");
                    }
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
