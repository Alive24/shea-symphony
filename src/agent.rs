use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io::Write};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::model::AgentEvent;
use crate::profiles::{selected_execution_profile, ExecutionProfile};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedRun {
    pub backend: String,
    pub workspace: PathBuf,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_artifact_path: Option<PathBuf>,
    pub command: Option<String>,
    pub timeout_ms: u64,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub profile_id: Option<String>,
    pub instance_name: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub actor_role: Option<String>,
    pub actor_label: Option<String>,
    pub git_author: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSummary {
    pub backend: String,
    pub success: bool,
    #[serde(default)]
    pub pending_session: bool,
    pub session_id: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLimitPause {
    pub classifier: String,
    pub evidence: String,
}

pub trait AgentBackend {
    fn name(&self) -> &'static str;
    fn prepare(
        &self,
        workspace: PathBuf,
        rendered_prompt: String,
        config: &RuntimeConfig,
    ) -> Result<PreparedRun, AgentError>;
    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError>;
    fn stop(&self, reason: &str) -> Result<(), AgentError>;
    fn summarize(&self, events: &[AgentEvent]) -> AgentSummary;
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent backend unavailable: {0}")]
    Unavailable(String),
    #[error("agent io failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Default)]
pub struct DryRunBackend;

impl AgentBackend for DryRunBackend {
    fn name(&self) -> &'static str {
        "dry-run"
    }

    fn prepare(
        &self,
        workspace: PathBuf,
        rendered_prompt: String,
        config: &RuntimeConfig,
    ) -> Result<PreparedRun, AgentError> {
        let profile = selected_execution_profile(&config.profiles)
            .map_err(|error| AgentError::Unavailable(error.to_string()))?;
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            prompt_artifact_path: None,
            command: None,
            timeout_ms: 0,
            approval_policy: None,
            sandbox: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env: profile_environment(profile.as_ref(), self.name()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        let session_id = session_id_with_profile("dry-run-session", &prepared);
        Ok(vec![
            AgentEvent::SessionStarted {
                backend: prepared.backend.clone(),
                session_id: session_id.clone(),
            },
            AgentEvent::Completed {
                backend: prepared.backend,
                session_id: Some(session_id),
                summary: "Dry-run backend did not execute external agent commands.".into(),
            },
        ])
    }

    fn stop(&self, _reason: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn summarize(&self, events: &[AgentEvent]) -> AgentSummary {
        let success = events
            .iter()
            .any(|event| matches!(event, AgentEvent::Completed { .. }));
        AgentSummary {
            backend: self.name().into(),
            success,
            pending_session: false,
            session_id: events.iter().find_map(|event| match event {
                AgentEvent::SessionStarted { session_id, .. } => Some(session_id.clone()),
                _ => None,
            }),
            message: "dry-run complete".into(),
            log_path: None,
            attach_command: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct CodexBackend;

impl AgentBackend for CodexBackend {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn prepare(
        &self,
        workspace: PathBuf,
        rendered_prompt: String,
        config: &RuntimeConfig,
    ) -> Result<PreparedRun, AgentError> {
        let profile = selected_execution_profile(&config.profiles)
            .map_err(|error| AgentError::Unavailable(error.to_string()))?;
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            prompt_artifact_path: None,
            command: Some(config.codex.command.clone()),
            timeout_ms: config.codex.turn_timeout_ms,
            approval_policy: Some(config.codex.approval_policy.to_string()),
            sandbox: Some(config.codex.thread_sandbox.clone()),
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env: profile_environment(profile.as_ref(), self.name()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        if prepared
            .command
            .as_deref()
            .is_some_and(is_codex_app_server_command)
        {
            let session_id = session_id_with_profile("codex-subprocess", &prepared);
            return Ok(vec![
                AgentEvent::SessionStarted {
                    backend: prepared.backend.clone(),
                    session_id,
                },
                AgentEvent::Failed {
                    backend: prepared.backend,
                    error: "Codex app-server transport is not implemented for the subprocess backend; configure an explicit subprocess command or use dry-run until app-server support lands.".into(),
                },
            ]);
        }

        run_subprocess_backend(prepared, "codex-subprocess", "Codex subprocess")
    }

    fn stop(&self, _reason: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn summarize(&self, events: &[AgentEvent]) -> AgentSummary {
        summarize_events(self.name(), events)
    }
}

#[derive(Debug, Default)]
pub struct ClaudeCodeBackend;

impl AgentBackend for ClaudeCodeBackend {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn prepare(
        &self,
        workspace: PathBuf,
        rendered_prompt: String,
        config: &RuntimeConfig,
    ) -> Result<PreparedRun, AgentError> {
        let profile = selected_execution_profile(&config.profiles)
            .map_err(|error| AgentError::Unavailable(error.to_string()))?;
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            prompt_artifact_path: None,
            command: Some(config.claude.command.clone()),
            timeout_ms: config.claude.turn_timeout_ms,
            approval_policy: None,
            sandbox: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env: profile_environment(profile.as_ref(), self.name()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        run_subprocess_backend(prepared, "claude-code-subprocess", "Claude Code subprocess")
    }

    fn stop(&self, _reason: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn summarize(&self, events: &[AgentEvent]) -> AgentSummary {
        summarize_events(self.name(), events)
    }
}

#[derive(Debug, Default)]
pub struct TmuxBackend;

impl AgentBackend for TmuxBackend {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn prepare(
        &self,
        workspace: PathBuf,
        rendered_prompt: String,
        config: &RuntimeConfig,
    ) -> Result<PreparedRun, AgentError> {
        let profile = selected_execution_profile(&config.profiles)
            .map_err(|error| AgentError::Unavailable(error.to_string()))?;
        let mut env = profile_environment(profile.as_ref(), self.name());
        env.insert(
            "JADE_SYMPHONY_TMUX_COMMAND".into(),
            config.tmux.command.clone(),
        );
        env.insert(
            "JADE_SYMPHONY_TMUX_SESSION_PREFIX".into(),
            config.tmux.session_prefix.clone(),
        );
        Ok(PreparedRun {
            backend: self.name().into(),
            workspace,
            prompt: rendered_prompt,
            prompt_artifact_path: None,
            command: Some(config.tmux.agent_command.clone()),
            timeout_ms: 0,
            approval_policy: None,
            sandbox: None,
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            env,
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
        })
    }

    fn run(&self, prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
        run_tmux_backend(prepared)
    }

    fn stop(&self, _reason: &str) -> Result<(), AgentError> {
        Ok(())
    }

    fn summarize(&self, events: &[AgentEvent]) -> AgentSummary {
        let session_id = events.iter().find_map(|event| match event {
            AgentEvent::SessionStarted { session_id, .. } => Some(session_id.clone()),
            _ => None,
        });
        let failure = events.iter().find_map(|event| match event {
            AgentEvent::Failed { error, .. } => Some(error.clone()),
            _ => None,
        });
        let log_path = message_field(events, "log_path=").map(PathBuf::from);
        let attach_command = session_id
            .as_ref()
            .map(|session| format!("tmux attach-session -t {session}"));
        let message = failure.clone().unwrap_or_else(|| {
            format!(
                "tmux session running; attach with `{}`",
                attach_command.as_deref().unwrap_or("tmux attach-session")
            )
        });

        AgentSummary {
            backend: self.name().into(),
            success: false,
            pending_session: failure.is_none() && session_id.is_some(),
            session_id,
            message,
            log_path,
            attach_command,
        }
    }
}

fn run_subprocess_backend(
    prepared: PreparedRun,
    session_id: &str,
    completion_subject: &str,
) -> Result<Vec<AgentEvent>, AgentError> {
    let session_id = session_id_with_profile(session_id, &prepared);
    let mut events = vec![AgentEvent::SessionStarted {
        backend: prepared.backend.clone(),
        session_id: session_id.clone(),
    }];

    if !prepared.workspace.is_dir() {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: format!("workspace does not exist: {}", prepared.workspace.display()),
        });
        return Ok(events);
    }

    let Some(command) = prepared.command.as_deref() else {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: "missing backend command".into(),
        });
        return Ok(events);
    };

    let prompt_artifact_path = persist_prompt_artifact(&prepared)?;
    let mut child = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(&prepared.workspace)
        .env("JADE_SYMPHONY_PROMPT_PATH", &prompt_artifact_path)
        .env(
            "JADE_SYMPHONY_APPROVAL_POLICY",
            prepared.approval_policy.as_deref().unwrap_or_default(),
        )
        .env(
            "JADE_SYMPHONY_SANDBOX",
            prepared.sandbox.as_deref().unwrap_or_default(),
        )
        .envs(prepared.env.iter())
        .env(
            "JADE_SYMPHONY_ACTOR_ROLE",
            prepared.actor_role.as_deref().unwrap_or_default(),
        )
        .env(
            "JADE_SYMPHONY_ACTOR_LABEL",
            prepared.actor_label.as_deref().unwrap_or_default(),
        )
        .env(
            "JADE_SYMPHONY_GIT_AUTHOR",
            prepared.git_author.as_deref().unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prepared.prompt.as_bytes())?;
    }

    let started = Instant::now();
    let timeout = Duration::from_millis(prepared.timeout_ms.max(1));
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if !stdout.trim().is_empty() {
                events.push(AgentEvent::Message {
                    backend: prepared.backend.clone(),
                    session_id: Some(session_id.clone()),
                    text: stdout,
                });
            }
            if !stderr.trim().is_empty() {
                events.push(AgentEvent::Message {
                    backend: prepared.backend.clone(),
                    session_id: Some(session_id.clone()),
                    text: stderr,
                });
            }
            if output.status.success() {
                events.push(AgentEvent::Completed {
                    backend: prepared.backend,
                    session_id: Some(session_id),
                    summary: format!("{completion_subject} completed successfully."),
                });
            } else {
                events.push(AgentEvent::Failed {
                    backend: prepared.backend,
                    error: format!(
                        "{completion_subject} exited with status {}",
                        output.status.code().unwrap_or(-1)
                    ),
                });
            }
            return Ok(events);
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            events.push(AgentEvent::Failed {
                backend: prepared.backend,
                error: format!(
                    "{completion_subject} timed out after {}ms",
                    prepared.timeout_ms
                ),
            });
            return Ok(events);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn run_tmux_backend(prepared: PreparedRun) -> Result<Vec<AgentEvent>, AgentError> {
    let mut events = Vec::new();

    if !prepared.workspace.is_dir() {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: format!("workspace does not exist: {}", prepared.workspace.display()),
        });
        return Ok(events);
    }

    let Some(agent_command) = prepared.command.as_deref() else {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error: "missing tmux agent command".into(),
        });
        return Ok(events);
    };

    let prompt_artifact_path = persist_prompt_artifact(&prepared)?;
    let session_id = tmux_session_name(&prepared);
    let log_path = tmux_log_path(&prepared, &session_id);
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmux = prepared
        .env
        .get("JADE_SYMPHONY_TMUX_COMMAND")
        .cloned()
        .or_else(|| std::env::var("JADE_SYMPHONY_TMUX_COMMAND").ok())
        .unwrap_or_else(|| "tmux".into());
    let target = session_id.as_str();
    let shell_command = tmux_agent_shell_command(&prepared, agent_command, &prompt_artifact_path);
    if let Err(error) = tmux_command_status(
        Command::new(&tmux)
            .envs(prepared.env.iter())
            .args(["new-session", "-d", "-s", target, "-c"])
            .arg(&prepared.workspace)
            .arg(shell_command),
        "new-session",
    ) {
        events.push(AgentEvent::Failed {
            backend: prepared.backend,
            error,
        });
        return Ok(events);
    }
    for (action, mut command) in [
        ("pipe-pane", {
            let mut command = Command::new(&tmux);
            command
                .envs(prepared.env.iter())
                .args(["pipe-pane", "-o", "-t", target])
                .arg(format!("cat >> {}", shell_quote_path(&log_path)));
            command
        }),
        ("load-buffer", {
            let mut command = Command::new(&tmux);
            command
                .envs(prepared.env.iter())
                .args(["load-buffer", "-b", target])
                .arg(&prompt_artifact_path);
            command
        }),
        ("paste-buffer", {
            let mut command = Command::new(&tmux);
            command
                .envs(prepared.env.iter())
                .args(["paste-buffer", "-b", target, "-t", target]);
            command
        }),
        ("send-keys", {
            let mut command = Command::new(&tmux);
            command
                .envs(prepared.env.iter())
                .args(["send-keys", "-t", target, "Enter"]);
            command
        }),
    ] {
        if let Err(error) = tmux_command_status(&mut command, action) {
            events.push(AgentEvent::Failed {
                backend: prepared.backend,
                error,
            });
            return Ok(events);
        }
    }

    events.push(AgentEvent::SessionStarted {
        backend: prepared.backend.clone(),
        session_id: session_id.clone(),
    });
    events.push(AgentEvent::Message {
        backend: prepared.backend,
        session_id: Some(session_id.clone()),
        text: format!(
            "tmux_session_started session={} attach_command=\"tmux attach-session -t {}\" log_path={} prompt_artifact={}",
            session_id,
            session_id,
            log_path.display(),
            prompt_artifact_path.display()
        ),
    });
    Ok(events)
}

fn tmux_command_status(command: &mut Command, action: &str) -> Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "tmux {action} exited with status {}",
            status.code().unwrap_or(-1)
        )),
        Err(error) => Err(format!("tmux {action} failed: {error}")),
    }
}

