use jade_symphony::config::RuntimeConfig;
use jade_symphony::handoff::HandoffError;
use jade_symphony::model::{GateDecision, TrackerIssue};
use jade_symphony::tracker::TrackerAdapter;

use super::RunLoopOptions;
use crate::{
    gate_target_state, gate_workpad, latest_status_for_issue, print_latest_status,
    run_loop_handoff_failure_workpad,
};

pub(crate) fn handle_run_loop_gate_failure(
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    decision: &GateDecision,
    options: &RunLoopOptions,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    print_latest_status(&latest_status_for_issue(
        config,
        issue,
        "main",
        "blocked",
        "quality_gate_failed",
        Some(gate_target_state(decision).into()),
    ));
    println!(
        "run_loop_gate=failed issue={} decision={:?}",
        issue.identifier, decision.kind
    );
    if options.write {
        adapter.upsert_workpad(&issue.identifier, &gate_workpad(issue, decision))?;
        adapter.set_state(&issue.identifier, gate_target_state(decision))?;
    } else {
        println!(
            "run_loop_dry_run action=workpad issue={} reason=quality_gate_failed",
            issue.identifier
        );
        println!(
            "run_loop_dry_run action=set_state issue={} target_state={}",
            issue.identifier,
            gate_target_state(decision)
        );
    }
    Ok(())
}

pub(crate) fn handle_run_loop_handoff_failure(
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    error: &HandoffError,
    options: &RunLoopOptions,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    print_latest_status(&latest_status_for_issue(
        config,
        issue,
        "main",
        "blocked",
        "handoff_plan_failed",
        Some("Need Human Input".into()),
    ));
    println!(
        "run_loop_handoff=failed issue={} error={}",
        issue.identifier, error
    );
    let workpad = run_loop_handoff_failure_workpad(issue, error);
    if options.write {
        adapter.upsert_workpad(&issue.identifier, &workpad)?;
        adapter.set_state(&issue.identifier, "need_human_input")?;
    } else {
        println!(
            "run_loop_dry_run action=workpad issue={} reason=handoff_plan_failed",
            issue.identifier
        );
        println!(
            "run_loop_dry_run action=set_state issue={} target_state=need_human_input",
            issue.identifier
        );
    }
    Ok(())
}
