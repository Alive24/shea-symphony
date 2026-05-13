use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;

pub const RUNTIME_STATE_FILE: &str = "runtime-state.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeState {
    pub active_issue: Option<RuntimeIssueState>,
    pub workspace_path: Option<PathBuf>,
    pub branch_name: Option<String>,
    pub backend: String,
    pub backend_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author: Option<String>,
    pub attempt_count: u32,
    pub last_event: Option<String>,
    pub last_transition: Option<RuntimeTransition>,
}

impl RuntimeState {
    pub fn active(issue: RuntimeIssueState, backend: impl Into<String>) -> Self {
        Self {
            active_issue: Some(issue),
            workspace_path: None,
            branch_name: None,
            backend: backend.into(),
            backend_session_id: None,
            actor_role: None,
            actor_label: None,
            git_author: None,
            attempt_count: 1,
            last_event: None,
            last_transition: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIssueState {
    pub id: String,
    pub identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeTransition {
    pub from: Option<String>,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Error)]
pub enum RuntimeStateError {
    #[error("runtime state io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime state serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn runtime_state_dir(config: &RuntimeConfig) -> PathBuf {
    config.observability.logs_root.join("runtime")
}

pub fn runtime_state_path(config: &RuntimeConfig) -> PathBuf {
    runtime_state_dir(config).join(RUNTIME_STATE_FILE)
}

pub fn load_runtime_state(
    config: &RuntimeConfig,
) -> Result<Option<RuntimeState>, RuntimeStateError> {
    load_runtime_state_from_path(&runtime_state_path(config))
}

pub fn save_runtime_state(
    config: &RuntimeConfig,
    state: &RuntimeState,
) -> Result<(), RuntimeStateError> {
    save_runtime_state_to_path(&runtime_state_path(config), state)
}

pub fn clear_runtime_state(config: &RuntimeConfig) -> Result<(), RuntimeStateError> {
    clear_runtime_state_at_path(&runtime_state_path(config))
}

pub fn load_runtime_state_from_path(
    path: &Path,
) -> Result<Option<RuntimeState>, RuntimeStateError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn save_runtime_state_to_path(
    path: &Path,
    state: &RuntimeState,
) -> Result<(), RuntimeStateError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(state)?;
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

pub fn clear_runtime_state_at_path(path: &Path) -> Result<(), RuntimeStateError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowDefinition;

    fn config_with_logs_root(root: &Path) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nobservability:\n  logs_root: {:?}\n---\nPrompt",
                root.display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn state() -> RuntimeState {
        RuntimeState {
            active_issue: Some(RuntimeIssueState {
                id: "GHI_1".into(),
                identifier: "#1".into(),
            }),
            workspace_path: Some(PathBuf::from("/tmp/jade/_1")),
            branch_name: Some("feature-issue-1".into()),
            backend: "dry-run".into(),
            backend_session_id: Some("session".into()),
            actor_role: Some("implementation_agent".into()),
            actor_label: Some("Jade Symphony Agent".into()),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            attempt_count: 2,
            last_event: Some("Completed".into()),
            last_transition: Some(RuntimeTransition {
                from: Some("In Progress".into()),
                to: "Agent Review".into(),
                reason: "main agent completed".into(),
            }),
        }
    }

    #[test]
    fn derives_runtime_state_path_from_logs_root() {
        let temp = tempfile::tempdir().unwrap();
        let config = config_with_logs_root(temp.path());

        assert_eq!(
            runtime_state_path(&config),
            temp.path().join("runtime").join(RUNTIME_STATE_FILE)
        );
    }

    #[test]
    fn missing_state_file_resumes_as_none() {
        let temp = tempfile::tempdir().unwrap();
        let loaded = load_runtime_state_from_path(&temp.path().join("missing.json")).unwrap();

        assert_eq!(loaded, None);
    }

    #[test]
    fn writes_and_reads_runtime_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("runtime").join(RUNTIME_STATE_FILE);
        let state = state();

        save_runtime_state_to_path(&path, &state).unwrap();
        let loaded = load_runtime_state_from_path(&path).unwrap();

        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn clear_runtime_state_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(RUNTIME_STATE_FILE);
        save_runtime_state_to_path(&path, &state()).unwrap();

        clear_runtime_state_at_path(&path).unwrap();
        clear_runtime_state_at_path(&path).unwrap();

        assert_eq!(load_runtime_state_from_path(&path).unwrap(), None);
    }
}