fn tmux_agent_shell_command(
    prepared: &PreparedRun,
    agent_command: &str,
    prompt_artifact_path: &Path,
) -> String {
    format!(
        "JADE_SYMPHONY_PROMPT_PATH={} JADE_SYMPHONY_ACTOR_ROLE={} JADE_SYMPHONY_ACTOR_LABEL={} JADE_SYMPHONY_GIT_AUTHOR={} sh -lc {}",
        shell_quote_str(&prompt_artifact_path.display().to_string()),
        shell_quote_str(prepared.actor_role.as_deref().unwrap_or_default()),
        shell_quote_str(prepared.actor_label.as_deref().unwrap_or_default()),
        shell_quote_str(prepared.git_author.as_deref().unwrap_or_default()),
        shell_quote_str(agent_command)
    )
}

fn tmux_session_name(prepared: &PreparedRun) -> String {
    let prefix = prepared
        .env
        .get("JADE_SYMPHONY_TMUX_SESSION_PREFIX")
        .map(String::as_str)
        .unwrap_or("jade");
    format!(
        "{}-main-{}-{}",
        safe_path_component(Some(prefix)),
        safe_path_component(
            prepared
                .workspace
                .file_name()
                .and_then(|name| name.to_str())
        ),
        current_time_ms()
    )
}

