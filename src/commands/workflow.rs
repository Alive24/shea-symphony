use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::progress::run_with_progress_heartbeat;
use shea_symphony::prompt_runtime::{PROMPT_RENDERER_MODE, RUNTIME_ENVELOPES};
use shea_symphony::tracker::adapter_from_config;
use shea_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::commands::gate::evaluate_issue_for_current_source;
use crate::commands::project::{filter_issues_by_state, render_state_summary};
use crate::orchestration::{
    progress_spec_for_config, tracker_backend_label, warn_if_temporary_workflow_path,
};

pub(crate) fn validate(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(&workflow_path);
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    println!("workflow={}", workflow_path.display());
    println!("tracker={}", config.tracker.kind);
    println!("backend={}", config.backend.kind);
    println!("workspace_root={}", config.workspace.root.display());
    println!("prompt_template_bytes={}", workflow.prompt_template.len());
    println!("prompt_renderer={PROMPT_RENDERER_MODE}");
    for lane in [
        AgentLane::MainAgent,
        AgentLane::ReviewAgent,
        AgentLane::MergeAgent,
    ] {
        let source = workflow.prompt_source_for_lane(lane);
        let path = source
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".into());
        println!(
            "prompt_source.{}={} path={} bytes={}",
            lane.config_key(),
            source.kind.as_str(),
            path,
            workflow.prompt_for_lane(lane).len()
        );
    }
    for envelope in RUNTIME_ENVELOPES {
        println!(
            "runtime_envelope={} lane={} backend={} path={} purpose={}",
            envelope.id, envelope.lane, envelope.backend, envelope.path, envelope.purpose
        );
    }
    println!("status=valid");
    Ok(())
}

pub(crate) fn inspect(
    workflow_path: PathBuf,
    state_filters: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("load_project_summary"),
        || adapter.list_project_summary_issues(),
    )?;
    let issues = filter_issues_by_state(issues, &state_filters);

    if !state_filters.is_empty() {
        println!("state_filter={}", state_filters.join(","));
    }
    println!("issues={}", issues.len());
    println!("{}", render_state_summary(&issues));
    for issue in issues {
        let gate = evaluate_issue_for_current_source(&config, &issue)?;
        println!(
            "- {} {} state={} gate={:?}",
            issue.identifier, issue.title, issue.state, gate.kind
        );
        if !gate.missing.is_empty() {
            println!("  missing={}", gate.missing.join(", "));
        }
        if !gate.assumptions.is_empty() {
            println!("  assumptions={}", gate.assumptions.join("; "));
        }
    }

    for gap in adapter.integration_gaps() {
        println!("integration_gap={gap}");
    }

    Ok(())
}
