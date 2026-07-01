use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::workflow::WorkflowDefinition;

pub const DEFAULT_GIT_BASE_BRANCH: &str = "main";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub tracker: TrackerConfig,
    pub git: GitConfig,
    pub polling: PollingConfig,
    pub workspace: WorkspaceConfig,
    pub worker: WorkerConfig,
    pub hooks: HooksConfig,
    pub agent: AgentConfig,
    pub backend: BackendConfig,
    pub codex: CodexConfig,
    pub claude: ClaudeConfig,
    pub tmux: TmuxConfig,
    pub review: ReviewConfig,
    pub merge_lane: MergeLaneConfig,
    pub quality_gate: QualityGateConfig,
    pub verification: VerificationConfig,
    pub profiles: ProfilesConfig,
    pub identity: IdentityConfig,
    pub artifacts: ArtifactConfig,
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
    pub project_owner_type: Option<String>,
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
pub struct GitConfig {
    pub base_branch: String,
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
pub struct WorkerConfig {
    #[serde(default)]
    pub ssh_hosts: Vec<String>,
    pub max_concurrent_agents_per_host: Option<i64>,
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
    pub model: Option<String>,
    pub reasoning_effort: String,
    pub approval_policy: Value,
    pub thread_sandbox: String,
    pub turn_sandbox_policy: Option<Value>,
    pub turn_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub stall_timeout_ms: u64,
    pub session_stale_after_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub command: String,
    pub turn_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TmuxConfig {
    pub command: String,
    pub agent_command: String,
    pub main_agent_command: Option<String>,
    pub review_agent_command: Option<String>,
    pub merge_agent_command: Option<String>,
    pub session_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewConfig {
    pub backend: String,
    pub gemini_command: String,
    pub gemini_model: Option<String>,
    pub gemini_allowed_tools: Vec<String>,
    pub timeout_ms: u64,
    pub max_concurrent_workers: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeLaneConfig {
    pub max_concurrent_workers: usize,
    pub agent_backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityGateConfig {
    pub llm: LlmQualityGateConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationConfig {
    pub commands: Vec<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmQualityGateConfig {
    pub mode: String,
    pub command: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProfilesConfig {
    pub default: Option<String>,
    pub cockpit_tools: CockpitToolsProfilesConfig,
    #[serde(default)]
    pub entries: Vec<ExecutionProfileConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CockpitToolsProfilesConfig {
    pub codex_instances_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionProfileConfig {
    pub id: String,
    pub instance_name: Option<String>,
    pub backend: Option<String>,
    pub workspace_namespace: Option<String>,
    pub user_data_dir: Option<PathBuf>,
    pub working_dir: Option<PathBuf>,
    pub extra_args: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityConfig {
    pub actor_role: String,
    pub actor_label: String,
    pub git: GitIdentityConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GitIdentityConfig {
    pub name: Option<String>,
    pub email: Option<String>,
    pub signing_key: Option<String>,
    pub extra: BTreeMap<String, String>,
}

impl GitIdentityConfig {
    pub fn author(&self) -> Option<String> {
        match (self.name.as_deref(), self.email.as_deref()) {
            (Some(name), Some(email)) => Some(format!("{name} <{email}>")),
            (Some(name), None) => Some(name.to_string()),
            (None, Some(email)) => Some(format!("<{email}>")),
            (None, None) => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.email.is_none()
            && self.signing_key.is_none()
            && self.extra.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactConfig {
    pub root: PathBuf,
    pub namespace: Option<String>,
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
    pub fn git_base_branch(&self) -> &str {
        &self.git.base_branch
    }

    pub fn from_workflow(
        workflow: &WorkflowDefinition,
        workflow_path: &Path,
    ) -> Result<Self, ConfigError> {
        let workflow_dir = workflow_path.parent().unwrap_or_else(|| Path::new("."));
        let root = workflow.config.as_object().cloned().unwrap_or_default();

        let tracker = parse_tracker(root.get("tracker"), workflow_dir);
        let git = parse_git(root.get("git"));
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
        let worker = parse_worker(root.get("worker"));
        let hooks = HooksConfig {
            after_create: get_string(root.get("hooks"), "after_create"),
            before_run: get_string(root.get("hooks"), "before_run"),
            after_run: get_string(root.get("hooks"), "after_run"),
            before_remove: get_string(root.get("hooks"), "before_remove"),
            timeout_ms: get_u64(root.get("hooks"), "timeout_ms").unwrap_or(60_000),
        };
        let main_lane_config = root.get("main_lane").or_else(|| root.get("agent"));
        let review_lane_config = root.get("review_lane").or_else(|| root.get("review"));
        let merge_lane_config = root.get("merge_lane");

        let agent = AgentConfig {
            max_concurrent_agents: get_u64(main_lane_config, "max_concurrent_agents").unwrap_or(10)
                as usize,
            max_turns: get_u64(main_lane_config, "max_turns").unwrap_or(20) as u32,
            max_retry_backoff_ms: get_u64(main_lane_config, "max_retry_backoff_ms")
                .unwrap_or(300_000),
            max_concurrent_agents_by_state: parse_state_limits(get_value(
                main_lane_config,
                "max_concurrent_agents_by_state",
            )),
            backend: get_string(main_lane_config, "backend")
                .or_else(|| get_string(root.get("backend"), "kind"))
                .unwrap_or_else(|| "dry-run".to_string()),
        };
        let backend = BackendConfig {
            kind: agent.backend.clone(),
        };
        let codex = CodexConfig {
            command: get_string(root.get("codex"), "command")
                .unwrap_or_else(default_codex_app_server_command),
            model: get_string(root.get("codex"), "model"),
            reasoning_effort: get_string(root.get("codex"), "reasoning_effort")
                .unwrap_or_else(|| "high".to_string()),
            approval_policy: get_value(root.get("codex"), "approval_policy")
                .cloned()
                .unwrap_or_else(default_codex_approval_policy),
            thread_sandbox: get_string(root.get("codex"), "thread_sandbox")
                .unwrap_or_else(|| "workspace-write".to_string()),
            turn_sandbox_policy: get_value(root.get("codex"), "turn_sandbox_policy").cloned(),
            turn_timeout_ms: get_u64(root.get("codex"), "turn_timeout_ms").unwrap_or(3_600_000),
            read_timeout_ms: get_u64(root.get("codex"), "read_timeout_ms").unwrap_or(5_000),
            stall_timeout_ms: get_u64(root.get("codex"), "stall_timeout_ms").unwrap_or(300_000),
            session_stale_after_ms: get_u64(root.get("codex"), "session_stale_after_ms")
                .unwrap_or(30 * 60 * 1000),
        };
        let claude = ClaudeConfig {
            command: get_string(root.get("claude"), "command")
                .unwrap_or_else(|| "claude".to_string()),
            turn_timeout_ms: get_u64(root.get("claude"), "turn_timeout_ms").unwrap_or(3_600_000),
        };
        let tmux = TmuxConfig {
            command: resolve_command_token(get_string(root.get("tmux"), "command"), "tmux"),
            agent_command: get_string(root.get("tmux"), "agent_command")
                .unwrap_or_else(|| "codex".to_string()),
            main_agent_command: resolve_optional_command_token(get_string(
                root.get("tmux"),
                "main_agent_command",
            )),
            review_agent_command: resolve_optional_command_token(get_string(
                root.get("tmux"),
                "review_agent_command",
            )),
            merge_agent_command: resolve_optional_command_token(get_string(
                root.get("tmux"),
                "merge_agent_command",
            )),
            session_prefix: get_string(root.get("tmux"), "session_prefix")
                .unwrap_or_else(|| "shea".to_string()),
        };
        let review = ReviewConfig {
            backend: get_string(review_lane_config, "backend")
                .unwrap_or_else(|| "fake".to_string()),
            gemini_command: resolve_command_token(
                get_string(review_lane_config, "gemini_command"),
                "gemini",
            ),
            gemini_model: get_string(review_lane_config, "gemini_model"),
            gemini_allowed_tools: get_string_vec(review_lane_config, "gemini_allowed_tools")
                .unwrap_or_default()
                .into_iter()
                .map(|tool| tool.trim().to_string())
                .filter(|tool| !tool.is_empty())
                .collect(),
            timeout_ms: get_u64(review_lane_config, "timeout_ms").unwrap_or(600_000),
            max_concurrent_workers: get_u64(review_lane_config, "max_concurrent_workers")
                .unwrap_or(1)
                .max(1) as usize,
        };
        let merge_lane = MergeLaneConfig {
            max_concurrent_workers: get_u64(merge_lane_config, "max_concurrent_workers")
                .unwrap_or(1)
                .max(1) as usize,
            agent_backend: get_string(merge_lane_config, "agent_backend")
                .unwrap_or_else(|| "codex".to_string()),
        };
        let quality_gate = parse_quality_gate(root.get("quality_gate"));
        let verification = parse_verification(root.get("verification"));
        let profiles = parse_profiles(root.get("profiles"), workflow_dir);
        let identity = parse_identity(root.get("identity"));
        let artifacts = parse_artifacts(root.get("artifacts"), workflow_dir);
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
            git,
            polling,
            workspace,
            worker,
            hooks,
            agent,
            backend,
            codex,
            claude,
            tmux,
            review,
            merge_lane,
            quality_gate,
            verification,
            profiles,
            identity,
            artifacts,
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
            "dry-run" | "codex" | "claude-code" | "tmux" => {}
            other => return Err(ConfigError::UnsupportedBackend(other.to_string())),
        }
        if self.backend.kind == "tmux" && self.tmux.session_prefix.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "tmux.session_prefix must not be empty".into(),
            ));
        }
        if self.backend.kind == "tmux" && self.tmux.agent_command.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "tmux.agent_command must not be empty".into(),
            ));
        }
        match self.review.backend.as_str() {
            "fake" | "gemini-cli" => {}
            other => return Err(ConfigError::UnsupportedBackend(other.to_string())),
        }
        match self.quality_gate.llm.mode.as_str() {
            "disabled" | "advisory" | "required" => {}
            other => {
                return Err(ConfigError::Invalid(format!(
                    "quality_gate.llm.mode must be disabled, advisory, or required; got {other}"
                )))
            }
        }
        if self.quality_gate.llm.mode == "required" {
            require_present(
                "quality_gate.llm.command",
                self.quality_gate.llm.command.as_deref(),
            )?;
        }

        require_positive("polling.interval_ms", self.polling.interval_ms)?;
        require_present("git.base_branch", Some(self.git.base_branch.as_str()))?;
        require_positive("hooks.timeout_ms", self.hooks.timeout_ms)?;
        if let Some(limit) = self.worker.max_concurrent_agents_per_host {
            if limit <= 0 {
                return Err(ConfigError::Invalid(
                    "worker.max_concurrent_agents_per_host must be positive".into(),
                ));
            }
        }
        require_positive(
            "main_lane.max_concurrent_agents",
            self.agent.max_concurrent_agents as u64,
        )?;
        require_positive("main_lane.max_turns", self.agent.max_turns as u64)?;
        require_positive(
            "main_lane.max_retry_backoff_ms",
            self.agent.max_retry_backoff_ms,
        )?;
        require_positive("review_lane.timeout_ms", self.review.timeout_ms)?;
        require_positive(
            "review_lane.max_concurrent_workers",
            self.review.max_concurrent_workers as u64,
        )?;
        require_positive(
            "merge_lane.max_concurrent_workers",
            self.merge_lane.max_concurrent_workers as u64,
        )?;
        require_positive(
            "quality_gate.llm.timeout_ms",
            self.quality_gate.llm.timeout_ms,
        )?;
        require_positive("verification.timeout_ms", self.verification.timeout_ms)?;

        if self.tracker.kind == "github_project_v2" {
            require_present("tracker.owner", self.tracker.owner.as_deref())?;
            require_present("tracker.repo", self.tracker.repo.as_deref())?;
            require_present(
                "tracker.project_owner",
                self.tracker.project_owner.as_deref(),
            )?;
            if let Some(owner_type) = self.tracker.project_owner_type.as_deref() {
                match owner_type.trim().to_ascii_lowercase().as_str() {
                    "user" | "organization" => {}
                    other => {
                        return Err(ConfigError::Invalid(format!(
                            "tracker.project_owner_type must be user or organization; got {other}"
                        )))
                    }
                }
            }
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

fn parse_git(value: Option<&Value>) -> GitConfig {
    let base_branch = get_string(value, "base_branch")
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| DEFAULT_GIT_BASE_BRANCH.to_string());
    GitConfig { base_branch }
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
        project_owner_type: get_string(value, "project_owner_type"),
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
        marker: value_string(value, "marker", "<!-- shea-symphony-workpad -->"),
    }
}

fn parse_worker(value: Option<&Value>) -> WorkerConfig {
    WorkerConfig {
        ssh_hosts: get_string_vec(value, "ssh_hosts")
            .unwrap_or_default()
            .into_iter()
            .map(|host| host.trim().to_string())
            .filter(|host| !host.is_empty())
            .collect(),
        max_concurrent_agents_per_host: get_i64(value, "max_concurrent_agents_per_host"),
    }
}

fn parse_profiles(value: Option<&Value>, workflow_dir: &Path) -> ProfilesConfig {
    ProfilesConfig {
        default: get_string(value, "default"),
        cockpit_tools: CockpitToolsProfilesConfig {
            codex_instances_path: get_string(
                get_value(value, "cockpit_tools"),
                "codex_instances_path",
            )
            .map(|path| resolve_path(Some(&path), workflow_dir, Path::new(""))),
        },
        entries: parse_execution_profiles(get_value(value, "entries"), workflow_dir),
    }
}

fn parse_quality_gate(value: Option<&Value>) -> QualityGateConfig {
    let llm = get_value(value, "llm");
    QualityGateConfig {
        llm: LlmQualityGateConfig {
            mode: get_string(llm, "mode").unwrap_or_else(|| "disabled".to_string()),
            command: get_string(llm, "command"),
            timeout_ms: get_u64(llm, "timeout_ms").unwrap_or(120_000),
        },
    }
}

fn parse_verification(value: Option<&Value>) -> VerificationConfig {
    VerificationConfig {
        commands: get_string_vec(value, "commands").unwrap_or_default(),
        timeout_ms: get_u64(value, "timeout_ms").unwrap_or(600_000),
    }
}

fn parse_execution_profiles(
    value: Option<&Value>,
    workflow_dir: &Path,
) -> Vec<ExecutionProfileConfig> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = get_string(Some(item), "id")?;
                    Some(ExecutionProfileConfig {
                        id,
                        instance_name: get_string(Some(item), "instance_name"),
                        backend: get_string(Some(item), "backend"),
                        workspace_namespace: get_string(Some(item), "workspace_namespace"),
                        user_data_dir: get_string(Some(item), "user_data_dir")
                            .map(|path| resolve_path(Some(&path), workflow_dir, Path::new(""))),
                        working_dir: get_string(Some(item), "working_dir")
                            .map(|path| resolve_path(Some(&path), workflow_dir, Path::new(""))),
                        extra_args: get_string(Some(item), "extra_args"),
                        env: parse_string_map(get_value(Some(item), "env")),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Some(Value::Object(values)) = value {
        for (key, value) in values {
            if let Some(value) = value.as_str() {
                map.insert(key.clone(), value.to_string());
            }
        }
    }
    map
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

fn parse_identity(value: Option<&Value>) -> IdentityConfig {
    IdentityConfig {
        actor_role: get_string(value, "actor_role")
            .unwrap_or_else(|| "implementation_agent".to_string()),
        actor_label: get_string(value, "actor_label")
            .unwrap_or_else(|| "Shea Symphony Agent".to_string()),
        git: parse_git_identity(get_value(value, "git")),
    }
}

fn parse_artifacts(value: Option<&Value>, workflow_dir: &Path) -> ArtifactConfig {
    ArtifactConfig {
        root: resolve_path(
            get_string(value, "root").as_deref(),
            workflow_dir,
            &default_artifact_root(),
        ),
        namespace: get_string(value, "namespace"),
    }
}

fn parse_git_identity(value: Option<&Value>) -> GitIdentityConfig {
    GitIdentityConfig {
        name: get_string(value, "name"),
        email: get_string(value, "email"),
        signing_key: get_string(value, "signing_key"),
        extra: parse_string_map(get_value(value, "extra")),
    }
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

fn get_i64(root: Option<&Value>, key: &str) -> Option<i64> {
    get_value(root, key).and_then(Value::as_i64)
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

fn resolve_command_token(value: Option<String>, default: &str) -> String {
    match value {
        Some(raw) if raw.starts_with('$') => env::var(raw.trim_start_matches('$'))
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or(raw),
        Some(raw) if !raw.is_empty() => raw,
        _ => default.to_string(),
    }
}

fn resolve_optional_command_token(value: Option<String>) -> Option<String> {
    match value {
        Some(raw) if raw.starts_with('$') => Some(
            env::var(raw.trim_start_matches('$'))
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or(raw),
        ),
        Some(raw) if !raw.trim().is_empty() => Some(raw),
        _ => None,
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
    if let Some(env_path) = raw.strip_prefix('$') {
        return resolve_env_path_token(env_path);
    }
    Some(PathBuf::from(raw))
}

fn resolve_env_path_token(raw: &str) -> Option<PathBuf> {
    let (env_name, suffix) = raw.split_once('/').unwrap_or((raw, ""));
    if env_name.is_empty() {
        return None;
    }

    let base = env::var(env_name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| (env_name == "SHEA_SYMPHONY_ARTIFACT_ROOT").then(default_artifact_root))?;

    let base = expand_tilde(base);
    if suffix.is_empty() {
        Some(base)
    } else {
        Some(base.join(suffix))
    }
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

fn default_artifact_root() -> PathBuf {
    env::var_os("SHEA_SYMPHONY_ARTIFACT_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(expand_tilde)
        .or_else(|| home_dir().map(|home| home.join(".shea-symphony").join("artifacts")))
        .unwrap_or_else(|| env::temp_dir().join("shea-symphony-artifacts"))
}

fn default_codex_approval_policy() -> Value {
    serde_json::Value::String("never".to_string())
}

fn default_codex_app_server_command() -> String {
    "codex app-server -c 'service_tier=\"fast\"'".to_string()
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
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.tracker.kind, "github_project_v2");
        assert_eq!(config.tracker.status_field, "Status");
        assert_eq!(
            config.tracker.workpad.marker,
            "<!-- shea-symphony-workpad -->"
        );
        assert_eq!(config.git.base_branch, DEFAULT_GIT_BASE_BRANCH);
        assert_eq!(config.tracker.project_owner_type, None);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn git_base_branch_is_workflow_configurable() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\ngit:\n  base_branch: dev-chunteng\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.git_base_branch(), "dev-chunteng");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn codex_approval_policy_defaults_to_app_server_supported_never() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: codex\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(
            config.codex.command,
            "codex app-server -c 'service_tier=\"fast\"'"
        );
        assert_eq!(config.codex.approval_policy, serde_json::json!("never"));
    }

    #[test]
    fn codex_model_and_reasoning_effort_are_configurable() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\ncodex:\n  model: gpt-5.5\n  reasoning_effort: high\n  stall_timeout_ms: 60000\n  session_stale_after_ms: 120000\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.codex.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(config.codex.reasoning_effort, "high");
        assert_eq!(config.codex.stall_timeout_ms, 60_000);
        assert_eq!(config.codex.session_stale_after_ms, 120_000);
    }

    #[test]
    fn codex_session_stale_after_defaults_to_thirty_minutes() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.codex.session_stale_after_ms, 30 * 60 * 1000);
    }

    #[test]
    fn accepts_explicit_github_project_owner_type() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_owner_type: user\n  project_number: 1\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.tracker.project_owner_type.as_deref(), Some("user"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_unknown_github_project_owner_type() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: shea-symphony\n  project_owner: Alive24\n  project_owner_type: team\n  project_number: 1\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert!(config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("tracker.project_owner_type must be user or organization"));
    }

    #[test]
    fn normalizes_state_limits() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nmain_lane:\n  max_concurrent_agents_by_state:\n    In Progress: 2\n    bad: 0\n---\nPrompt",
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
    fn parses_tmux_backend_config() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmain_lane:\n  backend: tmux\ntmux:\n  command: /opt/homebrew/bin/tmux\n  agent_command: codex\n  review_agent_command: gemini\n  merge_agent_command: codex\n  session_prefix: shea-local\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.backend.kind, "tmux");
        assert_eq!(config.tmux.command, "/opt/homebrew/bin/tmux");
        assert_eq!(config.tmux.agent_command, "codex");
        assert_eq!(config.tmux.main_agent_command, None);
        assert_eq!(config.tmux.review_agent_command.as_deref(), Some("gemini"));
        assert_eq!(config.tmux.merge_agent_command.as_deref(), Some("codex"));
        assert_eq!(config.tmux.session_prefix, "shea-local");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn merge_lane_agent_backend_defaults_to_codex() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.merge_lane.agent_backend, "codex");
    }

    #[test]
    fn parses_merge_lane_agent_backend_override() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nmerge_lane:\n  agent_backend: tmux\n  max_concurrent_workers: 2\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.merge_lane.agent_backend, "tmux");
        assert_eq!(config.merge_lane.max_concurrent_workers, 2);
    }

    #[test]
    fn parses_artifact_root_and_namespace() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nartifacts:\n  root: artifacts\n  namespace: custom/project\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.artifacts.root, PathBuf::from("/tmp/artifacts"));
        assert_eq!(
            config.artifacts.namespace.as_deref(),
            Some("custom/project")
        );
    }

    #[test]
    fn expands_environment_path_prefixes_with_suffixes() {
        let previous = std::env::var_os("SHEA_TEST_ARTIFACT_ROOT");
        std::env::set_var("SHEA_TEST_ARTIFACT_ROOT", "/tmp/shea-artifacts");
        let workflow = WorkflowDefinition::parse(
            "/tmp/config/WORKFLOW.md",
            "---\nartifacts:\n  root: $SHEA_TEST_ARTIFACT_ROOT\nworkspace:\n  root: $SHEA_TEST_ARTIFACT_ROOT/worktrees\nobservability:\n  logs_root: $SHEA_TEST_ARTIFACT_ROOT/logs\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/config/WORKFLOW.md")).unwrap();
        match previous {
            Some(value) => std::env::set_var("SHEA_TEST_ARTIFACT_ROOT", value),
            None => std::env::remove_var("SHEA_TEST_ARTIFACT_ROOT"),
        }

        assert_eq!(config.artifacts.root, PathBuf::from("/tmp/shea-artifacts"));
        assert_eq!(
            config.workspace.root,
            PathBuf::from("/tmp/shea-artifacts/worktrees")
        );
        assert_eq!(
            config.observability.logs_root,
            PathBuf::from("/tmp/shea-artifacts/logs")
        );
    }

    #[test]
    fn shea_artifact_env_token_falls_back_to_default_root() {
        let previous = std::env::var_os("SHEA_SYMPHONY_ARTIFACT_ROOT");
        std::env::remove_var("SHEA_SYMPHONY_ARTIFACT_ROOT");
        let workflow = WorkflowDefinition::parse(
            "/tmp/config/WORKFLOW.md",
            "---\nworkspace:\n  root: $SHEA_SYMPHONY_ARTIFACT_ROOT/worktrees\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/config/WORKFLOW.md")).unwrap();
        let expected = default_artifact_root().join("worktrees");
        match previous {
            Some(value) => std::env::set_var("SHEA_SYMPHONY_ARTIFACT_ROOT", value),
            None => std::env::remove_var("SHEA_SYMPHONY_ARTIFACT_ROOT"),
        }

        assert_eq!(config.workspace.root, expected);
    }

