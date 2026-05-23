use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use jade_symphony::config::RuntimeConfig;
use jade_symphony::observability_api::serve_once;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::status_surface::render_snapshot;
use jade_symphony::tracker::adapter_from_config;
use jade_symphony::workflow::WorkflowDefinition;

use crate::{session_status_snapshots, warn_if_temporary_workflow_path};

pub(crate) fn plan(workflow_path: PathBuf, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = build_plan_snapshot(&workflow_path)?;
    println!("{}", render_plan_snapshot(&snapshot, json)?);

    Ok(())
}

pub(crate) fn status_api(
    workflow_path: PathBuf,
    bind: SocketAddr,
    once: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !once {
        return Err("status serve currently requires --once".into());
    }
    if !bind.ip().is_loopback() {
        return Err("status serve bind address must be loopback for this first slice".into());
    }

    let snapshot = build_plan_snapshot(&workflow_path)?;
    println!("status_api=serving bind={bind} mode=once");
    let local_addr = serve_once(bind, &snapshot)?;
    println!("status_api=stopped bind={local_addr} mode=once");
    Ok(())
}

fn build_plan_snapshot(
    workflow_path: &Path,
) -> Result<jade_symphony::model::RuntimeSnapshot, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let integration_gaps = adapter.integration_gaps();
    let issues = adapter.list_project_summary_issues()?;
    let session_statuses = session_status_snapshots(&config);
    let event_log_path = config
        .observability
        .logs_root
        .join("jade-symphony.jsonl")
        .display()
        .to_string();
    let orchestrator = Orchestrator::new(config);
    let mut plan = orchestrator.plan_dispatch(issues);
    plan.integration_gaps.extend(integration_gaps);
    match session_statuses {
        Ok(sessions) => plan.snapshot.sessions = sessions,
        Err(error) => plan
            .integration_gaps
            .push(format!("tmux session status unavailable: {error}")),
    }
    plan.snapshot.integration_gaps = plan.integration_gaps.clone();
    plan.snapshot.event_log_path = Some(event_log_path);
    Ok(plan.snapshot)
}

pub(crate) fn render_plan_snapshot(
    snapshot: &jade_symphony::model::RuntimeSnapshot,
    json: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if json {
        Ok(serde_json::to_string_pretty(snapshot)?)
    } else {
        Ok(render_snapshot(snapshot))
    }
}