fn tmux_log_path(prepared: &PreparedRun, session_id: &str) -> PathBuf {
    prepared
        .prompt_artifact_path
        .as_ref()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .unwrap_or_else(|| Path::new("/tmp"))
        .join("tmux")
        .join(format!("{}.log", safe_path_component(Some(session_id))))
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote_str(&path.display().to_string())
}

fn shell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn message_field(events: &[AgentEvent], prefix: &str) -> Option<String> {
    events.iter().find_map(|event| {
        let AgentEvent::Message { text, .. } = event else {
            return None;
        };
        text.split_whitespace()
            .find_map(|part| part.strip_prefix(prefix).map(str::to_string))
    })
}

pub fn persist_prompt_artifact(prepared: &PreparedRun) -> Result<PathBuf, AgentError> {
    let path = prepared
        .prompt_artifact_path
        .clone()
        .unwrap_or_else(|| fallback_prompt_artifact_path(prepared));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, &prepared.prompt)?;
    Ok(path)
}

fn fallback_prompt_artifact_path(prepared: &PreparedRun) -> PathBuf {
    std::env::temp_dir()
        .join("jade-symphony")
        .join("prompts")
        .join(format!(
            "{}-{}-{}.prompt.md",
            safe_path_component(
                prepared
                    .workspace
                    .file_name()
                    .and_then(|name| name.to_str())
            ),
            safe_path_component(Some(&prepared.backend)),
            current_time_ms()
        ))
}

