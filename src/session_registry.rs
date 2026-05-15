use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;

pub const SESSION_REGISTRY_FILE: &str = "session-registry.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRegistry {
    #[serde(default)]
    pub sessions: Vec<AgentSessionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    pub issue_id: Option<String>,
    pub issue_identifier: Option<String>,
    pub issue_title: Option<String>,
    pub lane: String,
    pub actor_role: Option<String>,
    pub actor_label: Option<String>,
    pub git_author: Option<String>,
    pub profile_id: Option<String>,
    pub instance_name: Option<String>,
    pub worktree: PathBuf,
    pub branch: Option<String>,
    pub backend: String,
    pub session_name: String,
    pub pane_target: String,
    pub prompt_artifact_path: PathBuf,
    pub log_path: PathBuf,
    pub attach_command: String,
    pub attempt: u32,
    pub status: SessionStatus,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
}

#[derive(Debug, Error)]
pub enum SessionRegistryError {
    #[error("session registry io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session registry serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn session_registry_path(config: &RuntimeConfig) -> PathBuf {
    let mut path = config.artifacts.root.clone();
    if let Some(namespace) = &config.artifacts.namespace {
        path = path.join(namespace);
    }
    path.join("default")
        .join("sessions")
        .join(SESSION_REGISTRY_FILE)
}

pub fn load_session_registry(path: &Path) -> Result<SessionRegistry, SessionRegistryError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SessionRegistry {
            sessions: Vec::new(),
        }),
        Err(error) => Err(error.into()),
    }
}

pub fn save_session_record(
    path: &Path,
    record: AgentSessionRecord,
) -> Result<(), SessionRegistryError> {
    let mut registry = load_session_registry(path)?;
    if let Some(existing) = registry
        .sessions
        .iter_mut()
        .find(|existing| existing.session_name == record.session_name)
    {
        *existing = record;
    } else {
        registry.sessions.push(record);
    }
    save_session_registry(path, &registry)
}

pub fn save_session_registry(
    path: &Path,
    registry: &SessionRegistry,
) -> Result<(), SessionRegistryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(registry)?;
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

pub fn deterministic_session_name(
    prefix: &str,
    lane: &str,
    issue_identifier: Option<&str>,
    attempt: u32,
    slug: Option<&str>,
) -> String {
    let issue = issue_identifier
        .and_then(issue_number)
        .unwrap_or_else(|| safe_component(issue_identifier, 24));
    format!(
        "{}-{}-{}-attempt-{}-{}",
        safe_component(Some(prefix), 24),
        safe_component(Some(lane), 16),
        issue,
        attempt.max(1),
        safe_component(slug, 48)
    )
}

pub fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn issue_number(identifier: &str) -> Option<String> {
    let number = identifier
        .trim()
        .strip_prefix('#')
        .unwrap_or(identifier.trim());
    (!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
        .then(|| number.to_string())
}

fn safe_component(value: Option<&str>, max_len: usize) -> String {
    let safe = value
        .unwrap_or("run")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let safe = if safe.is_empty() { "run".into() } else { safe };
    safe.chars().take(max_len.max(1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowDefinition;

    fn config_with_artifact_root(root: &Path) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nartifacts:\n  root: {:?}\n  namespace: acme/project\n---\nPrompt",
                root.display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    #[test]
    fn session_name_includes_lane_issue_attempt_and_slug() {
        let name = deterministic_session_name(
            "jade",
            "main",
            Some("#225"),
            3,
            Some("Add durable tmux session registry and naming contract"),
        );

        assert!(name.starts_with("jade-main-225-attempt-3-add-durable-tmux"));
        assert!(name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        }));
    }

    #[test]
    fn session_registry_path_uses_artifact_root_and_namespace() {
        let temp = tempfile::tempdir().unwrap();
        let config = config_with_artifact_root(temp.path());

        assert_eq!(
            session_registry_path(&config),
            temp.path()
                .join("acme/project")
                .join("default")
                .join("sessions")
                .join(SESSION_REGISTRY_FILE)
        );
    }

    #[test]
    fn saves_and_replaces_session_records() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(SESSION_REGISTRY_FILE);
        let mut record = AgentSessionRecord {
            issue_id: Some("I_225".into()),
            issue_identifier: Some("#225".into()),
            issue_title: Some("Add registry".into()),
            lane: "main".into(),
            actor_role: Some("implementation_agent".into()),
            actor_label: Some("Jade Symphony Agent".into()),
            git_author: None,
            profile_id: None,
            instance_name: None,
            worktree: PathBuf::from("/tmp/worktree"),
            branch: Some("feature/issue-225".into()),
            backend: "tmux".into(),
            session_name: "jade-main-225-attempt-1-add-registry".into(),
            pane_target: "jade-main-225-attempt-1-add-registry".into(),
            prompt_artifact_path: PathBuf::from("/tmp/prompt.md"),
            log_path: PathBuf::from("/tmp/session.log"),
            attach_command: "tmux attach-session -t jade-main-225-attempt-1-add-registry".into(),
            attempt: 1,
            status: SessionStatus::Running,
            started_at_ms: 10,
            updated_at_ms: 10,
        };

        save_session_record(&path, record.clone()).unwrap();
        record.updated_at_ms = 20;
        save_session_record(&path, record).unwrap();
        let loaded = load_session_registry(&path).unwrap();

        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].updated_at_ms, 20);
    }
}
