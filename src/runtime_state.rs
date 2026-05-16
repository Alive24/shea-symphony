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
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_log_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_attach_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author: Option<String>,
    pub attempt_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<RuntimeRetryState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall: Option<RuntimeStallState>,
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
            lane: None,
            run_id: None,
            backend_log_path: None,
            backend_attach_command: None,
            profile_id: None,
            instance_name: None,
            actor_role: None,
            actor_label: None,
            git_author: None,
            attempt_count: 1,
            updated_at_ms: None,
            retry: None,
            stall: None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRetryState {
    pub attempt: u32,
    pub scheduled_at_ms: u64,
    pub next_retry_at_ms: u64,
    pub error: String,
}

impl RuntimeRetryState {
    pub fn due_in_ms(&self, now_ms: u64) -> u64 {
        self.next_retry_at_ms.saturating_sub(now_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStallState {
    pub detected_at_ms: u64,
    pub stalled_for_ms: u64,
    pub reason: String,
}

pub fn mark_runtime_state_updated(state: &mut RuntimeState, now_ms: u64) {
    state.updated_at_ms = Some(now_ms);
}

pub fn record_runtime_retry(
    state: &mut RuntimeState,
    now_ms: u64,
    delay_ms: u64,
    error: impl Into<String>,
) {
    state.retry = Some(RuntimeRetryState {
        attempt: state.attempt_count,
        scheduled_at_ms: now_ms,
        next_retry_at_ms: now_ms.saturating_add(delay_ms),
        error: error.into(),
    });
    mark_runtime_state_updated(state, now_ms);
}

pub fn detect_runtime_stall(
    state: &RuntimeState,
    now_ms: u64,
    stall_timeout_ms: u64,
) -> Option<RuntimeStallState> {
    let updated_at_ms = state.updated_at_ms?;
    let stalled_for_ms = now_ms.saturating_sub(updated_at_ms);
    (stall_timeout_ms > 0 && stalled_for_ms >= stall_timeout_ms).then(|| RuntimeStallState {
        detected_at_ms: now_ms,
        stalled_for_ms,
        reason: format!("no runtime update for {stalled_for_ms}ms"),
    })
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
            lane: Some("main".into()),
            run_id: None,
            backend_log_path: Some(PathBuf::from("/tmp/jade/logs/session.log")),
            backend_attach_command: Some("tmux attach-session -t session".into()),
            profile_id: Some("codex-alpha".into()),
            instance_name: Some("Codex Alpha".into()),
            actor_role: Some("implementation_agent".into()),
            actor_label: Some("Jade Symphony Agent".into()),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            attempt_count: 2,
            updated_at_ms: Some(1_000),
            retry: None,
            stall: None,
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
    fn reads_runtime_state_without_profile_identity_for_backcompat() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("runtime-state.json");
        fs::write(
            &path,
            r##"{
  "active_issue": {"id": "GHI_1", "identifier": "#1"},
  "workspace_path": null,
  "branch_name": null,
  "backend": "dry-run",
  "backend_session_id": null,
  "attempt_count": 1,
  "last_event": "Claimed",
  "last_transition": null
}"##,
        )
        .unwrap();

        let loaded = load_runtime_state_from_path(&path).unwrap().unwrap();

        assert_eq!(loaded.profile_id, None);
        assert_eq!(loaded.instance_name, None);
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

    #[test]
    fn retry_state_reports_due_in_ms() {
        let retry = RuntimeRetryState {
            attempt: 2,
            scheduled_at_ms: 1_000,
            next_retry_at_ms: 6_000,
            error: "rate limited".into(),
        };

        assert_eq!(retry.due_in_ms(2_000), 4_000);
        assert_eq!(retry.due_in_ms(7_000), 0);
    }

    #[test]
    fn detects_stale_runtime_state_as_stall() {
        let mut state = state();
        state.updated_at_ms = Some(1_000);

        let stall = detect_runtime_stall(&state, 7_000, 5_000).unwrap();

        assert_eq!(stall.stalled_for_ms, 6_000);
        assert!(stall.reason.contains("no runtime update"));
    }
}
