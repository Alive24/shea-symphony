use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::RuntimeSnapshot;
use shea_symphony::observability_api::serve_once;
use shea_symphony::status_surface::render_snapshot;
use shea_symphony::workflow::WorkflowDefinition;

use crate::orchestration::{session_status_snapshots, warn_if_temporary_workflow_path};

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
) -> Result<shea_symphony::model::RuntimeSnapshot, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;

    let session_statuses = session_status_snapshots(&config);
    let event_log_path = config
        .observability
        .logs_root
        .join("shea-symphony.jsonl")
        .display()
        .to_string();
    let mut snapshot = RuntimeSnapshot {
        event_log_path: Some(event_log_path),
        ..RuntimeSnapshot::default()
    };
    match session_statuses {
        Ok(sessions) => snapshot.sessions = sessions,
        Err(error) => snapshot
            .integration_gaps
            .push(format!("tmux session status unavailable: {error}")),
    }
    Ok(snapshot)
}

pub(crate) fn render_plan_snapshot(
    snapshot: &shea_symphony::model::RuntimeSnapshot,
    json: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if json {
        Ok(serde_json::to_string_pretty(snapshot)?)
    } else {
        Ok(render_snapshot(snapshot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_snapshot_does_not_require_project_read() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        let logs_root = temp.path().join("logs");
        std::fs::write(
            &workflow_path,
            format!(
                "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 9\nobservability:\n  logs_root: {}\n---\nPrompt",
                logs_root.display()
            ),
        )
        .unwrap();

        let snapshot = build_plan_snapshot(&workflow_path).unwrap();

        assert!(snapshot.planned.is_empty());
        assert!(snapshot.skipped.is_empty());
        assert_eq!(
            snapshot.event_log_path.as_deref(),
            Some(logs_root.join("shea-symphony.jsonl").to_str().unwrap())
        );
    }
}