fn safe_path_component(value: Option<&str>) -> String {
    let safe = value
        .unwrap_or("run")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if safe.is_empty() {
        "run".into()
    } else {
        safe
    }
}

fn current_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn profile_environment(
    profile: Option<&ExecutionProfile>,
    backend: &str,
) -> BTreeMap<String, String> {
    profile
        .map(|profile| profile.environment_for_backend(backend))
        .unwrap_or_default()
}

fn session_id_with_profile(base: &str, prepared: &PreparedRun) -> String {
    prepared
        .profile_id
        .as_deref()
        .map(|profile| format!("{base}:{profile}"))
        .unwrap_or_else(|| base.into())
}

fn is_codex_app_server_command(command: &str) -> bool {
    let tokens = command
        .split_whitespace()
        .map(clean_shell_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;

    while let Some(token) = tokens.get(index) {
        if matches!(*token, "env" | "exec" | "command") || is_env_assignment(token) {
            index += 1;
        } else {
            break;
        }
    }

    let Some(executable) = tokens.get(index) else {
        return false;
    };
    let Some(first_arg) = tokens.get(index + 1) else {
        return false;
    };

    is_codex_executable(executable) && *first_arg == "app-server"
}

fn clean_shell_token(token: &str) -> &str {
    token.trim_matches(|character| matches!(character, '\'' | '"' | ';'))
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_codex_executable(token: &str) -> bool {
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "codex")
}

fn summarize_events(backend: &str, events: &[AgentEvent]) -> AgentSummary {
    let session_id = events.iter().find_map(|event| match event {
        AgentEvent::SessionStarted { session_id, .. } => Some(session_id.clone()),
        _ => None,
    });
    let failure = events.iter().find_map(|event| match event {
        AgentEvent::Failed { error, .. } => Some(error.clone()),
        _ => None,
    });
    let completed = events.iter().find_map(|event| match event {
        AgentEvent::Completed { summary, .. } => Some(summary.clone()),
        _ => None,
    });

    AgentSummary {
        backend: backend.into(),
        success: failure.is_none() && completed.is_some(),
        pending_session: false,
        session_id,
        message: failure
            .or(completed)
            .unwrap_or_else(|| "no terminal event".into()),
        log_path: None,
        attach_command: None,
    }
}

pub fn usage_limit_pause_from_events(events: &[AgentEvent]) -> Option<UsageLimitPause> {
    events.iter().find_map(|event| match event {
        AgentEvent::Message { text, .. } | AgentEvent::Failed { error: text, .. } => {
            classify_usage_limit_text(text)
        }
        _ => None,
    })
}

pub fn classify_usage_limit_text(text: &str) -> Option<UsageLimitPause> {
    let normalized = text.to_ascii_lowercase();
    let patterns = [
        ("usage_limit", "usage limit"),
        ("rate_limit", "rate limit"),
        ("rate_limit", "rate limited"),
        ("resource_exhausted", "resource exhausted"),
        ("quota_exceeded", "quota exceeded"),
        ("quota_exceeded", "insufficient quota"),
        ("too_many_requests", "too many requests"),
        ("http_429", "429"),
    ];

    patterns
        .iter()
        .find(|(_, pattern)| normalized.contains(pattern))
        .map(|(classifier, _)| UsageLimitPause {
            classifier: (*classifier).into(),
            evidence: compact_evidence(text),
        })
}

fn compact_evidence(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_LEN: usize = 240;
    if compact.len() > MAX_LEN {
        format!("{}...", &compact[..MAX_LEN])
    } else {
        compact
    }
}

pub fn backend_from_config(config: &RuntimeConfig) -> Box<dyn AgentBackend> {
    match config.backend.kind.as_str() {
        "codex" => Box::<CodexBackend>::default(),
        "claude-code" => Box::<ClaudeCodeBackend>::default(),
        "tmux" => Box::<TmuxBackend>::default(),
        _ => Box::<DryRunBackend>::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;
    use crate::workflow::WorkflowDefinition;

    #[test]
    fn dry_run_backend_emits_normalized_events() {
        let workflow = WorkflowDefinition::parse("/tmp/WORKFLOW.md", "---\n---\nPrompt").unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = DryRunBackend;
        let prepared = backend
            .prepare(PathBuf::from("/tmp/ws"), "prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        assert!(matches!(events[0], AgentEvent::SessionStarted { .. }));
    }

    #[test]
    fn tmux_backend_prepare_uses_local_session_config() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nagent:\n  backend: tmux\ntmux:\n  command: /usr/local/bin/tmux\n  agent_command: codex\n  session_prefix: jade-test\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;

        let prepared = backend
            .prepare(PathBuf::from("/tmp/ws"), "prompt".into(), &config)
            .unwrap();

        assert_eq!(prepared.backend, "tmux");
        assert_eq!(prepared.command.as_deref(), Some("codex"));
        assert_eq!(
            prepared
                .env
                .get("JADE_SYMPHONY_TMUX_COMMAND")
                .map(String::as_str),
            Some("/usr/local/bin/tmux")
        );
        assert_eq!(
            prepared
                .env
                .get("JADE_SYMPHONY_TMUX_SESSION_PREFIX")
                .map(String::as_str),
            Some("jade-test")
        );
    }

    #[test]
    fn tmux_summary_reports_pending_attachable_session() {
        let events = vec![
            AgentEvent::SessionStarted {
                backend: "tmux".into(),
                session_id: "jade-main-220".into(),
            },
            AgentEvent::Message {
                backend: "tmux".into(),
                session_id: Some("jade-main-220".into()),
                text: "tmux_session_started session=jade-main-220 log_path=/tmp/jade.log".into(),
            },
        ];

        let summary = TmuxBackend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.pending_session);
        assert_eq!(
            summary.attach_command.as_deref(),
            Some("tmux attach-session -t jade-main-220")
        );
        assert_eq!(summary.log_path, Some(PathBuf::from("/tmp/jade.log")));
    }

    #[test]
    fn tmux_backend_launches_attachable_session_when_tmux_available() {
        if Command::new("tmux")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping tmux smoke: tmux is unavailable");
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nagent:\n  backend: tmux\ntmux:\n  command: tmux\n  agent_command: cat > jade-prompt.txt\n  session_prefix: jade-test\n---\nPrompt",
        )
        .unwrap();
        let config =
            RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md"))
                .unwrap();
        let backend = TmuxBackend;
        let mut prepared = backend
            .prepare(workspace.clone(), "echo tmux-smoke".into(), &config)
            .unwrap();
        prepared.prompt_artifact_path = Some(temp.path().join("logs/prompts/smoke.prompt.md"));
        let tmux_tmp = temp.path().join("tmux-tmp");
        fs::create_dir_all(&tmux_tmp).unwrap();
        let probe_session = format!("jade-test-probe-{}", current_time_ms());
        let probe = Command::new("tmux")
            .env("TMUX_TMPDIR", &tmux_tmp)
            .args(["new-session", "-d", "-s", &probe_session, "sleep 30"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let probe_available = probe.is_ok_and(|status| status.success())
            && Command::new("tmux")
                .env("TMUX_TMPDIR", &tmux_tmp)
                .args(["has-session", "-t", &probe_session])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
        if !probe_available {
            eprintln!("skipping tmux smoke: tmux cannot create sessions in this sandbox");
            return;
        }
        Command::new("tmux")
            .env("TMUX_TMPDIR", &tmux_tmp)
            .args(["kill-session", "-t", &probe_session])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();
        prepared
            .env
            .insert("TMUX_TMPDIR".into(), tmux_tmp.display().to_string());

        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.pending_session);
        assert!(summary
            .attach_command
            .as_deref()
            .unwrap()
            .contains("tmux attach-session -t jade-test-main-workspace"));
        assert!(summary
            .log_path
            .as_ref()
            .unwrap()
            .display()
            .to_string()
            .ends_with(".log"));
        if let Some(session) = summary.session_id.as_deref() {
            Command::new("tmux")
                .env("TMUX_TMPDIR", tmux_tmp)
                .args(["kill-session", "-t", session])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok();
        }
    }

    #[test]
    fn classifies_usage_limit_text_conservatively() {
        let pause = classify_usage_limit_text("Error: usage limit reached for this model").unwrap();
        assert_eq!(pause.classifier, "usage_limit");
        assert!(pause.evidence.contains("usage limit"));

        let pause = classify_usage_limit_text("HTTP 429: too many requests").unwrap();
        assert_eq!(pause.classifier, "too_many_requests");

        assert!(classify_usage_limit_text("syntax error in generated patch").is_none());
    }

    #[test]
    fn detects_usage_limit_from_backend_events() {
        let events = vec![
            AgentEvent::SessionStarted {
                backend: "codex".into(),
                session_id: "s1".into(),
            },
            AgentEvent::Message {
                backend: "codex".into(),
                session_id: Some("s1".into()),
                text: "Resource exhausted, please retry later.".into(),
            },
            AgentEvent::Failed {
                backend: "codex".into(),
                error: "Codex subprocess exited with status 1".into(),
            },
        ];

        let pause = usage_limit_pause_from_events(&events).unwrap();
        assert_eq!(pause.classifier, "resource_exhausted");
    }

    fn codex_config(command: &str, timeout_ms: u64) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\nagent:\n  backend: codex\ncodex:\n  command: {command:?}\n  turn_timeout_ms: {timeout_ms}\n---\nPrompt"
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn claude_config(command: &str, timeout_ms: u64) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\nagent:\n  backend: claude-code\nclaude:\n  command: {command:?}\n  turn_timeout_ms: {timeout_ms}\n---\nPrompt"
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn codex_config_with_profile(command: &str) -> RuntimeConfig {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/fixtures/cockpit-tools-codex-instances.json");
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\nagent:\n  backend: codex\ncodex:\n  command: {command:?}\nprofiles:\n  default: codex-alpha\n  cockpit_tools:\n    codex_instances_path: {:?}\n---\nPrompt",
                fixture_path.display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    #[test]
    fn codex_backend_runs_subprocess_in_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let config = codex_config("cat > response.txt", 5_000);
        let backend = CodexBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "hello prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.success);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("response.txt")).unwrap(),
            "hello prompt"
        );
        assert!(!temp.path().join("JADE_SYMPHONY_PROMPT.md").exists());
    }

    #[test]
    fn subprocess_backend_uses_external_prompt_artifact_path() {
        let temp = tempfile::tempdir().unwrap();
        let prompt_path = temp.path().join("logs").join("prompts").join("prompt.md");
        let config = codex_config(
            "printf '%s' \"$JADE_SYMPHONY_PROMPT_PATH\" > prompt_path.txt",
            5_000,
        );
        let backend = CodexBackend;
        let mut prepared = backend
            .prepare(
                temp.path().join("workspace"),
                "hello prompt".into(),
                &config,
            )
            .unwrap();
        std::fs::create_dir_all(&prepared.workspace).unwrap();
        prepared.prompt_artifact_path = Some(prompt_path.clone());
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.success);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("workspace").join("prompt_path.txt")).unwrap(),
            prompt_path.display().to_string()
        );
        assert_eq!(
            std::fs::read_to_string(&prompt_path).unwrap(),
            "hello prompt"
        );
        assert!(!temp
            .path()
            .join("workspace")
            .join("JADE_SYMPHONY_PROMPT.md")
            .exists());
    }

    #[test]
    fn codex_backend_refuses_app_server_command_without_launching() {
        let temp = tempfile::tempdir().unwrap();
        let config = codex_config("codex app-server", 5_000);
        let backend = CodexBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "hello prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary
            .message
            .contains("app-server transport is not implemented"));
        assert!(!temp.path().join("JADE_SYMPHONY_PROMPT.md").exists());
    }

    #[test]
    fn codex_app_server_command_guard_is_specific() {
        assert!(is_codex_app_server_command("codex app-server"));
        assert!(is_codex_app_server_command(
            "env CODEX_HOME=/tmp/codex /opt/bin/codex app-server --port 0"
        ));
        assert!(is_codex_app_server_command("exec 'codex' app-server"));

        assert!(!is_codex_app_server_command("cat > response.txt"));
        assert!(!is_codex_app_server_command("echo codex app-server"));
        assert!(!is_codex_app_server_command("codex exec app-server"));
    }

    #[test]
    fn codex_backend_prepared_run_includes_profile_context() {
        let temp = tempfile::tempdir().unwrap();
        let config = codex_config_with_profile("printf profile");
        let backend = CodexBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "prompt".into(), &config)
            .unwrap();

        assert_eq!(prepared.profile_id.as_deref(), Some("codex-alpha"));
        assert_eq!(prepared.instance_name.as_deref(), Some("codex-alpha"));
        assert_eq!(
            prepared.env.get("CODEX_HOME"),
            Some(&"/tmp/cockpit/codex-alpha".into())
        );
        assert_eq!(
            session_id_with_profile("codex-subprocess", &prepared),
            "codex-subprocess:codex-alpha"
        );
    }

    #[test]
    fn codex_backend_reports_subprocess_failure() {
        let temp = tempfile::tempdir().unwrap();
        let config = codex_config("echo nope >&2; exit 7", 5_000);
        let backend = CodexBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("status 7"));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Message { text, .. } if text.contains("nope")
        )));
    }

    #[test]
    fn codex_backend_reports_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let config = codex_config("sleep 1", 10);
        let backend = CodexBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("timed out"));
    }

    #[test]
    fn claude_code_backend_runs_subprocess_in_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let config = claude_config("cat > claude-subprocess-output.md", 5_000);
        let backend = ClaudeCodeBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "hello claude".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(summary.success);
        assert_eq!(summary.backend, "claude-code");
        assert_eq!(
            summary.session_id.as_deref(),
            Some("claude-code-subprocess")
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("claude-subprocess-output.md")).unwrap(),
            "hello claude"
        );
    }

    #[test]
    fn claude_code_backend_reports_subprocess_failure() {
        let temp = tempfile::tempdir().unwrap();
        let config = claude_config("echo claude-nope >&2; exit 9", 5_000);
        let backend = ClaudeCodeBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("status 9"));
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::Message { text, .. } if text.contains("claude-nope")
        )));
    }

    #[test]
    fn claude_code_backend_reports_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let config = claude_config("sleep 1", 10);
        let backend = ClaudeCodeBackend;
        let prepared = backend
            .prepare(temp.path().to_path_buf(), "prompt".into(), &config)
            .unwrap();
        let events = backend.run(prepared).unwrap();
        let summary = backend.summarize(&events);

        assert!(!summary.success);
        assert!(summary.message.contains("timed out"));
    }
}
