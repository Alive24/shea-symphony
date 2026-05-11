use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::workflow::WorkflowDefinition;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub tracker: TrackerConfig,
    pub polling: PollingConfig,
    pub workspace: WorkspaceConfig,
    pub hooks: HooksConfig,
    pub agent: AgentConfig,
    pub backend: BackendConfig,
    pub codex: CodexConfig,
    pub claude: ClaudeConfig,
    pub review: ReviewConfig,
    pub observability: ObservabilityConfig,
    pub server: ServerConfig,
    #[serde(default)]
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub kind: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub project_owner: Option<String>,
    pub project_number: Option<u64>,
    pub project_slug: Option<String>,
    pub status_field: String,
    pub state_map: StateMap,
    pub assignee_filter: AssigneeFilter,
    pub workpad: WorkpadConfig,
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
    pub fixture_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMap {
    pub backlog: String,
    pub todo: String,
    pub need_to_clarify: String,
    pub in_progress: String,
    pub need_human_input: String,
    pub agent_review: String,
    pub human_review: String,
    pub rework: String,
    pub merging: String,
    pub done: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssigneeFilter {
    pub source: String,
    pub allow_unassigned: bool,
    pub assignees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkpadConfig {
    pub source: String,
    pub marker: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingConfig {
    pub interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_concurrent_agents: usize,
    pub max_turns: u32,
    pub max_retry_backoff_ms: u64,
    pub max_concurrent_agents_by_state: BTreeMap<String, usize>,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendConfig {
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexConfig {
    pub command: String,
    pub approval_policy: Value,
    pub thread_sandbox: String,
    pub turn_sandbox_policy: Option<Value>,
    pub turn_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub stall_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewConfig {
    pub backend: String,
    pub gemini_command: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub dashboard_enabled: bool,
    pub refresh_ms: u64,
    pub render_interval_ms: u64,
    pub logs_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: Option<u16>,
    pub host: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid WORKFLOW.md config: {0}")]
    Invalid(String),
    #[error("unsupported tracker kind: {0}")]
    UnsupportedTracker(String),
    #[error("unsupported agent backend: {0}")]
    UnsupportedBackend(String),
}

impl RuntimeConfig {
    pub fn from_workflow(
        workflow: &WorkflowDefinition,
        workflow_path: &Path,
    ) -> Result<Self, ConfigError> {
        let workflow_dir = workflow_path.parent().unwrap_or_else(|| Path::new("."));
        let root = workflow.config.as_object().cloned().unwrap_or_default();

        let tracker = parse_tracker(root.get("tracker"), workflow_dir);
        let polling = PollingConfig {
            interval_ms: get_u64(root.get("polling"), "interval_ms").unwrap_or(30_000),
        };
        let workspace = WorkspaceConfig {
            root: resolve_path(
                get_string(root.get("workspace"), "root").as_deref(),
                workflow_dir,
                &env::temp_dir().join("symphony_workspaces"),
            ),
        };
        let hooks = HooksConfig {
            after_create: get_string(root.get("hooks"), "after_create"),
            before_run: get_string(root.get("hooks"), "before_run"),
            after_run: get_string(root.get("hooks"), "after_run"),
            before_remove: get_string(root.get("hooks"), "before_remove"),
            timeout_ms: get_u64(root.get("hooks"), "timeout_ms").unwrap_or(60_000),
        };
        let agent = AgentConfig {
            max_concurrent_agents: get_u64(root.get("agent"), "max_concurrent_agents").unwrap_or(10)
                as usize,
            max_turns: get_u64(root.get("agent"), "max_turns").unwrap_or(20) as u32,
            max_retry_backoff_ms: get_u64(root.get("agent"), "max_retry_backoff_ms")
                .unwrap_or(300_000),
            max_concurrent_agents_by_state: parse_state_limits(get_value(
                root.get("agent"),
                "max_concurrent_agents_by_state",
            )),
            backend: get_string(root.get("agent"), "backend")
                .or_else(|| get_string(root.get("backend"), "kind"))
                .unwrap_or_else(|| "dry-run".to_string()),
        };
        let backend = BackendConfig {
            kind: agent.backend.clone(),
        };
        let codex = CodexConfig {
            command: get_string(root.get("codex"), "command")
                .unwrap_or_else(|| "codex app-server".to_string()),
            approval_policy: get_value(root.get("codex"), "approval_policy")
                .cloned()
                .unwrap_or_else(default_codex_approval_policy),
            thread_sandbox: get_string(root.get("codex"), "thread_sandbox")
                .unwrap_or_else(|| "workspace-write".to_string()),
            turn_sandbox_policy: get_value(root.get("codex"), "turn_sandbox_policy").cloned(),
            turn_timeout_ms: get_u64(root.get("codex"), "turn_timeout_ms").unwrap_or(3_600_000),
            read_timeout_ms: get_u64(root.get("codex"), "read_timeout_ms").unwrap_or(5_000),
            stall_timeout_ms: get_u64(root.get("codex"), "stall_timeout_ms").unwrap_or(300_000),
        };
        let claude = ClaudeConfig {
            command: get_string(root.get("claude"), "command")
                .unwrap_or_else(|| "claude".to_string()),
        };
        let review = ReviewConfig {
            backend: get_string(root.get("review"), "backend")
                .unwrap_or_else(|| "fake".to_string()),
            gemini_command: get_string(root.get("review"), "gemini_command")
                .unwrap_or_else(|| "gemini".to_string()),
            timeout_ms: get_u64(root.get("review"), "timeout_ms").unwrap_or(600_000),
        };
        let observability = ObservabilityConfig {
            dashboard_enabled: get_bool(root.get("observability"), "dashboard_enabled")
                .unwrap_or(true),
            refresh_ms: get_u64(root.get("observability"), "refresh_ms").unwrap_or(1_000),
            render_interval_ms: get_u64(root.get("observability"), "render_interval_ms")
                .unwrap_or(16),
            logs_root: resolve_path(
                get_string(root.get("observability"), "logs_root").as_deref(),
                workflow_dir,
                &PathBuf::from("log"),
            ),
        };
        let server = ServerConfig {
            port: get_u64(root.get("server"), "port").map(|port| port as u16),
            host: get_string(root.get("server"), "host").unwrap_or_else(|| "127.0.0.1".into()),
        };

        Ok(Self {
            tracker,
            polling,
            workspace,
            hooks,
            agent,
            backend,
            codex,
            claude,
            review,
            observability,
            server,
            raw: workflow.config.clone(),
        })
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        match self.tracker.kind.as_str() {
            "github_project_v2" | "linear" | "memory" => {}
            other => return Err(ConfigError::UnsupportedTracker(other.to_string())),
        }

        match self.backend.kind.as_str() {
            "dry-run" | "codex" | "claude-code" => {}
            other => return Err(ConfigError::UnsupportedBackend(other.to_string())),
        }
        match self.review.backend.as_str() {
            "fake" | "gemini-cli" => {}
            other => return Err(ConfigError::UnsupportedBackend(other.to_string())),
        }

        require_positive("polling.interval_ms", self.polling.interval_ms)?;
        require_positive("hooks.timeout_ms", self.hooks.timeout_ms)?;
        require_positive(
            "agent.max_concurrent_agents",
            self.agent.max_concurrent_agents as u64,
        )?;
        require_positive("agent.max_turns", self.agent.max_turns as u64)?;
        require_positive(
            "agent.max_retry_backoff_ms",
            self.agent.max_retry_backoff_ms,
        )?;
        require_positive("review.timeout_ms", self.review.timeout_ms)?;

        if self.tracker.kind == "github_project_v2" {
            require_present("tracker.owner", self.tracker.owner.as_deref())?;
            require_present("tracker.repo", self.tracker.repo.as_deref())?;
            require_present(
                "tracker.project_owner",
                self.tracker.project_owner.as_deref(),
            )?;
            if self.tracker.project_number.is_none() {
                return Err(ConfigError::Invalid(
                    "tracker.project_number is required for github_project_v2".into(),
                ));
            }
        }

        if self.tracker.kind == "linear" {
            require_present("tracker.project_slug", self.tracker.project_slug.as_deref())?;
            if self.tracker.fixture_path.is_none() {
                require_present("tracker.api_key", self.tracker.api_key.as_deref())?;
            }
        }

        Ok(())
    }

    pub fn active_state_set(&self) -> Vec<String> {
        self.tracker
            .active_states
            .iter()
            .map(|state| state.trim().to_lowercase())
            .filter(|state| !state.is_empty())
            .collect()
    }

    pub fn terminal_state_set(&self) -> Vec<String> {
        self.tracker
            .terminal_states
            .iter()
            .map(|state| state.trim().to_lowercase())
            .filter(|state| !state.is_empty())
            .collect()
    }
}

fn parse_tracker(value: Option<&Value>, workflow_dir: &Path) -> TrackerConfig {
    let kind = get_string(value, "kind").unwrap_or_else(|| "github_project_v2".to_string());
    let endpoint = get_string(value, "endpoint")
        .or_else(|| (kind == "linear").then(|| "https://api.linear.app/graphql".to_string()));
    let api_key = if kind == "linear" {
        resolve_secret(get_string(value, "api_key"), "LINEAR_API_KEY")
    } else {
        resolve_secret(get_string(value, "api_key"), "GITHUB_TOKEN")
            .or_else(|| resolve_secret(None, "GH_TOKEN"))
    };

    TrackerConfig {
        kind,
        endpoint,
        api_key,
        owner: get_string(value, "owner"),
        repo: get_string(value, "repo"),
        project_owner: get_string(value, "project_owner"),
        project_number: get_u64(value, "project_number"),
        project_slug: get_string(value, "project_slug"),
        status_field: get_string(value, "status_field").unwrap_or_else(|| "Status".to_string()),
        state_map: parse_state_map(get_value(value, "state_map")),
        assignee_filter: parse_assignee_filter(get_value(value, "assignee_filter")),
        workpad: parse_workpad(get_value(value, "workpad")),
        active_states: get_string_vec(value, "active_states")
            .unwrap_or_else(|| vec!["Todo".into(), "In Progress".into(), "Rework".into()]),
        terminal_states: get_string_vec(value, "terminal_states").unwrap_or_else(|| {
            vec![
                "Closed".into(),
                "Cancelled".into(),
                "Canceled".into(),
                "Duplicate".into(),
                "Done".into(),
            ]
        }),
        fixture_path: get_string(value, "fixture_path")
            .map(|path| resolve_path(Some(&path), workflow_dir, Path::new(""))),
    }
}

fn parse_state_map(value: Option<&Value>) -> StateMap {
    StateMap {
        backlog: value_string(value, "backlog", "Backlog"),
        todo: value_string(value, "todo", "Todo"),
        need_to_clarify: value_string(value, "need_to_clarify", "Need to Clarify"),
        in_progress: value_string(value, "in_progress", "In Progress"),
        need_human_input: value_string(value, "need_human_input", "Need Human Input"),
        agent_review: value_string(value, "agent_review", "Agent Review"),
        human_review: value_string(value, "human_review", "Human Review"),
        rework: value_string(value, "rework", "Rework"),
        merging: value_string(value, "merging", "Merging"),
        done: value_string(value, "done", "Done"),
    }
}

fn parse_assignee_filter(value: Option<&Value>) -> AssigneeFilter {
    AssigneeFilter {
        source: value_string(value, "source", "issue_assignees"),
        allow_unassigned: get_bool(value, "allow_unassigned").unwrap_or(false),
        assignees: get_string_vec(value, "assignees").unwrap_or_default(),
    }
}

fn parse_workpad(value: Option<&Value>) -> WorkpadConfig {
    WorkpadConfig {
        source: value_string(value, "source", "issue_comment"),
        marker: value_string(value, "marker", "<!-- jade-symphony-workpad -->"),
    }
}

fn parse_state_limits(value: Option<&Value>) -> BTreeMap<String, usize> {
    let mut limits = BTreeMap::new();
    if let Some(Value::Object(map)) = value {
        for (state, limit) in map {
            if let Some(limit) = limit.as_u64() {
                if limit > 0 {
                    limits.insert(state.trim().to_lowercase(), limit as usize);
                }
            }
        }
    }
    limits
}

fn get_value<'a>(root: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    root.and_then(Value::as_object).and_then(|map| map.get(key))
}

fn get_string(root: Option<&Value>, key: &str) -> Option<String> {
    get_value(root, key).and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn value_string(root: Option<&Value>, key: &str, default: &str) -> String {
    get_string(root, key).unwrap_or_else(|| default.to_string())
}

fn get_u64(root: Option<&Value>, key: &str) -> Option<u64> {
    get_value(root, key).and_then(Value::as_u64)
}

fn get_bool(root: Option<&Value>, key: &str) -> Option<bool> {
    get_value(root, key).and_then(Value::as_bool)
}

fn get_string_vec(root: Option<&Value>, key: &str) -> Option<Vec<String>> {
    get_value(root, key).and_then(|value| {
        value.as_array().map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
    })
}

fn resolve_secret(value: Option<String>, fallback_env: &str) -> Option<String> {
    match value {
        Some(raw) if raw.starts_with('$') => env::var(raw.trim_start_matches('$'))
            .ok()
            .filter(|value| !value.is_empty()),
        Some(raw) if !raw.is_empty() => Some(raw),
        _ => env::var(fallback_env)
            .ok()
            .filter(|value| !value.is_empty()),
    }
}

fn resolve_path(raw: Option<&str>, workflow_dir: &Path, default: &Path) -> PathBuf {
    let value = raw
        .and_then(resolve_path_token)
        .unwrap_or_else(|| default.to_path_buf());
    let value = expand_tilde(value);
    if value.is_absolute() {
        value
    } else {
        workflow_dir.join(value)
    }
}

fn resolve_path_token(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    if let Some(env_name) = raw.strip_prefix('$') {
        return env::var(env_name)
            .ok()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
    }
    Some(PathBuf::from(raw))
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        home_dir().unwrap_or(path)
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_dir().map(|home| home.join(rest)).unwrap_or(path)
    } else {
        path
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn default_codex_approval_policy() -> Value {
    serde_json::json!({
        "reject": {
            "sandbox_approval": true,
            "rules": true,
            "mcp_elicitations": true
        }
    })
}

fn require_positive(name: &str, value: u64) -> Result<(), ConfigError> {
    if value == 0 {
        Err(ConfigError::Invalid(format!("{name} must be positive")))
    } else {
        Ok(())
    }
}

fn require_present(name: &str, value: Option<&str>) -> Result<(), ConfigError> {
    if value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        Ok(())
    } else {
        Err(ConfigError::Invalid(format!("{name} is required")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowDefinition;

    #[test]
    fn applies_github_defaults_without_leaking_into_orchestrator() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.tracker.kind, "github_project_v2");
        assert_eq!(config.tracker.status_field, "Status");
        assert_eq!(
            config.tracker.workpad.marker,
            "<!-- jade-symphony-workpad -->"
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn normalizes_state_limits() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nagent:\n  max_concurrent_agents_by_state:\n    In Progress: 2\n    bad: 0\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(
            config
                .agent
                .max_concurrent_agents_by_state
                .get("in progress"),
            Some(&2)
        );
        assert!(!config
            .agent
            .max_concurrent_agents_by_state
            .contains_key("bad"));
    }

    #[test]
    fn linear_defaults_endpoint_and_allows_fixture_without_token() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: linear\n  project_slug: jade-symphony\n  fixture_path: issues.json\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.tracker.kind, "linear");
        assert_eq!(
            config.tracker.endpoint.as_deref(),
            Some("https://api.linear.app/graphql")
        );
        assert!(config.validate().is_ok());
    }
}