    #[test]
    fn parses_actor_and_git_identity_config() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nidentity:\n  actor_role: review_agent\n  actor_label: Gemini Review Runner\n  git:\n    name: Shea Symphony Review Bot\n    email: shea-review@example.invalid\n    signing_key: ABC123\n    extra:\n      shea.actorRole: review_agent\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.identity.actor_role, "review_agent");
        assert_eq!(config.identity.actor_label, "Gemini Review Runner");
        assert_eq!(
            config.identity.git.author().as_deref(),
            Some("Shea Symphony Review Bot <shea-review@example.invalid>")
        );
        assert_eq!(
            config
                .identity
                .git
                .extra
                .get("shea.actorRole")
                .map(String::as_str),
            Some("review_agent")
        );
    }

    #[test]
    fn review_gemini_command_can_use_environment_token() {
        std::env::set_var("SHEA_TEST_GEMINI_COMMAND", "/opt/homebrew/bin/gemini-test");
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nreview_lane:\n  backend: gemini-cli\n  gemini_command: $SHEA_TEST_GEMINI_COMMAND\n  gemini_model: gemini-3.1-pro-preview\n  gemini_allowed_tools:\n    - run_shell_command\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();
        std::env::remove_var("SHEA_TEST_GEMINI_COMMAND");

        assert_eq!(
            config.review.gemini_command,
            "/opt/homebrew/bin/gemini-test"
        );
        assert_eq!(
            config.review.gemini_model.as_deref(),
            Some("gemini-3.1-pro-preview")
        );
        assert_eq!(
            config.review.gemini_allowed_tools,
            vec!["run_shell_command".to_string()]
        );
    }

    #[test]
    fn linear_defaults_endpoint_and_allows_fixture_without_token() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: linear\n  project_slug: shea-symphony\n  fixture_path: issues.json\n---\nPrompt",
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

    #[test]
    fn parses_execution_profiles_from_workflow_config() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/config/WORKFLOW.md",
            "---\nprofiles:\n  default: codex-alpha\n  cockpit_tools:\n    codex_instances_path: fixtures/cockpit-tools-codex-instances.json\n  entries:\n    - id: fallback\n      instance_name: Fallback Worker\n      backend: dry-run\n      workspace_namespace: fallback-worker\n      user_data_dir: ./profiles/fallback\n      env:\n        SHEA_TEST_PROFILE: fallback\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/config/WORKFLOW.md")).unwrap();

        assert_eq!(config.profiles.default.as_deref(), Some("codex-alpha"));
        assert_eq!(
            config
                .profiles
                .cockpit_tools
                .codex_instances_path
                .as_deref(),
            Some(Path::new(
                "/tmp/config/fixtures/cockpit-tools-codex-instances.json"
            ))
        );
        assert_eq!(config.profiles.entries.len(), 1);
        assert_eq!(
            config.profiles.entries[0].env.get("SHEA_TEST_PROFILE"),
            Some(&"fallback".into())
        );
        assert_eq!(
            config.profiles.entries[0].user_data_dir.as_deref(),
            Some(Path::new("/tmp/config/profiles/fallback"))
        );
    }

    #[test]
    fn parses_optional_ssh_worker_config_without_enabling_remote_execution() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nworker:\n  ssh_hosts:\n    - worker-a.example\n    - ' worker-b.example '\n    - ''\n  max_concurrent_agents_per_host: 2\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(
            config.worker.ssh_hosts,
            vec!["worker-a.example", "worker-b.example"]
        );
        assert_eq!(config.worker.max_concurrent_agents_per_host, Some(2));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_non_positive_ssh_worker_host_limit() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nworker:\n  ssh_hosts:\n    - worker-a.example\n  max_concurrent_agents_per_host: 0\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("worker.max_concurrent_agents_per_host must be positive"));
    }

    #[test]
    fn parses_llm_quality_gate_config() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nquality_gate:\n  llm:\n    mode: required\n    command: sh examples/fixtures/llm-gate-ready.sh\n    timeout_ms: 5000\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.quality_gate.llm.mode, "required");
        assert_eq!(
            config.quality_gate.llm.command.as_deref(),
            Some("sh examples/fixtures/llm-gate-ready.sh")
        );
        assert_eq!(config.quality_gate.llm.timeout_ms, 5_000);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parses_handoff_verification_config() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nverification:\n  timeout_ms: 15000\n  commands:\n    - cargo test\n    - cargo fmt --check\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert_eq!(config.verification.timeout_ms, 15_000);
        assert_eq!(
            config.verification.commands,
            vec!["cargo test".to_string(), "cargo fmt --check".to_string()]
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn required_llm_quality_gate_requires_command() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nquality_gate:\n  llm:\n    mode: required\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap();

        assert!(config.validate().is_err());
    }
}
