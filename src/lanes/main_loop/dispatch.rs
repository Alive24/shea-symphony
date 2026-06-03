use std::thread;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::TrackerIssue;
use shea_symphony::session_registry::session_registry_path;
use shea_symphony::tracker::adapter_from_config;
use shea_symphony::workflow::WorkflowDefinition;

use super::write_candidate::run_loop_dispatch_write_candidate;
use super::RunLoopOptions;
use crate::lanes::claim::{worker_identity, WorkerLane};
use crate::lanes::main_loop::main_app_server_smoke_gate;
use crate::orchestration::{latest_status_for_issue, print_latest_status, shell_quote_display};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunLoopWorkerOutcome {
    Completed,
    StopIteration,
}

pub(crate) fn run_loop_dispatch_write_candidates(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    selected: Vec<TrackerIssue>,
    options: &RunLoopOptions,
    recover: bool,
    iterations: usize,
    max_concurrent: usize,
) -> Result<bool, Box<dyn std::error::Error>> {
    let selected_count = selected.len();
    let worker_id = worker_identity(config, WorkerLane::Main);
    let mut handles = Vec::new();

    for (slot_index, issue) in selected.into_iter().enumerate() {
        print_run_loop_write_selection(
            config,
            &issue,
            options,
            iterations,
            max_concurrent,
            selected_count,
            slot_index,
        );
        let issue_ref = issue.identifier.clone();
        let workflow = workflow.clone();
        let config = config.clone();
        let options = options.clone();
        let worker_id = worker_id.clone();
        handles.push((
            issue_ref,
            thread::spawn(move || {
                let adapter = adapter_from_config(&config);
                run_loop_dispatch_write_candidate(
                    &workflow,
                    &config,
                    adapter.as_ref(),
                    issue,
                    recover,
                    &worker_id,
                    &options,
                )
                .map_err(|error| error.to_string())
            }),
        ));
    }

    let mut should_stop_iteration = false;
    let mut errors = Vec::new();
    for (issue_ref, handle) in handles {
        match handle.join() {
            Ok(Ok(RunLoopWorkerOutcome::Completed)) => {}
            Ok(Ok(RunLoopWorkerOutcome::StopIteration)) => should_stop_iteration = true,
            Ok(Err(error)) => errors.push(format!("{issue_ref}: {error}")),
            Err(_) => errors.push(format!("{issue_ref}: worker thread panicked")),
        }
    }

    if !errors.is_empty() {
        return Err(format!("run_loop concurrent dispatch failed: {}", errors.join("; ")).into());
    }

    Ok(should_stop_iteration)
}

fn print_run_loop_write_selection(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    options: &RunLoopOptions,
    iterations: usize,
    max_concurrent: usize,
    selected_count: usize,
    slot_index: usize,
) {
    print_latest_status(&latest_status_for_issue(
        config,
        issue,
        "main",
        "running",
        "selected",
        Some("claim or resume".into()),
    ));
    if slot_index == 0 {
        if !options.quiet_idle {
            println!(
                "run_loop_iteration={} issue={} title={:?} mode=write max_concurrent={} selected_count={}",
                iterations, issue.identifier, issue.title, max_concurrent, selected_count
            );
        }
    } else {
        if !options.quiet_idle {
            println!(
                "run_loop_iteration={} issue={} title={:?} mode=write max_concurrent={} selected_count={} slot={}",
                iterations,
                issue.identifier,
                issue.title,
                max_concurrent,
                selected_count,
                slot_index + 1
            );
        }
    }
    let smoke_gate = main_app_server_smoke_gate(config);
    println!(
        "run_loop_action=backend issue={} backend={} backend_source={} command={} approval_policy={} app_server_live_smoke_ready={} session_registry={}",
        issue.identifier,
        smoke_gate.backend,
        smoke_gate.backend_source,
        shell_quote_display(&smoke_gate.command),
        smoke_gate.approval_policy,
        smoke_gate.app_server_live_smoke_ready,
        session_registry_path(config).display()
    );
}
